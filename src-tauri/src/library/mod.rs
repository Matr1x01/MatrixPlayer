//! The music library: folders the user added, indexed into SQLite.
//!
//! [`LibraryHandle`] is the only way in. Reads go straight to the database
//! ([`query`]); everything that changes which roots exist or what is indexed
//! (adding and removing folders, scans) is serialised through one background
//! task, so a scan never races with the removal of the folder it is reading.
//! File-system changes under the roots trigger incremental rescans.

mod index;
pub mod listening;
mod paths;
pub mod playlists;
pub mod query;
mod scan;
mod text;
mod walk;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot};

pub use scan::{ScanPhase, ScanStatus};

use self::paths::{path_bytes, path_from_bytes};
use self::query::TrackQuery;
use self::scan::{ScanContext, ScanRoot, validate_new_root};
use crate::artwork::ArtworkStore;
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::filesystem;
use crate::player::TrackInfo;

/// Quiet time after the last file-system change before a rescan starts.
const WATCH_DEBOUNCE: Duration = Duration::from_secs(3);

/// What changed, so the UI refetches only what it shows.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LibraryChange {
    /// Tracks, albums, artists, genres or folders.
    Tracks,
    Roots,
    Favorites,
    History,
    Playlists,
}

#[derive(Debug, Clone)]
pub enum LibraryEvent {
    Scan(ScanStatus),
    Changed(LibraryChange),
}

/// Tracks to play, as the UI describes them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PlaySource {
    /// Exactly these tracks, in this order.
    Tracks { ids: Vec<i64> },
    /// Whatever a track query currently returns (an album, a folder…).
    Query { query: TrackQuery },
}

enum Request {
    Scan(Option<Vec<i64>>),
    AddRoot(PathBuf, oneshot::Sender<AppResult<i64>>),
    RemoveRoot(i64, oneshot::Sender<AppResult<()>>),
}

#[derive(Clone)]
pub struct LibraryHandle {
    db: Database,
    events: broadcast::Sender<LibraryEvent>,
    requests: mpsc::UnboundedSender<Request>,
    status: Arc<Mutex<ScanStatus>>,
    cancel: Arc<AtomicBool>,
}

