//! Application information, interface preferences, and the logs.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::logging;
use crate::preferences::{self, UiPreferences, UiPreferencesPatch};
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: String,
    version: String,
    gstreamer_version: String,
    /// Where the log files are; `None` if they cannot be written.
    log_dir: Option<String>,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle) -> AppInfo {
    let info = app.package_info();
    AppInfo {
        name: info.name.clone(),
        version: info.version.to_string(),
        gstreamer_version: gstreamer::version_string().to_string(),
        log_dir: logging::dir().map(|d| d.display().to_string()),
    }
}

#[tauri::command]
pub async fn get_ui_preferences(state: State<'_, AppState>) -> AppResult<UiPreferences> {
    preferences::load(&state.db).await
}

#[tauri::command]
pub async fn update_ui_preferences(state: State<'_, AppState>, patch: UiPreferencesPatch) -> AppResult<UiPreferences> {
    let retention = patch.log_retention_days.is_some();
    let prefs = preferences::update(&state.db, patch).await?;
    if retention {
        logging::set_retention_days(prefs.log_retention_days);
    }
    Ok(prefs)
}

/// Opens the logs folder in the desktop's file manager.
#[tauri::command]
pub fn open_log_folder() -> AppResult<()> {
    let dir = logging::dir().ok_or_else(|| AppError::NotFound("the logs folder".into()))?;
    std::process::Command::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map_err(|e| AppError::Io(std::io::Error::other(format!("could not open the file manager: {e}"))))?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiLogLevel {
    Error,
    Warn,
    Info,
}

/// Longest message (and, separately, detail such as a stack trace) logged
/// for the interface, in characters.
const UI_MESSAGE_LIMIT: usize = 2_000;
const UI_DETAIL_LIMIT: usize = 8_000;

/// Logs what the interface reports: its errors, and the errors it shows.
#[tauri::command]
pub fn log_from_ui(level: UiLogLevel, message: String, detail: Option<String>) {
    let level = match level {
        UiLogLevel::Error => log::Level::Error,
        UiLogLevel::Warn => log::Level::Warn,
        UiLogLevel::Info => log::Level::Info,
    };
    let message = clip(&message, UI_MESSAGE_LIMIT);
    match detail.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(detail) => log::log!(target: "ui", level, "{message}\n{}", clip(detail, UI_DETAIL_LIMIT)),
        None => log::log!(target: "ui", level, "{message}"),
    }
}

fn clip(text: &str, chars: usize) -> std::borrow::Cow<'_, str> {
    match text.char_indices().nth(chars) {
        Some((end, _)) => format!("{}… ({} more characters)", &text[..end], text[end..].chars().count()).into(),
        None => text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_long_interface_messages() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("ééééé", 3), "ééé… (2 more characters)");
    }
}
