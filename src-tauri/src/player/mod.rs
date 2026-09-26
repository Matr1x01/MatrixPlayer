//! Playback domain: the queue, the authoritative playback state, and the
//! service that drives the audio engine.
//!
//! The frontend never keeps its own playback state. It renders the
//! [`PlayerSnapshot`] it fetched at startup and applies [`PlayerEvent`]s as
//! the service publishes them.

mod queue;
mod service;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::artwork::{ArtworkInfo, ArtworkStore, FolderArtCache};
use crate::audio::{EngineErrorKind, OutputBackend, StreamFormat};
use crate::metadata::{self, AudioProperties, TrackMetadata};

pub use queue::{EntryId, InsertMode, QueueEntry, RepeatMode};
pub use service::{PlayerCommand, PlayerHandle, PlayerSettings};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackStatus {
    /// Nothing is loaded.
    Idle,
    /// A track is being opened and prerolled.
    Loading,
    Playing,
    Paused,
    /// A seek was issued and the pipeline has not settled yet.
    Seeking,
    /// A track is loaded but playback was stopped or reached the end.
    Stopped,
    /// Playback failed and did not recover.
    Error,
}

/// Everything the UI shows about one track.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    /// The library row this track came from, if it is in the library.
    pub library_id: Option<i64>,
    #[serde(skip)]
    pub path: PathBuf,
    /// The path for display. Non-UTF-8 bytes are replaced.
    pub display_path: String,
    pub file_name: String,
    pub metadata: TrackMetadata,
    pub properties: AudioProperties,
    pub artwork: Option<ArtworkInfo>,
    /// Set when the file could not be probed; playback may still work.
    pub probe_error: Option<String>,
}

impl TrackInfo {
    /// Reads a file's tags, properties and artwork. Never fails: unreadable
    /// files get a minimal entry and GStreamer decides whether they play.
    pub fn load(path: &Path, artwork: &ArtworkStore) -> Self {
        Self::load_cached(path, artwork, None)
    }

    /// [`Self::load`], sharing folder-artwork lookups through `cache`.
    pub fn load_cached(path: &Path, artwork: &ArtworkStore, cache: Option<&FolderArtCache>) -> Self {
        let display_path = path.to_string_lossy().into_owned();
        let file_name = path.file_name().map_or_else(|| display_path.clone(), |n| n.to_string_lossy().into_owned());
        match metadata::probe(path) {
            Ok(probed) => Self {
                library_id: None,
                artwork: artwork.resolve_cached(path, probed.picture.as_ref(), cache),
                path: path.to_path_buf(),
                display_path,
                file_name,
                metadata: probed.metadata,
                properties: probed.properties,
                probe_error: None,
            },
            Err(err) => {
                log::warn!("{err}");
                let stem = path.file_stem().map_or_else(|| file_name.clone(), |s| s.to_string_lossy().into_owned());
                let codec = path.extension().map_or_else(String::new, |e| e.to_string_lossy().to_ascii_uppercase());
                Self {
                    library_id: None,
                    artwork: artwork.resolve_cached(path, None, cache),
                    path: path.to_path_buf(),
                    display_path,
                    file_name,
                    metadata: TrackMetadata { title: stem, derived_fields: vec!["title"], ..Default::default() },
                    properties: AudioProperties {
                        codec: codec.clone(),
                        container: codec,
                        lossless: false,
                        sample_rate: None,
                        bit_depth: None,
                        channels: None,
                        bitrate_kbps: None,
                        duration_ms: 0,
                        file_size: std::fs::metadata(path).map_or(0, |m| m.len()),
                    },
                    probe_error: Some(err.to_string()),
                }
            }
        }
    }

    #[cfg(test)]
    pub fn for_test(title: &str) -> Self {
        Self {
            library_id: None,
            path: PathBuf::from(format!("/music/{title}.flac")),
            display_path: format!("/music/{title}.flac"),
            file_name: format!("{title}.flac"),
            metadata: TrackMetadata { title: title.to_owned(), ..Default::default() },
            properties: AudioProperties {
                codec: "FLAC".into(),
                container: "FLAC".into(),
                lossless: true,
                sample_rate: Some(44_100),
                bit_depth: Some(16),
                channels: Some(2),
                bitrate_kbps: Some(900),
                duration_ms: 180_000,
                file_size: 20_000_000,
            },
            artwork: None,
            probe_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    /// Entries in play order.
    pub entries: Vec<QueueEntry>,
    /// Index into `entries` of the current entry.
    pub current_index: Option<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

/// The part of the queue that changes on every track change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueState {
    pub current_index: Option<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeState {
    /// Slider position, 0–1.
    pub level: f64,
    pub muted: bool,
    /// Applied gain in dB; `None` means silence.
    pub gain_db: Option<f64>,
}

/// What the signal path currently looks like, from decoder to device.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalPath {
    pub backend: OutputBackend,
    pub decoded: Option<StreamFormat>,
    pub output: Option<StreamFormat>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerError {
    pub kind: EngineErrorKind,
    pub message: String,
    pub detail: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub status: PlaybackStatus,
    pub track: Option<Arc<TrackInfo>>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: VolumeState,
    pub queue: QueueSnapshot,
    pub signal: SignalPath,
    pub error: Option<PlayerError>,
}

/// Changes published by the player. Each maps to one frontend event.
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Status {
        status: PlaybackStatus,
        error: Option<PlayerError>,
    },
    Track {
        track: Option<Arc<TrackInfo>>,
    },
    Position {
        position_ms: u64,
        duration_ms: Option<u64>,
    },
    Volume(VolumeState),
    /// Contents or order changed.
    Queue(QueueSnapshot),
    /// Only the current entry or the modes changed.
    QueueState(QueueState),
    Signal(SignalPath),
    /// A playback failure the user should hear about, even if playback
    /// recovered by skipping to the next track.
    Error(PlayerError),
}