impl LibraryHandle {
    /// Creates the library. The returned future is its background task.
    pub fn new(db: Database, artwork: Arc<ArtworkStore>) -> (Self, impl std::future::Future<Output = ()> + Send) {
        let (events, _) = broadcast::channel(256);
        let (requests, requests_rx) = mpsc::unbounded_channel();
        let handle = Self {
            db,
            events,
            requests,
            status: Arc::new(Mutex::new(ScanStatus::default())),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let task = Worker { handle: handle.clone(), artwork, watcher: None }.run(requests_rx);
        (handle, task)
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LibraryEvent> {
        self.events.subscribe()
    }

    pub fn emit(&self, event: LibraryEvent) {
        let _ = self.events.send(event);
    }

    pub fn status(&self) -> ScanStatus {
        self.status.lock().unwrap().clone()
    }

    /// Rescans one root, or all of them.
    pub fn rescan(&self, root: Option<i64>) {
        let _ = self.requests.send(Request::Scan(root.map(|r| vec![r])));
    }

    /// Adds `path` as a library folder and starts indexing it.
    pub async fn add_root(&self, path: PathBuf) -> AppResult<i64> {
        let (reply, result) = oneshot::channel();
        // Let a running scan stop so the new folder is handled promptly.
        self.cancel.store(true, Ordering::SeqCst);
        self.requests.send(Request::AddRoot(path, reply)).map_err(|_| AppError::PlayerUnavailable)?;
        result.await.map_err(|_| AppError::PlayerUnavailable)?
    }

    /// Removes a library folder and everything indexed from it. The files
    /// themselves are not touched.
    pub async fn remove_root(&self, id: i64) -> AppResult<()> {
        let (reply, result) = oneshot::channel();
        self.cancel.store(true, Ordering::SeqCst);
        self.requests.send(Request::RemoveRoot(id, reply)).map_err(|_| AppError::PlayerUnavailable)?;
        result.await.map_err(|_| AppError::PlayerUnavailable)?
    }

    /// Stops any running scan (the app is exiting).
    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Resolves a play request to player tracks, in order.
    pub async fn resolve(&self, source: PlaySource) -> AppResult<Vec<Arc<TrackInfo>>> {
        let ids = match source {
            PlaySource::Tracks { ids } => ids,
            PlaySource::Query { query } => query::track_list(&self.db, &query).await?.ids,
        };
        let tracks = query::track_infos(&self.db, &ids).await?;
        if tracks.is_empty() {
            return Err(AppError::NotFound("none of these tracks are available".into()));
        }
        Ok(tracks)
    }

    /// Turns files and folders opened from outside the library into player
    /// tracks: indexed files come from the database (with their library
    /// identity), the rest are read from disk.
    pub async fn resolve_files(
        &self,
        artwork: Arc<ArtworkStore>,
        paths: Vec<PathBuf>,
    ) -> AppResult<Vec<Arc<TrackInfo>>> {
        let files = tokio::task::spawn_blocking(move || filesystem::collect_audio_files(&paths))
            .await
            .map_err(|e| AppError::Io(std::io::Error::other(e)))?;
        let mut known = query::track_infos_for_paths(&self.db, &files).await?;
        let tracks = tokio::task::spawn_blocking(move || {
            files
                .iter()
                .map(|path| known.remove(path).unwrap_or_else(|| Arc::new(TrackInfo::load(path, &artwork))))
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| AppError::Io(std::io::Error::other(e)))?;
        if tracks.is_empty() {
            return Err(AppError::Unsupported("no playable audio files were found".into()));
        }
        Ok(tracks)
    }

    pub async fn set_favorite(&self, ids: &[i64], favorite: bool) -> AppResult<()> {
        for chunk in ids.chunks(500) {
            let list = chunk.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            let sql = if favorite {
                format!("UPDATE tracks SET favorite_at = unixepoch() WHERE favorite_at IS NULL AND id IN ({list})")
            } else {
                format!("UPDATE tracks SET favorite_at = NULL WHERE id IN ({list})")
            };
            sqlx::query(&sql).execute(self.db.pool()).await?;
        }
        self.emit(LibraryEvent::Changed(LibraryChange::Favorites));
        Ok(())
    }

    pub async fn favorite_ids(&self) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar("SELECT id FROM tracks WHERE favorite_at IS NOT NULL").fetch_all(self.db.pool()).await?)
    }

    /// Absolute path of an indexed folder, for opening it in the file manager.
    pub async fn folder_path(&self, id: i64) -> AppResult<PathBuf> {
        let path: Vec<u8> = sqlx::query_scalar("SELECT path FROM folders WHERE id = ?")
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?
            .ok_or_else(|| AppError::NotFound("folder".into()))?;
        Ok(path_from_bytes(&path))
    }

    /// Folder id of an indexed track, for "Browse folder".
    pub async fn track_folder(&self, id: i64) -> AppResult<i64> {
        sqlx::query_scalar("SELECT folder_id FROM tracks WHERE id = ?")
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?
            .ok_or_else(|| AppError::NotFound("track".into()))
    }
}

struct Worker {
    handle: LibraryHandle,
    artwork: Arc<ArtworkStore>,
    watcher: Option<notify::RecommendedWatcher>,
}

impl Worker {
    async fn run(mut self, mut requests: mpsc::UnboundedReceiver<Request>) {
        // Scan everything once at startup, like a quick "what changed while
        // the app was closed" pass.
        let mut pending: Option<BTreeSet<i64>> = Some(BTreeSet::new());
        let mut all = true;
        self.restart_watcher().await;
        loop {
            // Handle everything already queued before (re)starting a scan.
            let request = if pending.is_some() {
                match requests.try_recv() {
                    Ok(r) => Some(r),
                    Err(mpsc::error::TryRecvError::Empty) => None,
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            } else {
                match requests.recv().await {
                    Some(r) => Some(r),
                    None => break,
                }
            };
            if let Some(request) = request {
                match request {
                    Request::Scan(None) => {
                        all = true;
                        pending.get_or_insert_default();
                    }
                    Request::Scan(Some(ids)) => pending.get_or_insert_default().extend(ids),
                    Request::AddRoot(path, reply) => {
                        let result = self.add_root(&path).await;
                        if let Ok(id) = result {
                            pending.get_or_insert_default().insert(id);
                        }
                        let _ = reply.send(result);
                    }
                    Request::RemoveRoot(id, reply) => {
                        if let Some(p) = pending.as_mut() {
                            p.remove(&id);
                        }
                        let _ = reply.send(self.remove_root(id).await);
                    }
                }
                continue;
            }
            let Some(targets) = pending.take() else { continue };
            let only = if all { None } else { Some(targets) };
            all = false;
            if let Some(unfinished) = self.scan(only).await {
                pending = Some(unfinished);
            }
        }
    }

    /// Scans the given roots (or all). Returns the roots left unscanned when
    /// the scan was interrupted.
    async fn scan(&mut self, only: Option<BTreeSet<i64>>) -> Option<BTreeSet<i64>> {
        let h = &self.handle;
        h.cancel.store(false, Ordering::SeqCst);
        let roots = match load_roots(&h.db).await {
            Ok(roots) => roots,
            Err(e) => {
                log::error!("could not load library roots: {e}");
                return None;
            }
        };
        let roots: Vec<ScanRoot> =
            roots.into_iter().filter(|r| only.as_ref().is_none_or(|ids| ids.contains(&r.id))).collect();
        if roots.is_empty() {
            return None;
        }

        let mut status = ScanStatus { finished_at: h.status().finished_at, ..Default::default() };
        let report = {
            let shared = h.status.clone();
            let events = h.events.clone();
            move |s: &ScanStatus| {
                *shared.lock().unwrap() = s.clone();
                let _ = events.send(LibraryEvent::Scan(s.clone()));
            }
        };
        let publish = {
            let events = h.events.clone();
            move || {
                let _ = events.send(LibraryEvent::Changed(LibraryChange::Tracks));
            }
        };
        let ctx =
            ScanContext { db: &h.db, artwork: &self.artwork, cancel: &h.cancel, report: &report, publish: &publish };
        let mut unfinished = BTreeSet::new();
        let mut interrupted = false;
        for root in &roots {
            if interrupted {
                unfinished.insert(root.id);
                continue;
            }
            match scan::scan_root(&ctx, root, &mut status).await {
                Ok(outcome) if outcome.cancelled => {
                    interrupted = true;
                    unfinished.insert(root.id);
                }
                Ok(outcome) => {
                    if outcome.offline {
                        status.offline.push(root.display.clone());
                    }
                    log::info!(
                        "scanned {}: {} added, {} updated, {} removed",
                        root.display,
                        outcome.added,
                        outcome.updated,
                        outcome.removed
                    );
                }
                Err(e) => log::error!("scan of {} failed: {e}", root.display),
            }
        }
        status.phase = ScanPhase::Idle;
        status.current = None;
        if !interrupted {
            status.finished_at = Some(now());
        }
        report(&status);
        publish();
        interrupted.then_some(unfinished)
    }

    async fn add_root(&mut self, path: &Path) -> AppResult<i64> {
        let db = &self.handle.db;
        let existing: Vec<(i64, PathBuf)> = load_roots(db).await?.into_iter().map(|r| (r.id, r.path)).collect();
        let (root, absorbed) = {
            let path = path.to_path_buf();
            tokio::task::spawn_blocking(move || validate_new_root(&path, &existing))
                .await
                .map_err(|e| AppError::Io(std::io::Error::other(e)))??
        };
        let mut tx = db.pool().begin().await?;
        let (id,): (i64,) = sqlx::query_as("INSERT INTO library_roots (path, display) VALUES (?, ?) RETURNING id")
            .bind(path_bytes(&root))
            .bind(display_path(&root))
            .fetch_one(&mut *tx)
            .await?;
        // A parent of existing roots takes over their tracks, keeping their
        // favorites and play counts.
        for child in absorbed {
            sqlx::query("UPDATE tracks SET root_id = ? WHERE root_id = ?")
                .bind(id)
                .bind(child)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE folders SET root_id = ? WHERE root_id = ?")
                .bind(id)
                .bind(child)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM library_roots WHERE id = ?").bind(child).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        self.restart_watcher().await;
        self.handle.emit(LibraryEvent::Changed(LibraryChange::Roots));
        Ok(id)
    }

    async fn remove_root(&mut self, id: i64) -> AppResult<()> {
        let db = &self.handle.db;
        let removed = sqlx::query("DELETE FROM library_roots WHERE id = ?").bind(id).execute(db.pool()).await?;
        if removed.rows_affected() == 0 {
            return Err(AppError::NotFound("library folder".into()));
        }
        index::refresh_albums(db, None).await?;
        self.restart_watcher().await;
        self.handle.emit(LibraryEvent::Changed(LibraryChange::Roots));
        self.handle.emit(LibraryEvent::Changed(LibraryChange::Tracks));
        Ok(())
    }

    /// Watches every online root for changes, replacing the previous watcher.
    async fn restart_watcher(&mut self) {
        self.watcher = None;
        let handle = self.handle.clone();
        let (changes_tx, changes_rx) = mpsc::unbounded_channel::<PathBuf>();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            let relevant = matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(notify::event::ModifyKind::Data(_) | notify::event::ModifyKind::Name(_))
                    | EventKind::Access(notify::event::AccessKind::Close(notify::event::AccessMode::Write))
            );
            if !relevant {
                return;
            }
            for path in event.paths {
                let hidden = path.file_name().is_some_and(|n| n.as_encoded_bytes().first() == Some(&b'.'));
                if !hidden {
                    let _ = changes_tx.send(path);
                }
            }
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                log::warn!("file watching is unavailable: {e}");
                return;
            }
        };
        let roots = load_roots(&handle.db).await.unwrap_or_default();
        let mut watched = Vec::new();
        for root in &roots {
            if !root.path.is_dir() {
                continue;
            }
            match watcher.watch(&root.path, RecursiveMode::Recursive) {
                Ok(()) => watched.push((root.id, root.path.clone())),
                // Usually the inotify watch limit; manual rescans still work.
                Err(e) => log::warn!("cannot watch {} for changes: {e}", root.display),
            }
        }
        tokio::spawn(debounce_changes(changes_rx, watched, handle));
        self.watcher = Some(watcher);
    }
}

