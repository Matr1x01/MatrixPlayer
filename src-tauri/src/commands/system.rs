//! Application information and interface preferences.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::preferences::{self, UiPreferences, UiPreferencesPatch};
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: String,
    version: String,
    gstreamer_version: String,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle) -> AppInfo {
    let info = app.package_info();
    AppInfo {
        name: info.name.clone(),
        version: info.version.to_string(),
        gstreamer_version: gstreamer::version_string().to_string(),
    }
}

#[tauri::command]
pub async fn get_ui_preferences(state: State<'_, AppState>) -> AppResult<UiPreferences> {
    preferences::load(&state.db).await
}

#[tauri::command]
pub async fn update_ui_preferences(state: State<'_, AppState>, patch: UiPreferencesPatch) -> AppResult<UiPreferences> {
    preferences::update(&state.db, patch).await
}
