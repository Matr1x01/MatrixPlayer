//! Rust → frontend events.
//!
//! The player publishes domain events on a broadcast channel; this bridge is
//! the only place that turns them into Tauri events. Other consumers (MPRIS,
//! notifications) subscribe to the same channel independently.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::player::{PlaybackStatus, PlayerError, PlayerEvent, TrackInfo};

pub const PLAYBACK_STATE_CHANGED: &str = "playback-state-changed";
pub const TRACK_CHANGED: &str = "track-changed";
pub const POSITION_CHANGED: &str = "position-changed";
pub const VOLUME_CHANGED: &str = "volume-changed";
pub const QUEUE_CHANGED: &str = "queue-changed";
pub const QUEUE_STATE_CHANGED: &str = "queue-state-changed";
pub const SIGNAL_PATH_CHANGED: &str = "signal-path-changed";
pub const AUDIO_ERROR: &str = "audio-error";
/// Sent when events were dropped; the frontend should refetch the snapshot.
pub const PLAYER_RESYNC: &str = "player-resync";
pub const TRACKS_QUEUED: &str = "tracks-queued";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatusPayload {
    status: PlaybackStatus,
    error: Option<PlayerError>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TrackPayload {
    track: Option<std::sync::Arc<TrackInfo>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PositionPayload {
    position_ms: u64,
    duration_ms: Option<u64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TracksQueuedPayload {
    pub count: usize,
}

pub fn forward_player_events(app: AppHandle, mut events: broadcast::Receiver<PlayerEvent>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(skipped)) => {
                    log::warn!("frontend bridge skipped {skipped} player events");
                    emit(&app, PLAYER_RESYNC, ());
                    continue;
                }
                Err(RecvError::Closed) => break,
            };
            match event {
                PlayerEvent::Status { status, error } => {
                    emit(&app, PLAYBACK_STATE_CHANGED, StatusPayload { status, error })
                }
                PlayerEvent::Track { track } => emit(&app, TRACK_CHANGED, TrackPayload { track }),
                PlayerEvent::Position { position_ms, duration_ms } => {
                    emit(&app, POSITION_CHANGED, PositionPayload { position_ms, duration_ms })
                }
                PlayerEvent::Volume(volume) => emit(&app, VOLUME_CHANGED, volume),
                PlayerEvent::Queue(queue) => emit(&app, QUEUE_CHANGED, queue),
                PlayerEvent::QueueState(state) => emit(&app, QUEUE_STATE_CHANGED, state),
                PlayerEvent::Signal(signal) => emit(&app, SIGNAL_PATH_CHANGED, signal),
                PlayerEvent::Error(error) => emit(&app, AUDIO_ERROR, error),
            }
        }
    });
}

pub fn emit<T: Serialize + Clone>(app: &AppHandle, name: &str, payload: T) {
    if let Err(e) = app.emit(name, payload) {
        log::warn!("could not emit {name}: {e}");
    }
}
