//! Persistent logs, for finding out afterwards why MatrixPlayer failed to
//! start or to play.
//!
//! The files are in `~/.local/share/io.github.matr1x01.MatrixPlayer/logs/`
//! (where Tauri's `app_log_dir` points); `latest.log` is the newest. They
//! hold:
//!
//! - the app's own log records (`RUST_LOG` sets the level; `info` by default),
//! - panics, with where they happened and a backtrace,
//! - whatever the libraries underneath and child processes print to
//!   standard error (see [`stderr`]),
//! - GStreamer's own warnings and errors (see `audio::diagnostics`),
//! - errors in the interface, which the webview sends over,
//! - what the system looked like at startup (see [`system`]), and whether
//!   the run before ended normally.
//!
//! Everything still goes to the terminal too. Files rotate and expire as
//! [`files`] describes; how long they are kept is a preference.

pub mod files;
mod stderr;
pub mod system;

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::SystemTime;

use files::LogFiles;

/// The app's identifier (`tauri.conf.json`), which names its data folder.
pub const APP_ID: &str = "io.github.matr1x01.MatrixPlayer";
pub const DEFAULT_RETENTION_DAYS: u32 = 7;
/// `tracing` logs its spans' lifecycles (zbus has many) as `info`.
const DEFAULT_FILTER: &str = "info,zbus=warn,tracing=warn";

/// Logged last on a normal exit: a log that does not have it was cut short.
pub const EXITED: &str = "MatrixPlayer exited normally";
/// Logged by a launch that hands its arguments to a running instance.
pub const HANDED_OVER: &str = "this launch hands over to it and exits";

static FILES: OnceLock<Arc<Mutex<LogFiles>>> = OnceLock::new();

/// `$XDG_DATA_HOME/<app id>/logs`, as Tauri's `app_log_dir` resolves it.
fn default_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))?;
    Some(data.join(APP_ID).join("logs"))
}

/// Starts logging, to the terminal and into the log files. Call first
/// thing, before anything that can fail.
pub fn init() {
    let mut problems = Vec::new();
    let files = default_dir()
        .ok_or_else(|| io::Error::other("there is no home folder"))
        .and_then(|dir| LogFiles::open(&dir, DEFAULT_RETENTION_DAYS, files::LIMITS, SystemTime::now()))
        .map(|files| Arc::new(Mutex::new(files)))
        .map_err(|e| problems.push(format!("log files cannot be written: {e}")))
        .ok();
    let terminal: Box<dyn Write + Send> = match files.as_ref().map(|files| stderr::capture(files.clone())) {
        Some(Ok(terminal)) => Box::new(terminal),
        Some(Err(e)) => {
            problems.push(format!("standard error cannot be copied into the log: {e}"));
            Box::new(io::stderr())
        }
        None => Box::new(io::stderr()),
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(DEFAULT_FILTER))
        .format(format)
        .target(env_logger::Target::Pipe(Box::new(Tee { terminal, files: files.clone() })))
        .init();
    if let Some(files) = files {
        // Not inside the log call: writing the record takes the same lock.
        let path = lock(&files).path().to_owned();
        log::info!("logging to {}", path.display());
        let _ = FILES.set(files);
    }
    for problem in problems {
        log::warn!("{problem}");
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if log::log_enabled!(target: "panic", log::Level::Error) {
            log_panic(info);
        } else {
            default_hook(info);
        }
    }));
}

/// Writes each record to the terminal and the log file as it is made, so
/// the last ones before a crash are not lost.
struct Tee {
    terminal: Box<dyn Write + Send>,
    files: Option<Arc<Mutex<LogFiles>>>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = self.terminal.write_all(buf);
        if let Some(files) = &self.files {
            lock(files).write(buf, SystemTime::now());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.terminal.flush()
    }
}

/// `2026-10-01T09:30:12.345Z INFO  audio::output: message`, with the
/// lines after the first of a message indented.
fn format(buf: &mut env_logger::fmt::Formatter, record: &log::Record) -> io::Result<()> {
    let target = record.target();
    let target = target.strip_prefix("matrixplayer_lib::").unwrap_or(target);
    let message = record.args().to_string().replace('\n', "\n    ");
    writeln!(buf, "{} {:<5} {target}: {message}", files::timestamp(SystemTime::now()), record.level())
}

fn log_panic(info: &std::panic::PanicHookInfo<'_>) {
    let thread = std::thread::current();
    let message = info.payload_as_str().unwrap_or("(no message)");
    let place = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
    let backtrace = short_backtrace(&std::backtrace::Backtrace::force_capture().to_string());
    log::error!(
        target: "panic",
        "thread '{}' panicked{place}: {message}\n{backtrace}",
        thread.name().unwrap_or("<unnamed>")
    );
}

