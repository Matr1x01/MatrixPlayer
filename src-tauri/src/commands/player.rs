//! Transport and volume commands.

use std::time::Duration;

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::metadata::lyrics::{self, Lyrics};
use crate::player::{PlayerCommand, PlayerSnapshot, RepeatMode};
use crate::state::AppState;

#[tauri::command]
pub async fn get_player_snapshot(state: State<'_, AppState>) -> AppResult<PlayerSnapshot> {
    state.player.snapshot().await
}

#[tauri::command]
pub async fn player_play(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::Play).await
}

#[tauri::command]
pub async fn player_pause(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::Pause).await
}

#[tauri::command]
pub async fn player_toggle(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::TogglePlayPause).await
}

#[tauri::command]
pub async fn player_stop(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::Stop).await
}

#[tauri::command]
pub async fn player_next(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::Next).await
}

#[tauri::command]
pub async fn player_previous(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::Previous).await
}

#[tauri::command]
pub async fn player_seek(state: State<'_, AppState>, position_ms: f64) -> AppResult<()> {
    if !position_ms.is_finite() || position_ms < 0.0 {
        return Err(AppError::InvalidArgument("position must be a non-negative number".into()));
    }
    let position = Duration::from_secs_f64(position_ms / 1000.0);
    state.player.command(PlayerCommand::Seek(position)).await
}

#[tauri::command]
pub async fn player_set_volume(state: State<'_, AppState>, level: f64) -> AppResult<()> {
    if !level.is_finite() {
        return Err(AppError::InvalidArgument("volume must be a number".into()));
    }
    state.player.command(PlayerCommand::SetVolume(level)).await
}

#[tauri::command]
pub async fn player_set_muted(state: State<'_, AppState>, muted: bool) -> AppResult<()> {
    state.player.command(PlayerCommand::SetMuted(muted)).await
}

#[tauri::command]
pub async fn player_set_repeat(state: State<'_, AppState>, mode: RepeatMode) -> AppResult<()> {
    state.player.command(PlayerCommand::SetRepeat(mode)).await
}

#[tauri::command]
pub async fn player_set_shuffle(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.player.command(PlayerCommand::SetShuffle(enabled)).await
}

/// Lyrics of the current track: its `.lrc` file, or lyrics in its tags.
#[tauri::command]
pub async fn player_lyrics(state: State<'_, AppState>) -> AppResult<Option<Lyrics>> {
    let Some(track) = state.player.snapshot().await?.track else { return Ok(None) };
    tauri::async_runtime::spawn_blocking(move || lyrics::load(&track.path))
        .await
        .map_err(|e| AppError::Metadata(e.to_string()))
}

/// Peak levels (0–255) of the current track in `points` columns, for the
/// waveform seek bar. The first request for a track decodes it.
#[tauri::command]
pub async fn player_waveform(state: State<'_, AppState>, points: usize) -> AppResult<Option<Vec<u8>>> {
    let Some(track) = state.player.snapshot().await?.track else { return Ok(None) };
    let cache = state.waveforms.clone();
    tauri::async_runtime::spawn_blocking(move || cache.get(&track.path, points))
        .await
        .map_err(|e| AppError::Audio(e.to_string()))?
        .map(Some)
}
