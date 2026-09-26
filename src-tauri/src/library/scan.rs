//! One scan of one library root.
//!
//! ```text
//! walk (stat only) ─▶ diff against the database ─▶ probe new/changed files
//!   on a thread pool ─▶ write in batches ─▶ delete what is gone ─▶ refresh
//!   album aggregates
//! ```
//!
//! Unchanged files (same size and mtime) are never opened, so rescanning a
//! large, stable library costs little more than listing its directories. A
//! file that disappeared while an identical one (same size and mtime) appeared
//! elsewhere is treated as moved: its row keeps its id, and with it favorites,
//! play counts and playlist entries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::mpsc;

use super::index::{Indexer, ScannedTrack, refresh_albums};
use super::paths::path_from_bytes;
use super::walk::{FoundFile, walk};
use crate::artwork::{ArtworkStore, FolderArtCache};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::player::TrackInfo;

const BATCH: usize = 200;
/// How often, at most, progress is reported and partial results published.
const REPORT_INTERVAL: Duration = Duration::from_millis(120);
const PUBLISH_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    #[default]
    Idle,
    /// Listing folders.
    Discovering,
    /// Reading tags and artwork of new and changed files.
    Reading,
    /// Removing missing files and updating albums.
    Finishing,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub phase: ScanPhase,
    /// The root being scanned, for display.
    pub root: Option<String>,
    /// The folder or file currently being looked at.
    pub current: Option<String>,
    /// Files handled / files to handle in the reading phase.
    pub processed: usize,
    pub total: usize,
    /// Audio files found so far (discovering) or in total (later phases).
    pub found: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// Folders that could not be read.
    pub unreadable: usize,
    /// Roots that were offline during the last scan.
    pub offline: Vec<String>,
    /// When the last complete scan finished (seconds since the epoch).
    pub finished_at: Option<i64>,
}

pub struct ScanRoot {
    pub id: i64,
    pub path: PathBuf,
    pub display: String,
}

pub struct ScanContext<'a> {
    pub db: &'a Database,
    pub artwork: &'a Arc<ArtworkStore>,
    pub cancel: &'a Arc<AtomicBool>,
    /// Receives progress (throttled) and is expected to publish it.
    pub report: &'a (dyn Fn(&ScanStatus) + Send + Sync),
    /// Called when enough new data was written to be worth showing.
    pub publish: &'a (dyn Fn() + Send + Sync),
    /// Re-read files even when their size and mtime are unchanged.
    pub full: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanOutcome {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub offline: bool,
    pub cancelled: bool,
}

struct Existing {
    id: i64,
    size: i64,
    mtime: i64,
    available: bool,
}