/// The frames that matter, as Rust's own short backtraces pick them: from
/// below the panic machinery to the start of the thread. The full one if
/// those markers are missing (a binary without symbols).
fn short_backtrace(full: &str) -> String {
    // A frame is a line `  12: function`, followed by its `at file:line`.
    let mut frames: Vec<Vec<&str>> = Vec::new();
    for line in full.lines() {
        let numbered = line
            .trim_start()
            .split_once(':')
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        match frames.last_mut() {
            Some(frame) if !numbered => frame.push(line),
            _ => frames.push(vec![line]),
        }
    }
    fn name<'a>(frame: &[&'a str]) -> &'a str {
        frame[0]
    }
    let Some(end_of_panic) = frames.iter().position(|f| name(f).contains("__rust_end_short_backtrace")) else {
        return full.to_owned();
    };
    let machinery = ["rust_begin_unwind", "core::panicking::"];
    let start = (end_of_panic + 1..frames.len())
        .find(|&i| !machinery.iter().any(|m| name(&frames[i]).contains(m)))
        .unwrap_or(frames.len());
    let end = (start..frames.len())
        .find(|&i| name(&frames[i]).contains("__rust_begin_short_backtrace"))
        .unwrap_or(frames.len());
    frames[start..end].iter().map(|frame| frame.join("\n")).collect::<Vec<_>>().join("\n")
}

fn lock(files: &Mutex<LogFiles>) -> MutexGuard<'_, LogFiles> {
    files.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The folder the logs are in, if they are being written.
pub fn dir() -> Option<PathBuf> {
    FILES.get().map(|files| lock(files).dir().to_owned())
}

/// Keeps log files for `days` from now on, deleting older ones.
pub fn set_retention_days(days: u32) {
    if let Some(files) = FILES.get() {
        lock(files).set_retention_days(days, SystemTime::now());
    }
}

/// Warns if the run before this one did not log its exit: it crashed, hung
/// and was killed, or the computer went down with it. Launches that only
/// handed over to a running instance are not counted.
pub fn check_previous_run() {
    let Some(files) = FILES.get() else { return };
    let earlier = lock(files).earlier().to_vec();
    if let Some((path, ending)) = unfinished_run(&earlier) {
        log::warn!(
            "the previous run did not exit normally (it crashed, was killed, or the system went down); \
             its log is {}, ending with: {ending}",
            path.display()
        );
    }
}

/// The newest of `logs` that is a run of its own, and its last line, if it
/// did not end normally.
fn unfinished_run(logs: &[PathBuf]) -> Option<(&PathBuf, String)> {
    for path in logs.iter().take(10) {
        let Some(tail) = files::tail(path, 8192) else { continue };
        if tail.contains(HANDED_OVER) {
            continue;
        }
        if tail.contains(EXITED) {
            return None;
        }
        let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("(nothing)");
        return Some((path, last.trim().to_owned()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_frames_between_the_panic_and_the_thread_start() {
        let full = "   0: matrixplayer_lib::logging::log_panic
             at ./src/logging/mod.rs:128:21
   1: std::panicking::panic_with_hook
   2: std::sys::backtrace::__rust_end_short_backtrace::<std::panicking::panic_handler::{closure#0}, !>
   3: __rustc::rust_begin_unwind
   4: core::panicking::panic_fmt
             at /rustc/library/core/src/panicking.rs:80:14
   5: core::result::unwrap_failed
   6: matrixplayer_lib::run
             at ./src/lib.rs:182:10
   7: std::panicking::catch_unwind::do_call
   8: std::sys::backtrace::__rust_begin_short_backtrace::<fn(), ()>
   9: main";
        assert_eq!(
            short_backtrace(full),
            "   5: core::result::unwrap_failed
   6: matrixplayer_lib::run
             at ./src/lib.rs:182:10
   7: std::panicking::catch_unwind::do_call"
        );
        let stripped = "   0: <unknown>\n   1: <unknown>";
        assert_eq!(short_backtrace(stripped), stripped);
    }

    #[test]
    fn finds_a_run_that_did_not_exit_normally() {
        let dir = tempfile::tempdir().unwrap();
        let log = |name: &str, text: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            path
        };
        let clean = log("clean.log", &format!("starting\n{EXITED}\nstderr: a late warning\n"));
        let crashed = log("crashed.log", "starting\nplaying\n  \n");
        let handover = log("handover.log", &format!("MatrixPlayer is already running; {HANDED_OVER}\n"));

        assert_eq!(unfinished_run(&[clean.clone(), crashed.clone()]), None);
        assert_eq!(
            unfinished_run(&[handover.clone(), crashed.clone(), clean.clone()]),
            Some((&crashed, "playing".into()))
        );
        assert_eq!(unfinished_run(&[handover, clean]), None);
        assert_eq!(unfinished_run(&[]), None);
    }
}
