//! Audio output selection.
//!
//! MatrixPlayer talks to PipeWire natively when the `pipewiresink` plugin is
//! available, falls back to PipeWire's PulseAudio interface, then to plain
//! ALSA, and finally to whatever `autoaudiosink` picks.

use gstreamer as gst;
use gstreamer::prelude::*;
use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OutputBackend {
    PipeWire,
    PulseAudio,
    Alsa,
    Auto,
}

impl OutputBackend {
    fn factory(self) -> &'static str {
        match self {
            Self::PipeWire => "pipewiresink",
            Self::PulseAudio => "pulsesink",
            Self::Alsa => "alsasink",
            Self::Auto => "autoaudiosink",
        }
    }
}

const PREFERENCE: [OutputBackend; 4] =
    [OutputBackend::PipeWire, OutputBackend::PulseAudio, OutputBackend::Alsa, OutputBackend::Auto];

pub struct Output {
    pub sink: gst::Element,
    pub backend: OutputBackend,
}

/// Creates the first available output sink, honouring `MATRIXPLAYER_AUDIO_SINK`
/// (`pipewire`, `pulse`, `alsa` or `auto`) when set, for troubleshooting.
pub fn create() -> AppResult<Output> {
    let forced = std::env::var("MATRIXPLAYER_AUDIO_SINK").ok().and_then(|v| match v.as_str() {
        "pipewire" => Some(OutputBackend::PipeWire),
        "pulse" => Some(OutputBackend::PulseAudio),
        "alsa" => Some(OutputBackend::Alsa),
        "auto" => Some(OutputBackend::Auto),
        _ => None,
    });
    let candidates: Vec<_> = forced.into_iter().chain(PREFERENCE).collect();

    for backend in candidates {
        let Ok(sink) = gst::ElementFactory::make(backend.factory()).name("matrixplayer-output").build() else {
            continue;
        };
        describe_stream(&sink);
        log::info!("audio output: {}", backend.factory());
        return Ok(Output { sink, backend });
    }
    Err(AppError::Audio("No audio output is available (no PipeWire, PulseAudio or ALSA sink)".into()))
}

impl Output {
    /// A sink that discards audio, for tests. With `sync` off, streams run
    /// as fast as they decode.
    #[cfg(test)]
    pub fn fake(sync: bool) -> Self {
        let sink =
            gst::ElementFactory::make("fakesink").name("matrixplayer-output").property("sync", sync).build().unwrap();
        Self { sink, backend: OutputBackend::Auto }
    }
}

/// Identifies the stream to the sound server so it shows up as "MatrixPlayer" in
/// mixers and is routed with the music role.
fn describe_stream(sink: &gst::Element) {
    if sink.has_property("client-name") {
        sink.set_property("client-name", "MatrixPlayer");
    }
    if sink.has_property("stream-properties") {
        let props = gst::Structure::builder("props")
            .field("media.role", "Music")
            .field("media.category", "Playback")
            .field("application.name", "MatrixPlayer")
            .field("application.icon-name", "matrixplayer")
            .build();
        sink.set_property("stream-properties", props);
    }
}
