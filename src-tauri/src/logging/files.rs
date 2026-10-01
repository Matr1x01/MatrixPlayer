//! The log files: one per launch, a new one each day (UTC) and whenever one
//! grows too large; none kept past the retention period, and the oldest
//! deleted first once the folder holds too much.

use std::cmp::Reverse;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PREFIX: &str = "matrixplayer-";
const SUFFIX: &str = ".log";
/// A link to the file being written.
pub const LATEST: &str = "latest.log";
const DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Past this, logging moves on to a new file.
    pub file_bytes: u64,
    /// The most the folder keeps; the oldest files go first beyond it.
    pub total_bytes: u64,
}

pub const LIMITS: Limits = Limits { file_bytes: 8 << 20, total_bytes: 64 << 20 };

pub struct LogFiles {
    dir: PathBuf,
    file: File,
    path: PathBuf,
    written: u64,
    day: u64,
    retention: Duration,
    limits: Limits,
    /// The files there were before this launch, most recently written first.
    earlier: Vec<PathBuf>,
}

impl LogFiles {
    /// Starts a new file in `dir` (creating it), and deletes what is past
    /// `retention_days`.
    pub fn open(dir: &Path, retention_days: u32, limits: Limits, now: SystemTime) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let earlier = logs(dir).into_iter().map(|log| log.path).collect();
        let (file, path) = create(dir, now)?;
        let files = Self {
            dir: dir.to_owned(),
            file,
            path,
            written: 0,
            day: day(now),
            retention: retention(retention_days),
            limits,
            earlier,
        };
        files.prune(now);
        Ok(files)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn earlier(&self) -> &[PathBuf] {
        &self.earlier
    }

    /// Appends `bytes` (whole lines), moving on to a new file first on a
    /// new day or when this one is full. Failures are dropped: there is
    /// nowhere left to report them.
    pub fn write(&mut self, bytes: &[u8], now: SystemTime) {
        let full = self.written > 0 && self.written + bytes.len() as u64 > self.limits.file_bytes;
        if (full || day(now) != self.day) && !self.rotate(now) && full {
            return;
        }
        if self.file.write_all(bytes).is_ok() {
            self.written += bytes.len() as u64;
        }
    }

    /// Keeps files for `days` from now on, and deletes what is older.
    pub fn set_retention_days(&mut self, days: u32, now: SystemTime) {
        self.retention = retention(days);
        self.prune(now);
    }

    /// Moves on to a new file, noting the move in both. False if the new
    /// one could not be created.
    fn rotate(&mut self, now: SystemTime) -> bool {
        self.day = day(now);
        let Ok((file, path)) = create(&self.dir, now) else { return false };
        let stamp = timestamp(now);
        let _ = writeln!(self.file, "{stamp} INFO  logging: continued in {}", file_name(&path));
        let old = std::mem::replace(&mut self.path, path);
        self.file = file;
        let note = format!("{stamp} INFO  logging: continued from {}\n", file_name(&old));
        self.written = if self.file.write_all(note.as_bytes()).is_ok() { note.len() as u64 } else { 0 };
        self.prune(now);
        true
    }

    /// Deletes the files past the retention period, then the oldest ones
    /// beyond the size limit. The file being written always stays.
    fn prune(&self, now: SystemTime) {
        let mut kept = self.written;
        let mut full = false;
        for log in logs(&self.dir).into_iter().filter(|log| log.path != self.path) {
            full |= kept + log.len > self.limits.total_bytes;
            let expired = now.duration_since(log.modified).is_ok_and(|age| age > self.retention);
            if full || expired {
                let _ = fs::remove_file(&log.path);
            } else {
                kept += log.len;
            }
        }
    }
}

fn retention(days: u32) -> Duration {
    Duration::from_secs(u64::from(days.max(1)) * DAY)
}

struct Log {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
}

/// The log files in `dir`, most recently written first.
fn logs(dir: &Path) -> Vec<Log> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut logs: Vec<Log> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(SUFFIX)))
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(fs::Metadata::is_file)?;
            Some(Log { path: e.path(), modified: meta.modified().ok()?, len: meta.len() })
        })
        .collect();
    logs.sort_by_key(|log| Reverse(log.modified));
    logs
}