/// Scans `root`, updating `status` as it goes.
pub async fn scan_root(ctx: &ScanContext<'_>, root: &ScanRoot, status: &mut ScanStatus) -> AppResult<ScanOutcome> {
    status.root = Some(root.display.clone());
    status.phase = ScanPhase::Discovering;
    status.processed = 0;
    status.total = 0;
    status.found = 0;
    (ctx.report)(status);

    if !root.path.is_dir() {
        // An unmounted drive or network share: keep its tracks, but mark them
        // unavailable until it comes back.
        log::info!("library root {} is offline", root.display);
        sqlx::query("UPDATE tracks SET available = 0 WHERE root_id = ?").bind(root.id).execute(ctx.db.pool()).await?;
        return Ok(ScanOutcome { offline: true, ..Default::default() });
    }

    // Walking is blocking I/O; report the folder being listed as we go.
    let found = {
        let path = root.path.clone();
        let cancel = ctx.cancel.clone();
        let progress = Arc::new(Mutex::new((String::new(), 0usize)));
        let seen = progress.clone();
        let mut walk_task = tokio::task::spawn_blocking(move || {
            walk(&path, &cancel, |dir, found| *seen.lock().unwrap() = (dir.display().to_string(), found))
        });
        let mut ticker = tokio::time::interval(REPORT_INTERVAL);
        loop {
            tokio::select! {
                result = &mut walk_task => break result.map_err(|e| AppError::Io(std::io::Error::other(e)))?,
                _ = ticker.tick() => {
                    let (dir, count) = progress.lock().unwrap().clone();
                    status.current = Some(dir);
                    status.found = count;
                    (ctx.report)(status);
                }
            }
        }
    };
    status.unreadable += found.unreadable.len();
    if found.cancelled {
        return Ok(ScanOutcome { cancelled: true, ..Default::default() });
    }
    status.found = found.files.len();

    let existing: Vec<(Vec<u8>, i64, i64, i64, bool)> =
        sqlx::query_as("SELECT path, id, file_size, mtime, available FROM tracks WHERE root_id = ?")
            .bind(root.id)
            .fetch_all(ctx.db.pool())
            .await?;
    let mut existing: HashMap<PathBuf, Existing> = existing
        .into_iter()
        .map(|(path, id, size, mtime, available)| (path_from_bytes(&path), Existing { id, size, mtime, available }))
        .collect();

    // Split into unchanged, changed and new files.
    let mut jobs: Vec<(FoundFile, Option<i64>)> = Vec::new();
    let mut new_files = Vec::new();
    let mut reavailable = Vec::new();
    for file in found.files {
        match existing.remove(&file.path) {
            Some(row) if !ctx.full && row.size == file.size as i64 && row.mtime == file.mtime => {
                if !row.available {
                    reavailable.push(row.id);
                }
            }
            Some(row) => jobs.push((file, Some(row.id))),
            None => new_files.push(file),
        }
    }
    // What is left in `existing` is gone from disk. A new file identical in
    // size and mtime to exactly one of them was moved or renamed.
    let mut by_identity: HashMap<(i64, i64), Vec<i64>> = HashMap::new();
    for row in existing.values() {
        by_identity.entry((row.size, row.mtime)).or_default().push(row.id);
    }
    let mut moved = std::collections::HashSet::new();
    for file in new_files {
        let id = match by_identity.get_mut(&(file.size as i64, file.mtime)) {
            Some(ids) if ids.len() == 1 => ids.pop(),
            _ => None,
        };
        if let Some(id) = id {
            moved.insert(id);
        }
        jobs.push((file, id));
    }
    let missing: Vec<i64> = existing.values().map(|r| r.id).filter(|id| !moved.contains(id)).collect();

    if !reavailable.is_empty() {
        for chunk in reavailable.chunks(500) {
            let list = chunk.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            sqlx::query(&format!("UPDATE tracks SET available = 1 WHERE id IN ({list})"))
                .execute(ctx.db.pool())
                .await?;
        }
    }

    let mut outcome = read_and_write(ctx, root, jobs, status).await?;
    if outcome.cancelled {
        return Ok(outcome);
    }

    status.phase = ScanPhase::Finishing;
    status.current = None;
    (ctx.report)(status);
    for chunk in missing.chunks(500) {
        let list = chunk.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        sqlx::query(&format!("DELETE FROM tracks WHERE id IN ({list})")).execute(ctx.db.pool()).await?;
    }
    outcome.removed = missing.len();
    status.removed += missing.len();
    refresh_albums(ctx.db, None).await?;
    sqlx::query("UPDATE library_roots SET last_scan_at = unixepoch() WHERE id = ?")
        .bind(root.id)
        .execute(ctx.db.pool())
        .await?;
    Ok(outcome)
}

