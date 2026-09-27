//! Queue commands, including opening files through the native dialog.
//!
//! The frontend never passes file paths: files reach the player through the
//! native dialog opened here, drag and drop, or the command line, all of which
//! are handled in Rust.

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::database::settings;
use crate::error::{AppError, AppResult};
use crate::filesystem::AUDIO_EXTENSIONS;
use crate::player::{EntryId, InsertMode, PlayerCommand, SESSION_KEY, SavedSession};
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

/// Puts back the queue from before the last replace or clear.
#[tauri::command]
pub async fn queue_undo(state: State<'_, AppState>) -> AppResult<()> {
    state.player.command(PlayerCommand::UndoQueue).await
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    tracks: usize,
}

/// What a restore would bring back, for the empty queue's button.
#[tauri::command]
pub async fn queue_saved_session(state: State<'_, AppState>) -> AppResult<Option<SessionSummary>> {
    let saved: Option<SavedSession> = settings::get(&state.db, SESSION_KEY).await?;
    Ok(saved.filter(|s| !s.entries.is_empty()).map(|s| SessionSummary { tracks: s.entries.len() }))
}

/// Restores the queue saved when the app last closed, paused where it was.
/// Library tracks are found again even if their files moved; other files
/// are read from disk, and anything that no longer exists is left out.
#[tauri::command]
pub async fn queue_restore_session(state: State<'_, AppState>) -> AppResult<usize> {
    let saved: SavedSession =
        settings::get(&state.db, SESSION_KEY).await?.ok_or_else(|| AppError::NotFound("saved session".into()))?;
    let ids: Vec<i64> = saved.entries.iter().filter_map(|e| e.track_id).collect();
    let known: std::collections::HashMap<i64, _> = crate::library::query::track_infos(state.library.db(), &ids)
        .await?
        .into_iter()
        .filter_map(|t| t.library.map(|l| (l.track_id, t.clone())))
        .collect();
    let artwork = state.artwork.clone();
    let entries = saved.entries.clone();
    let (tracks, start) = tokio::task::spawn_blocking(move || {
        let mut tracks = Vec::new();
        let mut start = 0;
        for (i, entry) in entries.iter().enumerate() {
            let track = match entry.track_id.and_then(|id| known.get(&id)) {
                Some(track) => Some(track.clone()),
                None => {
                    let path = std::path::PathBuf::from(&entry.path);
                    path.is_file().then(|| std::sync::Arc::new(crate::player::TrackInfo::load(&path, &artwork)))
                }
            };
            if let Some(track) = track {
                if Some(i) == saved.current {
                    start = tracks.len();
                }
                tracks.push(track);
            }
        }
        (tracks, start)
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::other(e)))?;
    if tracks.is_empty() {
        return Err(AppError::NotFound("none of the saved tracks exist any more".into()));
    }
    let count = tracks.len();
    let position = std::time::Duration::from_millis(saved.position_ms);
    state.player.command(PlayerCommand::Restore { tracks, start, position }).await?;
    Ok(count)
}