/// Collects file-system changes and asks for a rescan of the affected roots
/// once things have been quiet for [`WATCH_DEBOUNCE`].
async fn debounce_changes(
    mut changes: mpsc::UnboundedReceiver<PathBuf>,
    roots: Vec<(i64, PathBuf)>,
    handle: LibraryHandle,
) {
    let mut dirty = BTreeSet::new();
    loop {
        let next = if dirty.is_empty() {
            changes.recv().await.map(Some)
        } else {
            match tokio::time::timeout(WATCH_DEBOUNCE, changes.recv()).await {
                Ok(change) => change.map(Some),
                Err(_) => Some(None),
            }
        };
        match next {
            None => break,
            Some(Some(path)) => {
                if let Some((id, _)) = roots.iter().find(|(_, root)| path.starts_with(root)) {
                    dirty.insert(*id);
                }
            }
            Some(None) => {
                log::debug!("library files changed; rescanning {dirty:?}");
                let _ = handle.requests.send(Request::Scan(Some(std::mem::take(&mut dirty).into_iter().collect())));
            }
        }
    }
}

async fn load_roots(db: &Database) -> AppResult<Vec<ScanRoot>> {
    let rows: Vec<(i64, Vec<u8>, String)> =
        sqlx::query_as("SELECT id, path, display FROM library_roots ORDER BY id").fetch_all(db.pool()).await?;
    Ok(rows.into_iter().map(|(id, path, display)| ScanRoot { id, path: path_from_bytes(&path), display }).collect())
}

