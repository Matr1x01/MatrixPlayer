//! Equalizer, processing and output device commands.

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::audio::output::{self, OutputBackend, OutputDevice};
use crate::database::settings;
use crate::dsp::service::DspState;
use crate::dsp::settings::DspSettings;
use crate::error::{AppError, AppResult};
use crate::player::{OUTPUT_DEVICE_KEY, PlayerCommand};
use crate::state::AppState;

#[tauri::command]
pub fn dsp_state(state: State<'_, AppState>) -> DspState {
    state.dsp.state()
}

/// Applies new settings right away. Returns them as applied (clamped).
#[tauri::command]
pub async fn dsp_update(state: State<'_, AppState>, settings: DspSettings) -> AppResult<DspSettings> {
    let before = state.dsp.control().settings();
    let applied = state.dsp.update(settings);
    apply_output_settings(&state, &before, &applied).await?;
    Ok(applied)
}

/// Resets playback processing to its defaults; the equalizer is kept.
#[tauri::command]
pub async fn dsp_restore_defaults(state: State<'_, AppState>) -> AppResult<DspSettings> {
    let before = state.dsp.control().settings();
    let applied = state.dsp.restore_defaults();
    apply_output_settings(&state, &before, &applied).await?;
    Ok(applied)
}

/// Dither is set up when the output's format is negotiated, so a change is
/// only heard once the track is reopened (which the player does only for
/// outputs that take integer samples).
async fn apply_output_settings(state: &AppState, before: &DspSettings, after: &DspSettings) -> AppResult<()> {
    if before.dither == after.dither {
        return Ok(());
    }
    state.player.command(PlayerCommand::RenegotiateOutput).await
}

#[tauri::command]
pub async fn dsp_save_preset(state: State<'_, AppState>, name: String) -> AppResult<DspState> {
    state.dsp.save_preset(&name).await
}

#[tauri::command]
pub async fn dsp_delete_preset(state: State<'_, AppState>, name: String) -> AppResult<DspState> {
    state.dsp.delete_preset(&name).await
}

/// Saves the user's presets to a file the user picks. Returns how many were
/// written, or `None` if the dialog was cancelled.
#[tauri::command]
pub async fn dsp_export_presets(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<usize>> {
    let (reply, picked) = oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export equalizer presets")
        .set_file_name("MatrixPlayer presets.json")
        .add_filter("Presets", &["json"])
        .save_file(move |file| {
            let _ = reply.send(file);
        });
    let Some(file) = picked.await.map_err(|_| AppError::PlayerUnavailable)? else { return Ok(None) };
    let path = file.into_path().map_err(|_| AppError::InvalidArgument("not a local file".into()))?;
    state.dsp.export_presets(&path).await.map(Some)
}

/// Adds presets from a file the user picks. Returns how many were added,
/// or `None` if the dialog was cancelled.
#[tauri::command]
pub async fn dsp_import_presets(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<usize>> {
    let (reply, picked) = oneshot::channel();
    app.dialog().file().set_title("Import equalizer presets").add_filter("Presets", &["json"]).pick_file(move |file| {
        let _ = reply.send(file);
    });
    let Some(file) = picked.await.map_err(|_| AppError::PlayerUnavailable)? else { return Ok(None) };
    let path = file.into_path().map_err(|_| AppError::InvalidArgument("not a local file".into()))?;
    state.dsp.import_presets(&path).await.map(Some)
}

/// Starts or stops the `audio-analysis` event stream (visualizer, meters).
#[tauri::command]
pub fn dsp_set_analysis(state: State<'_, AppState>, enabled: bool) {
    state.dsp.control().set_analysis(enabled);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDevices {
    pub backend: OutputBackend,
    pub devices: Vec<OutputDevice>,
    /// The chosen device id; `None` follows the system default.
    pub selected: Option<String>,
}

#[tauri::command]
pub async fn audio_output_devices(state: State<'_, AppState>) -> AppResult<OutputDevices> {
    let backend = state.player.snapshot().await?.signal.backend;
    let devices = tauri::async_runtime::spawn_blocking(move || output::devices(backend))
        .await
        .map_err(|e| AppError::Audio(e.to_string()))?;
    let selected = settings::get::<Option<String>>(&state.db, OUTPUT_DEVICE_KEY).await?.flatten();
    Ok(OutputDevices { backend, devices, selected })
}

/// Plays through `id`, or through the system default with `None`.
#[tauri::command]
pub async fn player_set_output_device(state: State<'_, AppState>, id: Option<String>) -> AppResult<()> {
    let wanted = id.clone();
    let out = tauri::async_runtime::spawn_blocking(move || output::create(wanted.as_deref()))
        .await
        .map_err(|e| AppError::Audio(e.to_string()))??;
    if id.is_some() && out.device.is_none() {
        return Err(AppError::Audio("That output device is no longer available".into()));
    }
    state.player.command(PlayerCommand::SetOutput(Box::new(out))).await?;
    settings::set(&state.db, OUTPUT_DEVICE_KEY, &id).await
}

#[tauri::command]
pub async fn player_set_gapless(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.player.command(PlayerCommand::SetGapless(enabled)).await
}
