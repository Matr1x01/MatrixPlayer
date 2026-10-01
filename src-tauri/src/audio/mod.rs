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

pub mod diagnostics;
mod engine;
pub mod output;
pub mod reserve;
pub mod waveform;

use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use serde::Serialize;

pub use engine::AudioEngine;
pub use output::OutputBackend;

/// gst-libav's MP3 and Opus decoders, which output 32-bit float.
const FLOAT_DECODERS: [&str; 2] = ["avdec_mp3float", "avdec_opus"];

/// Makes GStreamer pick decoders that output float for MP3 and Opus.
///
/// The default ones (`mpg123audiodec`, `opusdec`) output 16-bit integers, so
/// a lossy stream would lose its resolution below 16 bits before any
/// processing. Both MP3 decoders trim LAME-tagged files to the exact sample
/// count, so gapless playback is unaffected. Without gst-libav this does
/// nothing. Call once, right after `gstreamer::init()`.
pub fn prefer_float_decoders() {
    let registry = gst::Registry::get();
    for name in FLOAT_DECODERS {
        if let Some(feature) = registry.lookup_feature(name) {
            feature.set_rank(gst::Rank::PRIMARY + 1);
            log::debug!("preferring {name}: it decodes to float");
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_decoders_outrank_the_16_bit_ones() {
        gst::init().unwrap();
        prefer_float_decoders();
        let registry = gst::Registry::get();
        let rank = |name: &str| registry.lookup_feature(name).map(|f| f.rank());
        for (float, fixed) in [("avdec_mp3float", "mpg123audiodec"), ("avdec_opus", "opusdec")] {
            if let (Some(float_rank), Some(fixed_rank)) = (rank(float), rank(fixed)) {
                assert!(float_rank > fixed_rank, "{float} {float_rank:?} vs {fixed} {fixed_rank:?}");
            }
        }
    }
}
