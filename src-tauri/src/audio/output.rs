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
//!
//! A device the sound server drives through ALSA can also be opened
//! exclusively: straight on its hardware PCM, bypassing the server, once
//! the card has been borrowed from it (see `reserve`).

use std::sync::{Mutex, RwLock};

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
    /// The ALSA hardware PCM behind it, when there is one: the device can
    /// then be opened exclusively.
    #[serde(skip)]
    pub pcm: Option<AlsaPcm>,
    /// Whether it can be opened exclusively (it has an ALSA PCM).
    pub exclusive: bool,
}

/// An ALSA hardware PCM: card and device number, as in `hw:2,0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlsaPcm {
    pub card: u32,
    pub device: u32,
}

pub struct Output {
    pub sink: gst::Element,
    pub backend: OutputBackend,
    /// The device chosen for this output; `None` follows the default.
    pub device: Option<OutputDevice>,
    /// Opened exclusively, on the device's hardware PCM.
    pub exclusive: bool,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output")
            .field("backend", &self.backend)
            .field("device", &self.device)
            .field("exclusive", &self.exclusive)
            .finish()
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
        if backend == OutputBackend::PipeWire {
            // Started before any stream is open: see `provider`.
            provider(backend);
        }
        if backend == OutputBackend::Null {
            sink.set_property("sync", true);
        }
        let mut output = Output { sink, backend, device: None, exclusive: false };
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

/// Opens `device` exclusively, on its ALSA hardware PCM. The card must
/// already be borrowed from the sound server, or opening it fails as busy.
/// Nothing converts behind it: the stream's rate and format go to the
/// hardware as they are, when it takes them.
pub fn open_exclusive(device: &OutputDevice) -> AppResult<Output> {
    let pcm = device.pcm.ok_or_else(|| AppError::Audio(format!("{} cannot be opened exclusively", device.name)))?;
    let sink = gst::ElementFactory::make("alsasink")
        .name(SINK_NAME)
        .property("device", format!("hw:{},{}", pcm.card, pcm.device))
        .build()
        .map_err(|_| AppError::Audio("GStreamer's ALSA output is not installed".into()))?;
    log::info!("audio output: alsasink on hw:{},{} ({}), exclusive", pcm.card, pcm.device, device.name);
    Ok(Output { sink, backend: OutputBackend::Alsa, device: Some(device.clone()), exclusive: true })
}

/// The backend shared playback uses (what [`create`] picks first).
pub fn shared_backend() -> OutputBackend {
    candidates().into_iter().find(|b| gst::ElementFactory::find(b.factory()).is_some()).unwrap_or(OutputBackend::Auto)
}

/// An output that renders nothing, in real time: a stand-in while a device
/// changes hands, so neither the old nor the new one is open.
pub fn silent() -> Output {
    let sink = gst::ElementFactory::make("fakesink")
        .name(SINK_NAME)
        .property("sync", true)
        .build()
        .expect("fakesink is part of GStreamer core");
    Output { sink, backend: OutputBackend::Null, device: None, exclusive: false }
}

/// The device with `id` among those of `backend`, if it is still there.
pub fn find_device(backend: OutputBackend, id: &str) -> Option<OutputDevice> {
    devices(backend).into_iter().find(|d| d.id == id)
}

/// PipeWire's clock settings (its `settings` metadata): the rate the graph
/// runs at when nothing asks for another, the rates it may switch to for a
/// stream, and a rate forced on everything (0: none).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PipeWireClock {
    pub rate: u32,
    pub allowed_rates: Vec<u32>,
    pub force_rate: u32,
}

impl PipeWireClock {
    /// The rate the device runs at while a stream at `rate` plays alone.
    pub fn rate_for(&self, rate: u32) -> u32 {
        if self.force_rate > 0 {
            self.force_rate
        } else if self.allowed_rates.contains(&rate) {
            rate
        } else {
            self.rate
        }
    }

    fn parse(metadata: &str) -> Option<Self> {
        // Lines like: update: id:0 key:'clock.allowed-rates' value:'[ 44100 48000 ]' type:''
        let value = |key: &str| {
            let line = metadata.lines().find(|l| l.contains(&format!("key:'{key}'")))?;
            let start = line.find("value:'")? + "value:'".len();
            let end = start + line[start..].find('\'')?;
            Some(line[start..end].to_owned())
        };
        let numbers =
            |v: String| -> Vec<u32> { v.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect() };
        let rate = numbers(value("clock.rate")?).first().copied()?;
        let allowed_rates = value("clock.allowed-rates").map(numbers).filter(|r| !r.is_empty()).unwrap_or(vec![rate]);
        let force_rate = value("clock.force-rate").and_then(|v| numbers(v).first().copied()).unwrap_or(0);
        Some(Self { rate, allowed_rates, force_rate })
    }
}

static PIPEWIRE_CLOCK: RwLock<Option<PipeWireClock>> = RwLock::new(None);

/// Reads PipeWire's clock settings with `pw-metadata` and keeps them for
/// [`pipewire_clock`]. Blocking (runs a short process); `None` without
/// PipeWire or its tools.
pub fn refresh_pipewire_clock() -> Option<PipeWireClock> {
    let out = std::process::Command::new("pw-metadata").args(["-n", "settings", "0"]).output().ok()?;
    let clock = PipeWireClock::parse(&String::from_utf8_lossy(&out.stdout));
    *PIPEWIRE_CLOCK.write().unwrap() = clock.clone();
    clock
}

