//! Audio output selection.
//!
//! MatrixPlayer talks to PipeWire natively when the `pipewiresink` plugin is
//! available, falls back to PipeWire's PulseAudio interface, then to plain
//! ALSA, and finally to whatever `autoaudiosink` picks.
//!
//! Devices are listed through the chosen backend's GStreamer device
//! provider, so a device picked in the UI is always one the active sink can
//! open. With no device chosen, the sound server's default is followed,
//! including when the user switches it elsewhere.

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
    /// Discards audio in real time (`MATRIXPLAYER_AUDIO_SINK=null`), for
    /// headless runs and troubleshooting.
    Null,
}

impl OutputBackend {
    fn factory(self) -> &'static str {
        match self {
            Self::PipeWire => "pipewiresink",
            Self::PulseAudio => "pulsesink",
            Self::Alsa => "alsasink",
            Self::Auto => "autoaudiosink",
            Self::Null => "fakesink",
        }
    }

    fn device_provider(self) -> Option<&'static str> {
        match self {
            Self::PipeWire => Some("pipewiredeviceprovider"),
            Self::PulseAudio => Some("pulsedeviceprovider"),
            Self::Alsa => Some("alsadeviceprovider"),
            Self::Auto | Self::Null => None,
        }
    }
}

const PREFERENCE: [OutputBackend; 4] =
    [OutputBackend::PipeWire, OutputBackend::PulseAudio, OutputBackend::Alsa, OutputBackend::Auto];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Usb,
    Bluetooth,
    Hdmi,
    Headphones,
    Internal,
    Other,
}

/// An output device as the UI lists it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputDevice {
    /// Stable id (the sound server's node or sink name), persisted.
    pub id: String,
    pub name: String,
    /// Connection and capabilities, e.g. "USB · up to 32-bit / 384 kHz".
    pub detail: String,
    pub kind: DeviceKind,
}

pub struct Output {
    pub sink: gst::Element,
    pub backend: OutputBackend,
    /// The device chosen for this output; `None` follows the default.
    pub device: Option<OutputDevice>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output").field("backend", &self.backend).field("device", &self.device).finish()
    }
}

/// The backend `create` would pick, honouring `MATRIXPLAYER_AUDIO_SINK`.
fn candidates() -> Vec<OutputBackend> {
    let forced = std::env::var("MATRIXPLAYER_AUDIO_SINK").ok().and_then(|v| match v.as_str() {
        "pipewire" => Some(OutputBackend::PipeWire),
        "pulse" => Some(OutputBackend::PulseAudio),
        "alsa" => Some(OutputBackend::Alsa),
        "auto" => Some(OutputBackend::Auto),
        "null" => Some(OutputBackend::Null),
        _ => None,
    });
    forced.into_iter().chain(PREFERENCE).collect()
}

/// Creates the first available output sink, honouring `MATRIXPLAYER_AUDIO_SINK`
/// (`pipewire`, `pulse`, `alsa`, `auto` or `null`) when set, for troubleshooting.
///
/// With `device`, the sink is opened on that device if the backend still
/// lists it; otherwise the default output is used.
pub fn create(device: Option<&str>) -> AppResult<Output> {
    for backend in candidates() {
        let Ok(sink) = gst::ElementFactory::make(backend.factory()).name(SINK_NAME).build() else {
            continue;
        };
        if backend == OutputBackend::Null {
            sink.set_property("sync", true);
        }
        let mut output = Output { sink, backend, device: None };
        if let Some(id) = device {
            match open_device(backend, id) {
                Some((sink, found)) => {
                    output.sink = sink;
                    output.device = Some(found);
                }
                None => log::warn!("output device {id} is not available; using the default output"),
            }
        }
        describe_stream(&output.sink);
        log::info!(
            "audio output: {}{}",
            backend.factory(),
            output.device.as_ref().map(|d| format!(" on {}", d.name)).unwrap_or_default()
        );
        return Ok(output);
    }
    Err(AppError::Audio("No audio output is available (no PipeWire, PulseAudio or ALSA sink)".into()))
}

const SINK_NAME: &str = "matrixplayer-output";

/// Lists the output devices of the backend in use. Blocking: providers may
/// talk to the sound server.
pub fn devices(backend: OutputBackend) -> Vec<OutputDevice> {
    provider_devices(backend).into_iter().map(|(device, _)| device).collect()
}

fn provider_devices(backend: OutputBackend) -> Vec<(OutputDevice, gst::Device)> {
    let Some(provider) = backend.device_provider().and_then(gst::DeviceProviderFactory::by_name) else {
        return Vec::new();
    };
    let mut out: Vec<(OutputDevice, gst::Device)> = provider
        .devices()
        .into_iter()
        .filter(|d| d.has_classes("Audio/Sink"))
        .filter_map(|d| describe_device(&d).map(|info| (info, d)))
        .collect();
    out.sort_by(|a, b| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()));
    out.dedup_by(|a, b| a.0.id == b.0.id);
    out
}

fn open_device(backend: OutputBackend, id: &str) -> Option<(gst::Element, OutputDevice)> {
    let (info, device) = provider_devices(backend).into_iter().find(|(info, _)| info.id == id)?;
    match device.create_element(Some(SINK_NAME)) {
        Ok(sink) => Some((sink, info)),
        Err(e) => {
            log::warn!("could not open {}: {e}", info.name);
            None
        }
    }
}

