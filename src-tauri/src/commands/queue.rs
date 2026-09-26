//! Queue commands, including opening files through the native dialog.
//!
//! The frontend never passes file paths: files reach the player through the
//! native dialog opened here, drag and drop, or the command line, all of which
//! are handled in Rust.

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::error::{AppError, AppResult};
use crate::filesystem::AUDIO_EXTENSIONS;
use crate::player::{EntryId, InsertMode, PlayerCommand};
use crate::state::AppState;

#[tauri::command]
pub async fn queue_play_entry(state: State<'_, AppState>, id: EntryId) -> AppResult<()> {
    state.player.command(PlayerCommand::PlayEntry(id)).await
}

#[tauri::command]
pub async fn queue_remove_entry(state: State<'_, AppState>, id: EntryId) -> AppResult<()> {
    state.player.command(PlayerCommand::RemoveEntry(id)).await
}

#[tauri::command]
pub async fn queue_clear(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::ClearQueue).await
}

/// Keeps only the playing track.
#[tauri::command]
pub async fn queue_clear_others(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::ClearOthers).await
}

/// Moves entries before `before`, or to the end of the queue.
#[tauri::command]
pub async fn queue_move(state: State<'_, AppState>, ids: Vec<EntryId>, before: Option<EntryId>) -> AppResult<()> {
    state.player.command(PlayerCommand::MoveEntries { ids, before }).await
}

/// Asks the user for audio files and queues them. Returns the number of
/// tracks queued, or 0 if the dialog was cancelled.
#[tauri::command]
pub async fn open_audio_files(app: AppHandle, state: State<'_, AppState>, mode: InsertMode) -> AppResult<usize> {
    let (reply, picked) = oneshot::channel();
    app.dialog().file().set_title("Open audio files").add_filter("Audio", AUDIO_EXTENSIONS).pick_files(move |files| {
        let _ = reply.send(files);
    });
    let Some(files) = picked.await.map_err(|_| AppError::PlayerUnavailable)? else {
        return Ok(0);
    };
    let paths = files.into_iter().filter_map(|f| f.into_path().ok()).collect();
    state.open_paths(paths, mode).await
}

/// Asks the user for a folder and queues every audio file inside it.
#[tauri::command]
pub async fn open_audio_folder(app: AppHandle, state: State<'_, AppState>, mode: InsertMode) -> AppResult<usize> {
    let (reply, picked) = oneshot::channel();
    app.dialog().file().set_title("Open a music folder").pick_folder(move |folder| {
        let _ = reply.send(folder);
    });
    let Some(folder) = picked.await.map_err(|_| AppError::PlayerUnavailable)? else {
        return Ok(0);
    };
    let path = folder.into_path().map_err(|_| AppError::InvalidArgument("not a local folder".into()))?;
    state.open_paths(vec![path], mode).await
}
