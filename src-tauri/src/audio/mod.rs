//! Native audio playback on GStreamer.
//!
//! [`AudioEngine`] owns one `playbin3` pipeline for the life of the app:
//!
//! ```text
//! file ─ demux ─ decode ─ [dsp chain] ─ output sink (PipeWire / Pulse / ALSA)
//! ```
//!
//! The engine knows nothing about queues or tracks. It plays URIs, reports
//! what the pipeline does as [`EngineEvent`]s, and switches to a preloaded
//! next URI without a gap when the current stream is about to end.

mod engine;
pub mod output;

use std::time::Duration;

use serde::Serialize;

pub use engine::AudioEngine;
pub use output::OutputBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    Null,
    Ready,
    Paused,
    Playing,
}

/// A raw PCM format as negotiated inside the pipeline.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamFormat {
    /// GStreamer sample format name, e.g. `S24LE`, `S32LE`, `F32LE`.
    pub sample_format: String,
    /// Significant bits per sample.
    pub bits: u32,
    pub float: bool,
    pub sample_rate: u32,
    pub channels: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EngineErrorKind {
    NotFound,
    Unreadable,
    UnsupportedFormat,
    Corrupt,
    OutputUnavailable,
    Other,
}

#[derive(Debug, Clone)]
pub struct EngineError {
    pub kind: EngineErrorKind,
    pub message: String,
    /// GStreamer's debug string, for logs and the "details" disclosure.
    pub detail: Option<String>,
    /// The stream that failed, when known.
    pub uri: Option<String>,
}

/// An [`EngineEvent`] tagged with the pipeline epoch it belongs to.
#[derive(Debug, Clone)]
pub struct EngineMessage {
    pub epoch: u64,
    pub event: EngineEvent,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    StateChanged(EngineState),
    /// A new stream reached the output. `gapless` is true when the engine
    /// switched to the preloaded next URI on its own.
    StreamStarted {
        uri: String,
        gapless: bool,
    },
    /// A pending state change or seek finished.
    AsyncDone,
    EndOfStream,
    Error(EngineError),
    Position {
        position: Duration,
        duration: Option<Duration>,
    },
    /// The stream's duration became known or changed; query it.
    DurationChanged,
    /// Format of the decoded stream entering the DSP chain.
    DecodedFormat(StreamFormat),
    /// Format handed to the output sink.
    OutputFormat(StreamFormat),
}