/// Probes `jobs` on a thread pool and writes the results in batches.
async fn read_and_write(
    ctx: &ScanContext<'_>,
    root: &ScanRoot,
    jobs: Vec<(FoundFile, Option<i64>)>,
    status: &mut ScanStatus,
) -> AppResult<ScanOutcome> {
    let mut outcome = ScanOutcome::default();
    status.phase = ScanPhase::Reading;
    status.total = jobs.len();
    status.processed = 0;
    (ctx.report)(status);
    if jobs.is_empty() {
        return Ok(outcome);
    }

    let (tx, mut rx) = mpsc::channel::<ScannedTrack>(BATCH * 2);
    let probing = {
        let artwork = ctx.artwork.clone();
        let cancel = ctx.cancel.clone();
        tokio::task::spawn_blocking(move || probe_all(jobs, &artwork, &cancel, &tx))
    };

    let mut indexer = Indexer::new(root.id, root.path.clone());
    let mut batch = Vec::with_capacity(BATCH);
    let mut last_report = Instant::now();
    let mut last_publish = Instant::now();
    loop {
        let next = tokio::time::timeout(REPORT_INTERVAL, rx.recv()).await;
        let done = matches!(next, Ok(None));
        let idle = next.is_err();
        if let Ok(Some(track)) = next {
            status.current = Some(track.info.display_path.clone());
            if track.existing_id.is_some() {
                outcome.updated += 1;
                status.updated += 1;
            } else {
                outcome.added += 1;
                status.added += 1;
            }
            batch.push(track);
        }
        if batch.len() >= BATCH || ((done || idle) && !batch.is_empty()) {
            indexer.write(ctx.db, &batch).await?;
            status.processed += batch.len();
            batch.clear();
        }
        if last_report.elapsed() >= REPORT_INTERVAL || done {
            last_report = Instant::now();
            (ctx.report)(status);
        }
        if last_publish.elapsed() >= PUBLISH_INTERVAL && !indexer.touched_albums.is_empty() {
            // Let the library fill in while the scan continues.
            last_publish = Instant::now();
            let touched: Vec<i64> = indexer.touched_albums.drain().collect();
            refresh_albums(ctx.db, Some(&touched)).await?;
            (ctx.publish)();
        }
        if done {
            break;
        }
    }
    let cancelled = probing.await.map_err(|e| AppError::Io(std::io::Error::other(e)))?;
    outcome.cancelled = cancelled;
    Ok(outcome)
}

/// Reads every job's tags and artwork on all cores. Returns whether it was
/// cancelled.
fn probe_all(
    jobs: Vec<(FoundFile, Option<i64>)>,
    artwork: &ArtworkStore,
    cancel: &AtomicBool,
    out: &mpsc::Sender<ScannedTrack>,
) -> bool {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
    let next = AtomicUsize::new(0);
    let folder_art = FolderArtCache::default();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((file, existing_id)) = jobs.get(i) else { return };
                    let info = TrackInfo::load_cached(&file.path, artwork, Some(&folder_art));
                    let track = ScannedTrack { info, size: file.size, mtime: file.mtime, existing_id: *existing_id };
                    if out.blocking_send(track).is_err() {
                        return;
                    }
                }
            });
        }
    });
    cancel.load(Ordering::Relaxed)
}

/// Checks that `path` can become a library root: an existing directory that
/// does not overlap an existing root. Returns its canonical form and the ids
/// of existing roots inside it, which the new root absorbs.
pub fn validate_new_root(path: &Path, existing: &[(i64, PathBuf)]) -> AppResult<(PathBuf, Vec<i64>)> {
    let canonical = path.canonicalize().map_err(|_| AppError::NotFound(path.display().to_string()))?;
    if !canonical.is_dir() {
        return Err(AppError::InvalidArgument(format!("{} is not a folder", canonical.display())));
    }
    let mut inside = Vec::new();
    for (id, root) in existing {
        if canonical.starts_with(root) {
            return Err(AppError::InvalidArgument(format!("{} is already part of your library", canonical.display())));
        }
        if root.starts_with(&canonical) {
            inside.push(*id);
        }
    }
    Ok((canonical, inside))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nested_roots_and_absorbs_children() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music");
        let jazz = music.join("Jazz");
        std::fs::create_dir_all(&jazz).unwrap();
        let music = music.canonicalize().unwrap();
        let jazz = jazz.canonicalize().unwrap();

        assert!(validate_new_root(&jazz, &[(1, music.clone())]).is_err(), "inside an existing root");
        let (root, absorbed) = validate_new_root(&music, &[(2, jazz.clone())]).unwrap();
        assert_eq!(root, music);
        assert_eq!(absorbed, [2]);
        assert!(validate_new_root(&dir.path().join("missing"), &[]).is_err());
    }
}
