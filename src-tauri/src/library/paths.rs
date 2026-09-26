//! Paths as stored in the database: raw bytes, because Linux file names are
//! arbitrary byte strings and need not be valid UTF-8.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

pub fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_non_utf8_names() {
        let path = path_from_bytes(b"/music/caf\xe9.flac");
        assert_eq!(path_bytes(&path), b"/music/caf\xe9.flac");
    }
}
