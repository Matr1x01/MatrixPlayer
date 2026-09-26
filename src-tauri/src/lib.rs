//! Fermata's native core.
//!
//! ```text
//! React UI ── commands ──▶ commands/ ──▶ player / preferences / …
//!          ◀── events ──── events.rs ◀── player broadcast channel
//! player ──▶ audio (GStreamer playbin3 + dsp chain) ──▶ PipeWire
//! ```

mod artwork;
mod audio;
mod commands;
mod database;
mod dsp;
mod error;
mod events;
mod filesystem;
mod metadata;
mod player;
mod preferences;
mod state;
#[cfg(test)]
mod test_support;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, DragDropEvent, Manager, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::artwork::ArtworkStore;
use crate::database::Database;
use crate::player::{InsertMode, PlayerHandle, PlayerSettings};
use crate::state::AppState;

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if let Err(err) = gstreamer::init() {
        eprintln!("Fermata needs GStreamer 1.x and could not initialise it: {err}");
        std::process::exit(1);
    }
    log::info!("{}", gstreamer::version_string());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("art", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || {
                let response = match app.try_state::<Arc<ArtworkStore>>() {
                    Some(store) => artwork::protocol::respond(&store, &path),
                    None => artwork::protocol::unavailable(),
                };
                responder.respond(response);
            });
        })
        .setup(|app| {
            if let Err(err) = setup(app.handle()) {
                log::error!("startup failed: {err}");
                app.dialog()
                    .message(format!("Fermata could not start.\n\n{err}"))
                    .kind(MessageDialogKind::Error)
                    .title("Fermata")
                    .blocking_show();
                return Err(err.into());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
                queue_paths(window.app_handle(), paths.clone(), InsertMode::Append);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::player::get_player_snapshot,
            commands::player::player_play,
            commands::player::player_pause,
            commands::player::player_toggle,
            commands::player::player_stop,
            commands::player::player_next,
            commands::player::player_previous,
            commands::player::player_seek,
            commands::player::player_set_volume,
            commands::player::player_set_muted,
            commands::player::player_set_repeat,
            commands::player::player_set_shuffle,
            commands::queue::queue_play_entry,
            commands::queue::queue_remove_entry,
            commands::queue::queue_clear,
            commands::queue::open_audio_files,
            commands::queue::open_audio_folder,
            commands::system::get_app_info,
            commands::system::get_ui_preferences,
            commands::system::update_ui_preferences,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Fermata")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app.try_state::<AppState>()
            {
                tauri::async_runtime::block_on(state.player.flush());
            }
        });
}

fn setup(app: &AppHandle) -> Result<(), error::AppError> {
    let path_err = |e: tauri::Error| error::AppError::Io(std::io::Error::other(e.to_string()));
    let data_dir = app.path().app_data_dir().map_err(path_err)?;
    let cache_dir = app.path().app_cache_dir().map_err(path_err)?;

    let db = tauri::async_runtime::block_on(Database::open(&data_dir.join("fermata.db")))?;
    let artwork = Arc::new(ArtworkStore::new(cache_dir.join("artwork"))?);
    let settings = tauri::async_runtime::block_on(PlayerSettings::load(&db));

    let output = audio::output::create()?;
    let (player, service) = PlayerHandle::new(db.clone(), artwork.clone(), settings, output)?;
    tauri::async_runtime::spawn(service);
    events::forward_player_events(app.clone(), player.subscribe());

    app.manage(artwork);
    app.manage(AppState { player, db });

    let cli_files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.exists()).collect();
    if !cli_files.is_empty() {
        queue_paths(app, cli_files, InsertMode::Replace);
    }
    Ok(())
}

/// Queues files from outside the UI (drag and drop, command line) and tells
/// the frontend how many were added.
fn queue_paths(app: &AppHandle, paths: Vec<PathBuf>, mode: InsertMode) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let player = app.state::<AppState>().player.clone();
        match player.open_paths(paths, mode).await {
            Ok(count) => events::emit(&app, events::TRACKS_QUEUED, events::TracksQueuedPayload { count }),
            Err(err) => log::warn!("could not queue files: {err}"),
        }
    });
}
