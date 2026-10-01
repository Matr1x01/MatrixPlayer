//! Linux desktop integration: MPRIS (media controls and media keys),
//! "now playing" notifications, and the command-line actions the desktop
//! entry offers (`--play-pause`, `--next`, …), which a running instance
//! receives through the single-instance plugin.

pub mod mpris;
pub mod notifications;

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::player::PlayerCommand;

/// The D-Bus name the single-instance plugin holds for the running app.
const SINGLE_INSTANCE: &str = "io.github.matr1x01.MatrixPlayer.SingleInstance";

/// The process id of a MatrixPlayer already running in this session. A new
/// launch hands its arguments to that one and exits, even when it does not
/// answer, so a stuck instance looks like a launch that does nothing.
pub fn running_instance() -> Option<u32> {
    tauri::async_runtime::block_on(async {
        let bus = zbus::Connection::session().await.ok()?;
        let dbus = zbus::fdo::DBusProxy::new(&bus).await.ok()?;
        let name = zbus::names::BusName::try_from(SINGLE_INSTANCE).ok()?;
        dbus.get_connection_unix_process_id(name).await.ok()
    })
}

/// Brings the main window to the front.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// What a command line asks for.
#[derive(Debug)]
pub enum Invocation {
    /// A transport action from the desktop entry or a script.
    Command(PlayerCommand),
    /// Files and folders to play.
    Open(Vec<PathBuf>),
    /// Nothing specific: just show the window.
    Show,
}

/// Reads an argument list (without the program name).
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Invocation {
    let mut files = Vec::new();
    for arg in args {
        let command = match arg.as_str() {
            "--play-pause" => Some(PlayerCommand::TogglePlayPause),
            "--play" => Some(PlayerCommand::Play),
            "--pause" => Some(PlayerCommand::Pause),
            "--stop" => Some(PlayerCommand::Stop),
            "--next" => Some(PlayerCommand::Next),
            "--previous" => Some(PlayerCommand::Previous),
            _ => None,
        };
        if let Some(command) = command {
            return Invocation::Command(command);
        }
        if arg.starts_with("--") {
            continue;
        }
        // Desktop environments may pass file:// URIs (%U).
        let path = match url::Url::parse(&arg) {
            Ok(u) if u.scheme() == "file" => u.to_file_path().ok(),
            _ => Some(PathBuf::from(&arg)),
        };
        files.extend(path.filter(|p| p.exists()));
    }
    if files.is_empty() { Invocation::Show } else { Invocation::Open(files) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_actions_files_and_uris() {
        let args = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(matches!(parse_args(args(&["--next"])), Invocation::Command(PlayerCommand::Next)));
        assert!(matches!(parse_args(args(&["--play-pause"])), Invocation::Command(PlayerCommand::TogglePlayPause)));
        assert!(matches!(parse_args(args(&[])), Invocation::Show));
        assert!(matches!(parse_args(args(&["/definitely/not/here.flac"])), Invocation::Show));

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a b.flac");
        std::fs::write(&file, b"").unwrap();
        let uri = url::Url::from_file_path(&file).unwrap().to_string();
        assert!(matches!(parse_args(args(&["--some-gtk-flag", &uri])), Invocation::Open(f) if f == [file.clone()]));
        assert!(matches!(parse_args(args(&[file.to_str().unwrap()])), Invocation::Open(f) if f == [file]));
    }
}