/// PipeWire's clock settings as last read by [`refresh_pipewire_clock`].
pub fn pipewire_clock() -> Option<PipeWireClock> {
    PIPEWIRE_CLOCK.read().unwrap().clone()
}

/// Lists the output devices of the backend in use. Blocking: providers may
/// talk to the sound server.
pub fn devices(backend: OutputBackend) -> Vec<OutputDevice> {
    provider_devices(backend).into_iter().map(|(device, _)| device).collect()
}

fn provider_devices(backend: OutputBackend) -> Vec<(OutputDevice, gst::Device)> {
    let Some(provider) = provider(backend) else {
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

/// The device provider for `backend`.
///
/// PipeWire's provider cannot be probed again while a stream is open: every
/// PipeWire element in the process shares one connection, and a second probe
/// on it waits forever for the server's reply, taking the connection's lock
/// (and so playback) down with it. Starting the provider is such a probe,
/// and hangs the same way while a stream sits paused (a restored session).
/// Its provider is therefore started once, by [`create`] before the first
/// stream opens, and kept running; a running provider answers from the
/// devices it monitors, without probing, and follows hotplug.
fn provider(backend: OutputBackend) -> Option<gst::DeviceProvider> {
    static PIPEWIRE: Mutex<Option<gst::DeviceProvider>> = Mutex::new(None);

    let factory = backend.device_provider()?;
    if backend != OutputBackend::PipeWire {
        return gst::DeviceProviderFactory::by_name(factory);
    }
    let mut monitor = PIPEWIRE.lock().unwrap();
    if monitor.is_none() {
        let provider = gst::DeviceProviderFactory::by_name(factory)?;
        // Nobody reads the hotplug messages; the device list is what matters.
        provider.bus().set_sync_handler(|_, _| gst::BusSyncReply::Drop);
        if let Err(e) = provider.start() {
            log::warn!("could not monitor {factory}: {e}");
            return None;
        }
        *monitor = Some(provider);
    }
    monitor.clone()
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
    // PipeWire and PulseAudio say which ALSA card and device they drive
    // (`alsa.card` = "2", `alsa.device` = "0"); a plain ALSA device's own id
    // is the PCM name.
    let number = |keys: &[&str]| keys.iter().find_map(|k| prop(k)).and_then(|v| v.trim().parse::<u32>().ok());
    let pcm = match (number(&["alsa.card", "api.alsa.pcm.card"]), number(&["alsa.device", "api.alsa.pcm.device"])) {
        (Some(card), device) => Some(AlsaPcm { card, device: device.unwrap_or(0) }),
        (None, _) => parse_hw(&id),
    };
    Some(OutputDevice { id, name, detail, kind, pcm, exclusive: pcm.is_some() })
}

/// The card and device of an ALSA PCM name such as `hw:2,0` or `hw:2`.
fn parse_hw(name: &str) -> Option<AlsaPcm> {
    let rest = name.strip_prefix("hw:").or_else(|| name.strip_prefix("plughw:"))?;
    let mut parts = rest.split(',');
    let card = parts.next()?.trim().parse().ok()?;
    let device = parts.next().map_or(Some(0), |d| d.trim().parse().ok())?;
    Some(AlsaPcm { card, device })
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
        Self { sink, backend: OutputBackend::Auto, device: None, exclusive: false }
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
            // When PipeWire must resample (the device is locked to another
            // rate), use its best quality rather than the default of 4.
            .field("resample.quality", 10i32)
            .build();
        sink.set_property("stream-properties", props);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt in with `cargo test -- --ignored` on a machine running PipeWire.
    #[test]
    #[ignore = "needs a running PipeWire daemon"]
    fn lists_and_opens_pipewire_devices() {
        gst::init().unwrap();
        let devices = devices(OutputBackend::PipeWire);
        eprintln!("{devices:#?}");
        let first = devices.first().expect("PipeWire lists at least one sink");
        let (sink, found) = open_device(OutputBackend::PipeWire, &first.id).expect("the listed device opens");
        assert_eq!(&found, first);
        assert_eq!(sink.factory().unwrap().name(), "pipewiresink");
    }

    #[test]
    fn reads_alsa_pcm_names() {
        assert_eq!(parse_hw("hw:2,0"), Some(AlsaPcm { card: 2, device: 0 }));
        assert_eq!(parse_hw("hw:1"), Some(AlsaPcm { card: 1, device: 0 }));
        assert_eq!(parse_hw("plughw:3,1"), Some(AlsaPcm { card: 3, device: 1 }));
        assert_eq!(parse_hw("front:1"), None);
        assert_eq!(parse_hw("hw:x"), None);
    }

    #[test]
    fn reads_pipewire_clock_settings() {
        let metadata = "Found \"settings\" metadata 32\n\
            update: id:0 key:'log.level' value:'2' type:''\n\
            update: id:0 key:'clock.rate' value:'48000' type:''\n\
            update: id:0 key:'clock.allowed-rates' value:'[ 44100 48000 96000 ]' type:''\n\
            update: id:0 key:'clock.force-rate' value:'0' type:''\n";
        let clock = PipeWireClock::parse(metadata).unwrap();
        assert_eq!(clock, PipeWireClock { rate: 48_000, allowed_rates: vec![44_100, 48_000, 96_000], force_rate: 0 });
        assert_eq!((clock.rate_for(44_100), clock.rate_for(88_200)), (44_100, 48_000));
        let forced = PipeWireClock { force_rate: 96_000, ..clock };
        assert_eq!(forced.rate_for(44_100), 96_000);
        assert_eq!(PipeWireClock::parse("nothing here"), None);
    }

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
