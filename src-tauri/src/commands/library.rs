//! Library commands: folders, browsing, search, favorites, playlists.
//!
//! Library folders are chosen in the native dialog opened here; the UI only
//! ever refers to library items by id.

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::error::{AppError, AppResult};
use crate::library::playlists::HistoryEntry;
use crate::library::query::{
    self, AlbumDetail, AlbumRow, ArtistDetail, ArtistKind, ArtistRow, FolderDetail, FolderNode, GenreDetail, GenreRow,
    LibraryStats, PlaylistRow, RootInfo, SearchResults, TrackList, TrackQuery, TrackRow,
};
use crate::library::{PlaySource, ScanStatus};
use crate::player::{InsertMode, PlayerCommand};
use crate::state::AppState;

/// Upper bound on rows fetched at once, to keep responses small.
const MAX_ROWS: usize = 2000;

#[tauri::command]
pub async fn library_stats(state: State<'_, AppState>) -> AppResult<LibraryStats> {
    query::stats(state.library.db()).await
}

#[tauri::command]
pub fn library_scan_status(state: State<'_, AppState>) -> ScanStatus {
    state.library.status()
}

#[tauri::command]
pub async fn library_roots(state: State<'_, AppState>) -> AppResult<Vec<RootInfo>> {
    query::roots(state.library.db()).await
}

/// Asks for a folder and adds it to the library. Returns `None` if the
/// dialog was cancelled.
#[tauri::command]
pub async fn library_add_root(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<i64>> {
    let (reply, picked) = oneshot::channel();
    app.dialog().file().set_title("Add a music folder").pick_folder(move |folder| {
        let _ = reply.send(folder);
    });
    let Some(folder) = picked.await.map_err(|_| AppError::PlayerUnavailable)? else {
        return Ok(None);
    };
    let path = folder.into_path().map_err(|_| AppError::InvalidArgument("not a local folder".into()))?;
    state.library.add_root(path).await.map(Some)
}

/// Adds the user's standard music folder (XDG `MUSIC`, usually `~/Music`).
#[tauri::command]
pub async fn library_add_default_root(app: AppHandle, state: State<'_, AppState>) -> AppResult<i64> {
    use tauri::Manager;
    let path = app.path().audio_dir().map_err(|_| AppError::NotFound("music folder".into()))?;
    state.library.add_root(path).await
}

/// Where the standard music folder is and how many audio files it holds, for
/// the first-run screen.
#[tauri::command]
pub async fn library_default_root(app: AppHandle) -> AppResult<Option<DefaultRoot>> {
    use tauri::Manager;
    let Ok(path) = app.path().audio_dir() else { return Ok(None) };
    if !path.is_dir() {
        return Ok(None);
    }
    let display = path.display().to_string().replacen(&std::env::var("HOME").unwrap_or_default(), "~", 1);
    let files = tokio::task::spawn_blocking(move || count_audio_files(&path, 50_000))
        .await
        .map_err(|e| AppError::Io(std::io::Error::other(e)))?;
    Ok(Some(DefaultRoot { display, files }))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultRoot {
    display: String,
    files: usize,
}

fn count_audio_files(dir: &std::path::Path, limit: usize) -> usize {
    let mut count = 0;
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && depth < 32 {
                stack.push((entry.path(), depth + 1));
            } else if kind.is_file() && crate::filesystem::has_audio_extension(&entry.path()) {
                count += 1;
                if count >= limit {
                    return count;
                }
            }
        }
    }
    count
}

#[tauri::command]
pub async fn library_remove_root(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.library.remove_root(id).await
}

#[tauri::command]
pub fn library_rescan(state: State<'_, AppState>, root: Option<i64>) {
    state.library.rescan(root);
}

#[tauri::command]
pub async fn library_albums(state: State<'_, AppState>) -> AppResult<Vec<AlbumRow>> {
    query::albums(state.library.db()).await
}

#[tauri::command]
pub async fn library_album(state: State<'_, AppState>, id: i64) -> AppResult<AlbumDetail> {
    query::album(state.library.db(), id).await
}

#[tauri::command]
pub async fn library_artists(state: State<'_, AppState>, kind: ArtistKind) -> AppResult<Vec<ArtistRow>> {
    query::artists(state.library.db(), kind).await
}

#[tauri::command]
pub async fn library_artist(state: State<'_, AppState>, id: i64) -> AppResult<ArtistDetail> {
    query::artist(state.library.db(), id).await
}

#[tauri::command]
pub async fn library_genres(state: State<'_, AppState>) -> AppResult<Vec<GenreRow>> {
    query::genres(state.library.db()).await
}

#[tauri::command]
pub async fn library_genre(state: State<'_, AppState>, id: i64) -> AppResult<GenreDetail> {
    query::genre(state.library.db(), id).await
}

