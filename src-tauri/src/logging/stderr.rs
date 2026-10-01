//! Everything written to standard error, copied into the log.
//!
//! The libraries underneath (GStreamer and its plugin scanner, GLib, GTK,
//! WebKit and its web process, Mesa) and child processes print their
//! warnings straight to file descriptor 2, never through the `log` crate.
//! That descriptor is swapped for a pipe, read on a thread of its own: each
//! line still goes to the terminal, and into the log as `stderr`.
//!
//! The app's own records do not take this path (see [`super::Tee`]): they
//! are written synchronously, so the last ones before a crash are kept.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use super::files::{self, LogFiles};

unsafe extern "C" {
    // From the C library, which std already links.
    fn dup2(old: i32, new: i32) -> i32;
}

/// Redirects standard error into the log. Returns the original standard
/// error (the terminal, usually), for the app's own records.
pub fn capture(files: Arc<Mutex<LogFiles>>) -> io::Result<File> {
    let terminal = File::from(io::stderr().as_fd().try_clone_to_owned()?);
    let mut echo = terminal.try_clone()?;
    let (reader, writer) = io::pipe()?;
    // SAFETY: both are open descriptors; fd 2 is atomically replaced (and,
    // unlike the pipe's own descriptors, inherited by child processes).
    if unsafe { dup2(writer.as_raw_fd(), 2) } < 0 {
        return Err(io::Error::last_os_error());
    }
    drop(writer);
    std::thread::Builder::new().name("stderr-log".into()).spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let _ = echo.write_all(&line);
            let text = String::from_utf8_lossy(&line);
            let text = text.trim_end();
            // GLib starts each message with an empty line.
            if text.trim().is_empty() {
                continue;
            }
            let now = SystemTime::now();
            let record = format!("{}       stderr: {text}\n", files::timestamp(now));
            super::lock(&files).write(record.as_bytes(), now);
        }
    })?;
    Ok(terminal)
}