fn describe_device(device: &gst::Device) -> Option<OutputDevice> {
    let props = device.properties();
    let prop = |key: &str| props.as_ref().and_then(|p| p.get::<String>(key).ok()).filter(|s| !s.is_empty());
    let internal_name = || {
        device
            .has_property("internal-name")
            .then(|| device.property::<Option<String>>("internal-name"))
            .flatten()
            .filter(|s| !s.is_empty())
    };
    let id = prop("node.name").or_else(|| prop("device.name")).or_else(internal_name)?;
    let name = device.display_name().to_string();

    let bus = prop("device.bus").unwrap_or_default().to_lowercase();
    let form = prop("device.form-factor").or_else(|| prop("device.form_factor")).unwrap_or_default().to_lowercase();
    let haystack = format!("{id} {name}").to_lowercase();
    let kind = if bus == "bluetooth" || haystack.contains("bluez") {
        DeviceKind::Bluetooth
    } else if haystack.contains("hdmi") || haystack.contains("displayport") {
        DeviceKind::Hdmi
    } else if bus == "usb" || haystack.contains("usb") {
        DeviceKind::Usb
    } else if matches!(form.as_str(), "headphone" | "headset") {
        DeviceKind::Headphones
    } else if bus == "pci" || form == "internal" || haystack.contains("pci") {
        DeviceKind::Internal
    } else {
        DeviceKind::Other
    };
    let connection = match kind {
        DeviceKind::Usb => "USB",
        DeviceKind::Bluetooth => "Bluetooth",
        DeviceKind::Hdmi => "HDMI",
        DeviceKind::Headphones => "Headphones",
        DeviceKind::Internal => "Internal",
        DeviceKind::Other => "Output",
    };
    let detail = match device.caps().as_ref().and_then(capabilities) {
        Some(caps) => format!("{connection} · {caps}"),
        None => connection.to_owned(),
    };
    Some(OutputDevice { id, name, detail, kind })
}

/// "up to 32-bit / 384 kHz" from a device's caps, when it states limits.
fn capabilities(caps: &gst::Caps) -> Option<String> {
    let mut max_rate = 0;
    let mut max_bits = 0;
    for s in caps.iter() {
        if let Ok(rate) = s.get::<i32>("rate") {
            max_rate = max_rate.max(rate);
        } else if let Ok(range) = s.get::<gst::IntRange<i32>>("rate") {
            max_rate = max_rate.max(range.max());
        } else if let Ok(list) = s.get::<gst::List>("rate") {
            max_rate = max_rate.max(list.iter().filter_map(|v| v.get::<i32>().ok()).max().unwrap_or(0));
        }
        let formats: Vec<String> = if let Ok(f) = s.get::<String>("format") {
            vec![f]
        } else if let Ok(list) = s.get::<gst::List>("format") {
            list.iter().filter_map(|v| v.get::<String>().ok()).collect()
        } else {
            Vec::new()
        };
        for f in formats {
            let info = gstreamer_audio::AudioFormat::from_string(&f);
            if info != gstreamer_audio::AudioFormat::Unknown {
                let fi = gstreamer_audio::AudioFormatInfo::from_format(info);
                // Float formats say nothing about the DAC's resolution.
                if !fi.is_float() {
                    max_bits = max_bits.max(fi.depth());
                }
            }
        }
    }
    // Providers that accept anything (PipeWire converts) report huge ranges.
    let rate = (8_000..=768_000).contains(&max_rate).then_some(max_rate);
    let bits = (max_bits > 0).then_some(max_bits);
    match (bits, rate) {
        (Some(b), Some(r)) => Some(format!("up to {b}-bit / {} kHz", khz(r))),
        (None, Some(r)) => Some(format!("up to {} kHz", khz(r))),
        (Some(b), None) => Some(format!("up to {b}-bit")),
        (None, None) => None,
    }
}

fn khz(rate: i32) -> String {
    let k = f64::from(rate) / 1000.0;
    if k.fract() == 0.0 { format!("{k:.0}") } else { format!("{k:.1}") }
}

impl Output {
    /// A sink that discards audio, for tests. With `sync` off, streams run
    /// as fast as they decode.
    #[cfg(test)]
    pub fn fake(sync: bool) -> Self {
        let sink = gst::ElementFactory::make("fakesink").name(SINK_NAME).property("sync", sync).build().unwrap();
        Self { sink, backend: OutputBackend::Auto, device: None }
    }

    /// The sink's buffer size, when it is an audio sink that has one.
    pub fn buffer_ms(sink: &gst::Element) -> Option<u32> {
        sink.has_property("buffer-time")
            .then(|| sink.property::<i64>("buffer-time"))
            .and_then(|us| u32::try_from(us / 1000).ok())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_limits_from_device_caps() {
        gst::init().unwrap();
        let caps: gst::Caps =
            "audio/x-raw, format=(string){ S16LE, S24LE, S32LE }, rate=(int)[ 44100, 384000 ]; audio/x-raw, format=F32LE"
                .parse()
                .unwrap();
        assert_eq!(capabilities(&caps).as_deref(), Some("up to 32-bit / 384 kHz"));
        let caps: gst::Caps = "audio/x-raw, format=(string)F32LE, rate=(int)[ 1, 2147483647 ]".parse().unwrap();
        assert_eq!(capabilities(&caps), None, "unbounded, converting outputs state no limits");
        let caps: gst::Caps = "audio/x-raw, format=(string)S16LE, rate=(int){ 44100, 48000 }".parse().unwrap();
        assert_eq!(capabilities(&caps).as_deref(), Some("up to 16-bit / 48 kHz"));
    }
}