#[tauri::command]
pub async fn library_folder_children(state: State<'_, AppState>, id: i64) -> AppResult<Vec<FolderNode>> {
    query::folder_children(state.library.db(), id).await
}

#[tauri::command]
pub async fn library_folder(state: State<'_, AppState>, id: i64) -> AppResult<FolderDetail> {
    query::folder(state.library.db(), id).await
}

#[tauri::command]
pub async fn library_track_list(state: State<'_, AppState>, query: TrackQuery) -> AppResult<TrackList> {
    query::track_list(state.library.db(), &query).await
}

#[tauri::command]
pub async fn library_tracks(state: State<'_, AppState>, ids: Vec<i64>) -> AppResult<Vec<TrackRow>> {
    if ids.len() > MAX_ROWS {
        return Err(AppError::InvalidArgument(format!("at most {MAX_ROWS} rows per request")));
    }
    query::tracks_by_ids(state.library.db(), &ids).await
}

#[tauri::command]
pub async fn library_search(
    state: State<'_, AppState>,
    text: String,
    limit: Option<usize>,
) -> AppResult<SearchResults> {
    query::search(state.library.db(), &text, limit.unwrap_or(6).clamp(1, 50)).await
}

/// Plays library tracks: replaces the queue (starting at `start`, an index
/// into the resolved list), or inserts them next or at the end.
#[tauri::command]
pub async fn library_play(
    state: State<'_, AppState>,
    source: PlaySource,
    mode: InsertMode,
    start: Option<usize>,
    shuffle: Option<bool>,
) -> AppResult<usize> {
    let tracks = state.library.resolve(source).await?;
    let count = tracks.len();
    if let Some(shuffle) = shuffle {
        state.player.command(PlayerCommand::SetShuffle(shuffle)).await?;
    }
    state.player.command(PlayerCommand::Insert { tracks, mode, start: start.unwrap_or(0) }).await?;
    Ok(count)
}

#[tauri::command]
pub async fn library_set_favorite(state: State<'_, AppState>, ids: Vec<i64>, favorite: bool) -> AppResult<()> {
    state.library.set_favorite(&ids, favorite).await
}

#[tauri::command]
pub async fn library_favorite_ids(state: State<'_, AppState>) -> AppResult<Vec<i64>> {
    state.library.favorite_ids().await
}

#[tauri::command]
pub async fn library_history(state: State<'_, AppState>, limit: Option<i64>) -> AppResult<Vec<HistoryEntry>> {
    state.library.history(limit.unwrap_or(500).clamp(1, 5000)).await
}

#[tauri::command]
pub async fn library_clear_history(state: State<'_, AppState>) -> AppResult<()> {
    state.library.clear_history().await
}

/// Opens an indexed folder in the desktop's file manager.
#[tauri::command]
pub async fn library_reveal_folder(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let path = state.library.folder_path(id).await?;
    std::process::Command::new("xdg-open")
        .arg(&path)
        .spawn()
        .map_err(|e| AppError::Io(std::io::Error::other(format!("could not open the file manager: {e}"))))?;
    Ok(())
}

#[tauri::command]
pub async fn library_track_folder(state: State<'_, AppState>, id: i64) -> AppResult<i64> {
    state.library.track_folder(id).await
}

// ── playlists ──────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn playlists_list(state: State<'_, AppState>) -> AppResult<Vec<PlaylistRow>> {
    state.library.playlists().await
}

#[tauri::command]
pub async fn playlist_get(state: State<'_, AppState>, id: i64) -> AppResult<PlaylistRow> {
    state.library.playlist(id).await
}

#[tauri::command]
pub async fn playlist_create(state: State<'_, AppState>, name: String, tracks: Option<Vec<i64>>) -> AppResult<i64> {
    state.library.create_playlist(&name, tracks.as_deref().unwrap_or_default()).await
}

#[tauri::command]
pub async fn playlist_rename(state: State<'_, AppState>, id: i64, name: String) -> AppResult<()> {
    state.library.rename_playlist(id, &name).await
}

#[tauri::command]
pub async fn playlist_set_pinned(state: State<'_, AppState>, id: i64, pinned: bool) -> AppResult<()> {
    state.library.set_playlist_pinned(id, pinned).await
}

#[tauri::command]
pub async fn playlist_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.library.delete_playlist(id).await
}

#[tauri::command]
pub async fn playlist_add(state: State<'_, AppState>, id: i64, tracks: Vec<i64>) -> AppResult<usize> {
    state.library.add_to_playlist(id, &tracks).await
}

#[tauri::command]
pub async fn playlist_remove(state: State<'_, AppState>, id: i64, positions: Vec<usize>) -> AppResult<()> {
    state.library.remove_from_playlist(id, &positions).await
}

#[tauri::command]
pub async fn playlist_move(state: State<'_, AppState>, id: i64, positions: Vec<usize>, to: usize) -> AppResult<()> {
    state.library.move_in_playlist(id, &positions, to).await
}
