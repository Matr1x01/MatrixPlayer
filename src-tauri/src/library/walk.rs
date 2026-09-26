//! Directory walking for the scanner: finds audio files under a root without
//! reading them, so unchanged files can be skipped by size and mtime alone.

use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use crate::filesystem::has_audio_extension;

/// Deep enough for any real library, shallow enough to stop runaway trees.
const MAX_DEPTH: usize = 40;

#[derive(Debug, Clone)]
pub struct FoundFile {
    pub path: PathBuf,
    pub size: u64,
    /// Modification time in milliseconds since the epoch.
    pub mtime: i64,
}

#[derive(Debug, Default)]
pub struct Walk {
    pub files: Vec<FoundFile>,
    /// Directories that could not be read (permissions, I/O errors).
    pub unreadable: Vec<PathBuf>,
    pub cancelled: bool,
}

/// Recursively lists audio files under `root`.
///
/// Hidden entries (dot files) are skipped. Symbolic links are followed, but
/// each real directory is visited once, so link cycles and links back into
/// the tree cannot duplicate or loop. `on_dir` is called for each directory
/// entered, for progress reporting.
pub fn walk(root: &Path, cancel: &AtomicBool, mut on_dir: impl FnMut(&Path, usize)) -> Walk {
    let mut out = Walk::default();
    let mut visited = HashSet::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let Ok(meta) = std::fs::metadata(&dir) else {
            out.unreadable.push(dir);
            continue;
        };
        if !visited.insert((meta.dev(), meta.ino())) || depth > MAX_DEPTH {
            continue;
        }
        on_dir(&dir, out.files.len());
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("cannot read {}: {e}", dir.display());
                out.unreadable.push(dir);
                continue;
            }
        };
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.as_encoded_bytes().first() == Some(&b'.') {
                continue;
            }
            let path = entry.path();
            // `metadata` follows links, so a link to a file or folder is
            // treated as what it points to.
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if meta.is_dir() {
                subdirs.push(path);
            } else if meta.is_file() && has_audio_extension(&path) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
                out.files.push(FoundFile { path, size: meta.len(), mtime });
            }
        }
        // Reverse so the stack pops directories in name order.
        subdirs.sort_by(|a, b| b.cmp(a));
        stack.extend(subdirs.into_iter().map(|d| (d, depth + 1)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_audio_skips_hidden_and_survives_link_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("A/Disc 1")).unwrap();
        std::fs::create_dir_all(root.join(".trash")).unwrap();
        std::fs::write(root.join("A/Disc 1/01.flac"), "x").unwrap();
        std::fs::write(root.join("A/cover.jpg"), "x").unwrap();
        std::fs::write(root.join(".trash/old.mp3"), "x").unwrap();
        std::fs::write(root.join("A/.hidden.flac"), "x").unwrap();
        std::os::unix::fs::symlink(root, root.join("A/loop")).unwrap();

        let walk = walk(root, &AtomicBool::new(false), |_, _| {});
        let names: Vec<_> = walk.files.iter().map(|f| f.path.strip_prefix(root).unwrap().to_owned()).collect();
        assert_eq!(names, [PathBuf::from("A/Disc 1/01.flac")]);
        assert_eq!(walk.files[0].size, 1);
        assert!(walk.files[0].mtime > 0);
    }

    #[test]
    fn stops_when_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.flac"), "x").unwrap();
        let walk = walk(dir.path(), &AtomicBool::new(true), |_, _| {});
        assert!(walk.cancelled);
        assert!(walk.files.is_empty());
    }
}