/// A new file named after `now`, and `latest.log` pointed at it.
fn create(dir: &Path, now: SystemTime) -> io::Result<(File, PathBuf)> {
    let stamp = file_stamp(now);
    for n in 1..100 {
        let name = if n == 1 { format!("{PREFIX}{stamp}{SUFFIX}") } else { format!("{PREFIX}{stamp}-{n}{SUFFIX}") };
        let path = dir.join(&name);
        match OpenOptions::new().append(true).create_new(true).open(&path) {
            Ok(file) => {
                let link = dir.join(LATEST);
                let _ = fs::remove_file(&link);
                let _ = std::os::unix::fs::symlink(&name, &link);
                return Ok((file, path));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many log files for one second"))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The last `bytes` of a file, as text.
pub fn tail(path: &Path, bytes: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(bytes))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    Some(String::from_utf8_lossy(&tail).into_owned())
}

// ── UTC time, without a date library ───────────────────────────────────────

struct Utc {
    year: i64,
    month: u32,
    day: u32,
    hour: u64,
    minute: u64,
    second: u64,
    millis: u32,
}

fn utc(t: SystemTime) -> Utc {
    let since = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (year, month, day) = civil_from_days((secs / DAY) as i64);
    let time = secs % DAY;
    Utc {
        year,
        month,
        day,
        hour: time / 3600,
        minute: time / 60 % 60,
        second: time % 60,
        millis: since.subsec_millis(),
    }
}

/// Days since 1970-01-01 as a proleptic Gregorian date (after Howard
/// Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn day(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / DAY
}

/// `2026-10-01T09:30:12.345Z`: what each line starts with.
pub fn timestamp(t: SystemTime) -> String {
    let u = utc(t);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", u.year, u.month, u.day, u.hour, u.minute, u.second, u.millis)
}

/// `2026-10-01_09-30-12`: what a file is named after.
fn file_stamp(t: SystemTime) -> String {
    let u = utc(t);
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", u.year, u.month, u.day, u.hour, u.minute, u.second)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    const NOW: u64 = 1_790_812_800 + 9 * 3600; // 2026-10-01 09:00 UTC

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> =
            fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        names
    }

    /// An earlier launch's file, last written `age_days` before [`NOW`].
    fn old_log(dir: &Path, name: &str, bytes: usize, age_days: u64) {
        let path = dir.join(format!("{PREFIX}{name}{SUFFIX}"));
        fs::write(&path, vec![b'x'; bytes]).unwrap();
        File::options().write(true).open(&path).unwrap().set_modified(at(NOW - age_days * DAY)).unwrap();
    }

    #[test]
    fn writes_utc_timestamps() {
        assert_eq!(timestamp(at(0)), "1970-01-01T00:00:00.000Z");
        assert_eq!(timestamp(at(951_782_400)), "2000-02-29T00:00:00.000Z");
        assert_eq!(timestamp(at(1_790_812_800) + Duration::from_millis(45)), "2026-10-01T00:00:00.045Z");
        assert_eq!(timestamp(at(4_102_444_799)), "2099-12-31T23:59:59.000Z");
        assert_eq!(file_stamp(at(NOW + 62)), "2026-10-01_09-01-02");
    }

    #[test]
    fn each_launch_gets_its_own_file_and_latest_points_at_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = LogFiles::open(dir.path(), 7, LIMITS, at(NOW)).unwrap();
        first.write(b"one\n", at(NOW));
        let second = LogFiles::open(dir.path(), 7, LIMITS, at(NOW)).unwrap();
        assert_eq!(
            names(dir.path()),
            ["latest.log", "matrixplayer-2026-10-01_09-00-00-2.log", "matrixplayer-2026-10-01_09-00-00.log"]
        );
        assert_eq!(second.earlier(), [first.path().to_owned()]);
        assert_eq!(fs::read_link(dir.path().join(LATEST)).unwrap(), second.path().file_name().unwrap());
    }

    #[test]
    fn moves_on_to_a_new_file_each_day_and_when_full() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits { file_bytes: 10, total_bytes: 1000 };
        let mut files = LogFiles::open(dir.path(), 7, limits, at(NOW)).unwrap();
        let first = files.path().to_owned();
        files.write(b"12345678\n", at(NOW));
        files.write(b"abcdefgh\n", at(NOW + 1));
        let second = files.path().to_owned();
        assert_ne!(first, second, "the first file was full");
        files.write(b"tomorrow\n", at(NOW + DAY));
        assert!(files.path().ends_with("matrixplayer-2026-10-02_09-00-00.log"));

        let first = fs::read_to_string(first).unwrap();
        assert!(first.starts_with("12345678\n") && first.contains("continued in matrixplayer-2026-10-01_09-00-01.log"));
        let second = fs::read_to_string(second).unwrap();
        assert!(second.contains("continued from matrixplayer-2026-10-01_09-00-00.log\nabcdefgh\n"), "{second}");
        assert!(fs::read_to_string(files.path()).unwrap().ends_with("tomorrow\n"));
    }

    #[test]
    fn deletes_files_past_the_retention_period() {
        let dir = tempfile::tempdir().unwrap();
        old_log(dir.path(), "a-week-ago", 10, 8);
        old_log(dir.path(), "two-days-ago", 10, 2);
        fs::write(dir.path().join("notes.txt"), "not a log").unwrap();
        let mut files = LogFiles::open(dir.path(), 7, LIMITS, at(NOW)).unwrap();
        let current = file_name(files.path());
        assert_eq!(names(dir.path()), ["latest.log", current.as_str(), "matrixplayer-two-days-ago.log", "notes.txt"]);

        files.set_retention_days(1, at(NOW));
        assert_eq!(names(dir.path()), ["latest.log", current.as_str(), "notes.txt"]);
    }

    #[test]
    fn keeps_the_newest_files_within_the_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        old_log(dir.path(), "oldest", 40, 3);
        old_log(dir.path(), "older", 40, 2);
        old_log(dir.path(), "newer", 40, 1);
        let limits = Limits { file_bytes: 1000, total_bytes: 100 };
        let files = LogFiles::open(dir.path(), 7, limits, at(NOW)).unwrap();
        let current = file_name(files.path());
        assert_eq!(
            names(dir.path()),
            ["latest.log", current.as_str(), "matrixplayer-newer.log", "matrixplayer-older.log"]
        );
        assert_eq!(files.earlier().len(), 3, "listed before pruning");
    }

    #[test]
    fn reads_the_end_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.log");
        fs::write(&path, "first line\nlast line\n").unwrap();
        assert_eq!(tail(&path, 10).as_deref(), Some("last line\n"));
        assert_eq!(tail(&path, 1000).as_deref(), Some("first line\nlast line\n"));
        assert_eq!(tail(&dir.path().join("missing.log"), 10), None);
    }
}
