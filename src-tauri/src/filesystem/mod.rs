//! Filesystem helpers: which files count as audio, and validation of paths
//! that arrive from outside the Rust core (command line, drag and drop,
//! dialogs). Nothing here trusts a path it has not canonicalised.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Extensions MatrixPlayer offers to open. Playback itself is delegated to
/// GStreamer, so a listed format still fails gracefully if the matching
/// decoder plugin is not installed.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "m4a", "m4b", "mp4", "aac", "alac", "ogg", "oga", "opus", "wav", "wave", "aif", "aiff", "aifc",
    "wv", "ape", "mpc", "dsf", "dff", "mka", "spx", "wma",
];

pub fn has_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(ext)))
}

/// Resolves `path` to a canonical, existing, regular audio file.
pub fn validate_audio_file(path: &Path) -> AppResult<PathBuf> {
    let canonical = path.canonicalize().map_err(|_| AppError::NotFound(path.display().to_string()))?;
    let meta = std::fs::metadata(&canonical)?;
    if !meta.is_file() {
        return Err(AppError::InvalidArgument(format!("{} is not a file", canonical.display())));
    }
    if !has_audio_extension(&canonical) {
        return Err(AppError::Unsupported(canonical.display().to_string()));
    }
    Ok(canonical)
}

/// Upper bound on files queued from one open, so opening a very large tree
/// (or `/` by mistake) cannot stall the app. Whole libraries are the job of
/// the library scanner, not the queue.
pub const MAX_OPEN_FILES: usize = 10_000;

/// Expands a mixed list of files and directories into audio files, at most
/// [`MAX_OPEN_FILES`].
///
/// Directories are walked recursively and their files sorted by path so an
/// album dropped as a folder plays in track order. Entries that are not
/// audio, or cannot be read, are skipped rather than failing the whole batch.
pub fn collect_audio_files(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for input in inputs {
        if out.len() >= MAX_OPEN_FILES {
            break;
        }
        if input.is_dir() {
            let mut found = Vec::new();
            walk_dir(input, &mut found, 0, MAX_OPEN_FILES - out.len());
            found.sort();
            out.extend(found);
        } else if let Ok(file) = validate_audio_file(input) {
            out.push(file);
        }
    }
    out
}

/// Guards against symlink loops without tracking visited inodes.
const MAX_DEPTH: usize = 32;

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>, depth: usize, limit: usize) {
    if depth > MAX_DEPTH || out.len() >= limit {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        log::warn!("skipping unreadable directory {}", dir.display());
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else { continue };
        if out.len() >= limit {
            return;
        }
        if file_type.is_dir() {
            walk_dir(&path, out, depth + 1, limit);
        } else if let Ok(file) = validate_audio_file(&path) {
            out.push(file);
        }
    }
}

/// Converts an absolute path to a `file://` URI for GStreamer.
pub fn path_to_uri(path: &Path) -> AppResult<String> {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .map_err(|_| AppError::InvalidArgument(format!("{} is not an absolute path", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_audio_extensions_case_insensitively() {
        assert!(has_audio_extension(Path::new("/a/b/Track.FLAC")));
        assert!(has_audio_extension(Path::new("song.opus")));
        assert!(!has_audio_extension(Path::new("cover.jpg")));
        assert!(!has_audio_extension(Path::new("README")));
    }

    #[test]
    fn rejects_missing_and_non_audio_files() {
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, "x").unwrap();
        assert!(matches!(validate_audio_file(&txt), Err(AppError::Unsupported(_))));
        assert!(matches!(validate_audio_file(&dir.path().join("missing.flac")), Err(AppError::NotFound(_))));
    }

    #[test]
    fn collects_directories_recursively_in_path_order() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Disc 2");
        std::fs::create_dir(&sub).unwrap();
        for name in ["02 b.flac", "01 a.flac", "cover.jpg"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        std::fs::write(sub.join("01 c.mp3"), "x").unwrap();

        let files = collect_audio_files(&[dir.path().to_path_buf()]);
        let names: Vec<_> = files
            .iter()
            .map(|p| p.strip_prefix(dir.path().canonicalize().unwrap()).unwrap().display().to_string())
            .collect();
        assert_eq!(names, ["01 a.flac", "02 b.flac", "Disc 2/01 c.mp3"]);
    }

    #[test]
    fn builds_percent_encoded_file_uris() {
        let uri = path_to_uri(Path::new("/music/Sigur Rós/01 #1.flac")).unwrap();
        assert_eq!(uri, "file:///music/Sigur%20R%C3%B3s/01%20%231.flac");
    }
}
