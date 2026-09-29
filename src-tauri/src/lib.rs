//! MatrixPlayer's native core.
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
mod desktop;
mod dsp;
mod error;
mod events;
mod filesystem;
mod library;
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
use crate::audio::waveform::WaveformCache;
use crate::database::Database;
use crate::dsp::service::DspService;
use crate::library::LibraryHandle;
use crate::player::output_mode::OutputMode;
use crate::player::{InsertMode, PlayerHandle, PlayerSettings};
use crate::state::AppState;

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,zbus=warn")).init();

    if let Err(err) = gstreamer::init() {
        eprintln!("MatrixPlayer needs GStreamer 1.x and could not initialise it: {err}");
        std::process::exit(1);
    }
    log::info!("{}", gstreamer::version_string());
    audio::prefer_float_decoders();

    tauri::Builder::default()
        // Must come first: a second launch hands its arguments (files to
        // play, desktop-entry actions) to the running instance and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            handle_invocation(app, desktop::parse_args(argv.into_iter().skip(1)));
        }))
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
                    .message(format!("MatrixPlayer could not start.\n\n{err}"))
                    .kind(MessageDialogKind::Error)
                    .title("MatrixPlayer")
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
            commands::player::player_lyrics,
            commands::player::player_waveform,
            commands::queue::queue_play_entry,
            commands::queue::queue_remove_entry,
            commands::queue::queue_clear,
            commands::queue::queue_clear_others,
            commands::queue::queue_undo,
            commands::queue::queue_move,
            commands::queue::queue_saved_session,
            commands::queue::queue_restore_session,
            commands::queue::open_audio_files,
            commands::queue::open_audio_folder,
            commands::library::library_stats,
            commands::library::library_scan_status,
            commands::library::library_roots,
            commands::library::library_add_root,
            commands::library::library_add_default_root,
            commands::library::library_default_root,
            commands::library::library_remove_root,
            commands::library::library_rescan,
            commands::library::library_albums,
            commands::library::library_album,
            commands::library::library_artists,
            commands::library::library_artist,
            commands::library::library_genres,
            commands::library::library_genre,
            commands::library::library_folder_children,
            commands::library::library_folder,
            commands::library::library_track_list,
            commands::library::library_tracks,
            commands::library::library_search,
            commands::library::library_play,
            commands::library::library_set_favorite,
            commands::library::library_favorite_ids,
            commands::library::library_history,
            commands::library::library_clear_history,
            commands::library::library_reveal_folder,
            commands::library::library_track_folder,
            commands::library::playlists_list,
            commands::library::playlist_get,
            commands::library::playlist_create,
            commands::library::playlist_rename,
            commands::library::playlist_set_pinned,
            commands::library::playlist_delete,
            commands::library::playlist_add,
            commands::library::playlist_remove,
            commands::library::playlist_move,
            commands::dsp::dsp_state,
            commands::dsp::dsp_update,
            commands::dsp::dsp_restore_defaults,
            commands::dsp::dsp_save_preset,
            commands::dsp::dsp_delete_preset,
            commands::dsp::dsp_export_presets,
            commands::dsp::dsp_import_presets,
            commands::dsp::dsp_set_analysis,
            commands::dsp::dsp_eq_response,
            commands::dsp::audio_output_devices,
            commands::dsp::player_set_output_device,
            commands::dsp::player_set_bit_perfect,
            commands::dsp::player_set_exclusive,
            commands::dsp::player_set_gapless,
            commands::system::get_app_info,
            commands::system::get_ui_preferences,
            commands::system::update_ui_preferences,
        ])
        .build(tauri::generate_context!())
        .expect("error while building MatrixPlayer")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app.try_state::<AppState>()
            {
                state.library.shutdown();
                tauri::async_runtime::block_on(async {
                    state.player.flush().await;
                    state.dsp.flush().await;
                });
            }
        });
}

fn setup(app: &AppHandle) -> Result<(), error::AppError> {
    let path_err = |e: tauri::Error| error::AppError::Io(std::io::Error::other(e.to_string()));
    let data_dir = app.path().app_data_dir().map_err(path_err)?;
    let cache_dir = app.path().app_cache_dir().map_err(path_err)?;

    let db = tauri::async_runtime::block_on(Database::open(&data_dir.join("matrixplayer.db")))?;
    let artwork = Arc::new(ArtworkStore::new(cache_dir.join("artwork"))?);
    let settings = tauri::async_runtime::block_on(PlayerSettings::load(&db));

    let output = audio::output::create(settings.output_device.as_deref())?;
    let (player, service) = PlayerHandle::new(db.clone(), settings, output)?;
    let dsp = tauri::async_runtime::block_on(DspService::start(db.clone(), player.dsp()));
    tauri::async_runtime::spawn(service);
    let output_mode = tauri::async_runtime::block_on(OutputMode::start(player.clone(), db.clone()));
    events::forward_player_events(app.clone(), player.subscribe());
    events::forward_dsp_events(app.clone(), dsp.control());

    let (library, library_task) = LibraryHandle::new(db.clone(), artwork.clone());
    tauri::async_runtime::spawn(library_task);
    events::forward_library_events(app.clone(), library.subscribe());
    library::listening::spawn(library.clone(), player.subscribe());

    app.manage(artwork.clone());
    let waveforms = Arc::new(WaveformCache::new(cache_dir.join("waveforms"))?);
    desktop::mpris::spawn(app.clone(), player.clone(), artwork.clone());
    desktop::notifications::spawn(app.clone(), player.clone(), artwork.clone(), db.clone());
    app.manage(AppState { player, library, artwork, dsp, waveforms, db, output_mode });

    // Files on the command line play; transport flags only matter to an
    // instance that is already running. Otherwise the last session comes
    // back, paused where it was, unless that is turned off.
    if let desktop::Invocation::Open(files) = desktop::parse_args(std::env::args().skip(1)) {
        queue_paths(app, files, InsertMode::Replace);
    } else {
        restore_last_session(app);
    }
    Ok(())
}

/// Brings back the queue saved when the app last closed, if the preference
/// allows it and there is one.
fn restore_last_session(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let enabled = preferences::load(&state.db).await.map(|p| p.restore_session).unwrap_or(true);
        if !enabled {
            return;
        }
        match commands::queue::restore_session(&state).await {
            Ok(count) => log::info!("restored the last session ({count} tracks)"),
            Err(error::AppError::NotFound(_)) => {}
            Err(e) => log::warn!("could not restore the last session: {e}"),
        }
    });
}

/// Acts on the arguments of a second launch.
fn handle_invocation(app: &AppHandle, invocation: desktop::Invocation) {
    match invocation {
        desktop::Invocation::Command(command) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = app.try_state::<AppState>()
                    && let Err(e) = state.player.command(command).await
                {
                    log::debug!("desktop action failed: {e}");
                }
            });
        }
        desktop::Invocation::Open(files) => {
            queue_paths(app, files, InsertMode::Replace);
            desktop::show_main_window(app);
        }
        desktop::Invocation::Show => desktop::show_main_window(app),
    }
}

/// Queues files from outside the UI (drag and drop, command line) and tells
/// the frontend how many were added.
fn queue_paths(app: &AppHandle, paths: Vec<PathBuf>, mode: InsertMode) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        match state.open_paths(paths, mode).await {
            Ok(count) => events::emit(&app, events::TRACKS_QUEUED, events::TracksQueuedPayload { count }),
            Err(err) => log::warn!("could not queue files: {err}"),
        }
    });
}