/// Paths under the home directory are shown as `~/…`.
fn display_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() { "~".to_owned() } else { format!("~/{}", rest.display()) };
    }
    path.display().to_string()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::query::{self, ArtistKind, TrackScope};
    use super::*;
    use crate::test_support::write_flac_tags;

    struct Lib {
        library: LibraryHandle,
        events: broadcast::Receiver<LibraryEvent>,
        dir: tempfile::TempDir,
    }

    async fn library() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::in_memory().await.unwrap();
        let artwork = Arc::new(ArtworkStore::new(dir.path().join("art")).unwrap());
        let (library, task) = LibraryHandle::new(db, artwork);
        let events = library.subscribe();
        tokio::spawn(task);
        Lib { library, events, dir }
    }

    impl Lib {
        fn music(&self) -> PathBuf {
            self.dir.path().join("Music")
        }

        /// Waits until a scan has finished.
        async fn scanned(&mut self) {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
            loop {
                match tokio::time::timeout_at(deadline, self.events.recv()).await {
                    Ok(Ok(LibraryEvent::Scan(s))) if s.phase == ScanPhase::Idle && s.finished_at.is_some() => return,
                    Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
                    other => panic!("no finished scan: {other:?}"),
                }
            }
        }

        async fn titles(&self, scope: TrackScope, filter: &str) -> Vec<String> {
            let db = self.library.db();
            let q = TrackQuery {
                scope,
                filter: filter.into(),
                sort: Default::default(),
                descending: false,
                hi_res_only: false,
            };
            let ids = query::track_list(db, &q).await.unwrap().ids;
            query::tracks_by_ids(db, &ids).await.unwrap().into_iter().map(|t| t.title).collect()
        }
    }

    fn album(dir: &Path, artist: &str, album: &str, titles: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        for (i, title) in titles.iter().enumerate() {
            let tags = format!(
                "title=\"{title}\",artist=\"{artist}\",album-artist=\"{artist}\",album=\"{album}\",\
                 genre=\"Ambient\",track-number=(uint){}",
                i + 1
            );
            write_flac_tags(&dir.join(format!("{:02} {title}.flac", i + 1)), 0.2, &tags);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn indexes_browses_searches_and_follows_changes() {
        gstreamer::init().unwrap();
        let mut lib = library().await;
        let music = lib.music();
        album(
            &music.join("Halden Reach/Low Frequency Weather"),
            "Halden Reach",
            "Low Frequency Weather",
            &["Pressure Front", "Isobar", "Rain Shadow"],
        );
        album(&music.join("Sigur Rós/Takk"), "Sigur Rós", "Takk", &["Glósóli", "Hoppípolla"]);
        // A compilation without album-artist tags, split into disc folders.
        let comp = music.join("Various/Night Mix");
        std::fs::create_dir_all(comp.join("CD 1")).unwrap();
        std::fs::create_dir_all(comp.join("CD 2")).unwrap();
        write_flac_tags(&comp.join("CD 1/01.flac"), 0.2, "title=\"One\",artist=\"Ada\",album=\"Night Mix\"");
        write_flac_tags(&comp.join("CD 2/01.flac"), 0.2, "title=\"Two\",artist=\"Bea\",album=\"Night Mix\"");

        let root = lib.library.add_root(music.clone()).await.unwrap();
        lib.scanned().await;
        let db = lib.library.db().clone();

        let stats = query::stats(&db).await.unwrap();
        assert_eq!((stats.tracks, stats.albums, stats.genres), (7, 3, 1));

        let albums = query::albums(&db).await.unwrap();
        let names: Vec<_> = albums.iter().map(|a| (a.artist_name.as_str(), a.title.as_str(), a.track_count)).collect();
        assert_eq!(
            names,
            [
                ("Halden Reach", "Low Frequency Weather", 3),
                ("Sigur Rós", "Takk", 2),
                (index::VARIOUS_ARTISTS, "Night Mix", 2)
            ]
        );
        let lfw = &albums[0];
        assert_eq!((lfw.codec.as_deref(), lfw.sample_rate, lfw.bit_depth), (Some("FLAC"), Some(44_100), Some(16)));

        let detail = query::album(&db, lfw.id).await.unwrap();
        let order: Vec<_> = detail.tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(order, ["Pressure Front", "Isobar", "Rain Shadow"]);
        assert_eq!(detail.genres, ["Ambient"]);

        let artists = query::artists(&db, ArtistKind::AlbumArtists).await.unwrap();
        assert_eq!(artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Halden Reach", "Sigur Rós"]);
        let all = query::artists(&db, ArtistKind::All).await.unwrap();
        assert_eq!(all.len(), 4, "compilation artists are listed under all artists");

        // Search folds accents and matches word prefixes, across fields.
        assert_eq!(lib.titles(TrackScope::All, "hoppi").await, ["Hoppípolla"]);
        assert_eq!(lib.titles(TrackScope::All, "sigur glo").await, ["Glósóli"]);
        assert_eq!(lib.titles(TrackScope::All, "rain").await, ["Rain Shadow"]);
        let results = query::search(&db, "takk", 6).await.unwrap();
        assert!(matches!(results.top, Some(query::TopResult::Album { ref album }) if album.title == "Takk"));
        assert_eq!(results.track_count, 2);

        // Folders mirror the tree, disc folders included.
        let roots = query::roots(&db).await.unwrap();
        let root_folder = roots[0].folder_id.unwrap();
        let top: Vec<_> = query::folder_children(&db, root_folder)
            .await
            .unwrap()
            .into_iter()
            .map(|f| (f.name, f.track_count))
            .collect();
        assert_eq!(top, [("Halden Reach".to_owned(), 3), ("Sigur Rós".to_owned(), 2), ("Various".to_owned(), 2)]);

        // Favorites and play counts survive a rename.
        let isobar = query::track_list(
            &db,
            &TrackQuery {
                scope: TrackScope::All,
                filter: "isobar".into(),
                sort: Default::default(),
                descending: false,
                hi_res_only: false,
            },
        )
        .await
        .unwrap()
        .ids[0];
        lib.library.set_favorite(&[isobar], true).await.unwrap();
        listening::record_play(&lib.library, isobar, 60_000).await.unwrap();
        let lfw_dir = music.join("Halden Reach/Low Frequency Weather");
        std::fs::rename(lfw_dir.join("02 Isobar.flac"), lfw_dir.join("02 - Isobar (remaster).flac")).unwrap();
        std::fs::remove_file(music.join("Sigur Rós/Takk/01 Glósóli.flac")).unwrap();
        lib.library.rescan(Some(root));
        lib.scanned().await;

        let favorites = lib.titles(TrackScope::Favorites, "").await;
        assert_eq!(favorites, ["Isobar"], "the moved file kept its identity");
        let row = &query::tracks_by_ids(&db, &[isobar]).await.unwrap()[0];
        assert_eq!((row.file_name.as_str(), row.play_count), ("02 - Isobar (remaster).flac", 1));
        assert_eq!(query::stats(&db).await.unwrap().tracks, 6);
        assert!(lib.titles(TrackScope::All, "glosoli").await.is_empty(), "deleted files leave the search index");

        // Playing from the library builds player tracks from the database.
        let tracks = lib
            .library
            .resolve(PlaySource::Query {
                query: TrackQuery {
                    scope: TrackScope::Album { id: lfw.id },
                    filter: String::new(),
                    sort: Default::default(),
                    descending: false,
                    hi_res_only: false,
                },
            })
            .await
            .unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[1].library_id, Some(isobar));
        assert_eq!(tracks[1].metadata.album_artist.as_deref(), Some("Halden Reach"));
        assert!(tracks[1].path.ends_with("02 - Isobar (remaster).flac"));

        // Removing the folder removes what was indexed from it.
        lib.library.remove_root(root).await.unwrap();
        let stats = query::stats(&db).await.unwrap();
        assert_eq!((stats.tracks, stats.albums, stats.genres, stats.roots), (0, 0, 0, 0));
        let fts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM track_search").fetch_one(db.pool()).await.unwrap();
        assert_eq!(fts, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn offline_roots_keep_their_tracks() {
        gstreamer::init().unwrap();
        let mut lib = library().await;
        let music = lib.music();
        album(&music.join("A/B"), "Artist", "Album", &["Song"]);
        let root = lib.library.add_root(music.clone()).await.unwrap();
        lib.scanned().await;

        let unplugged = lib.dir.path().join("unplugged");
        std::fs::rename(&music, &unplugged).unwrap();
        lib.library.rescan(Some(root));
        lib.scanned().await;
        let db = lib.library.db().clone();
        let stats = query::stats(&db).await.unwrap();
        assert_eq!((stats.tracks, stats.unavailable), (1, 1));
        assert!(lib.library.status().offline.iter().any(|r| r.ends_with("Music")));

        std::fs::rename(&unplugged, &music).unwrap();
        lib.library.rescan(Some(root));
        lib.scanned().await;
        assert_eq!(query::stats(&db).await.unwrap().unavailable, 0);
    }
}
