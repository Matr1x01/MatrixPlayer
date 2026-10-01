//! What the log records about GStreamer: the plugins playback depends on
//! (their versions, and where they were loaded from: an AppImage's own, or
//! the system's), the formats it can decode, plugins that failed to load,
//! and GStreamer's own warnings and errors as they happen.
//!
//! After a system update, a plugin that no longer loads, or one from the
//! AppImage running against newer system libraries, is the usual reason
//! playback stops working.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;

/// Elements playback cannot do without.
const REQUIRED: [&str; 4] = ["playbin3", "audioconvert", "audioresample", "capsfilter"];
/// Output sinks, in the order they are tried; one is enough.
const OUTPUTS: [&str; 3] = ["pipewiresink", "pulsesink", "alsasink"];
/// Formats from the library's list, and elements that decode them (any).
const FORMATS: [(&str, &[&str]); 14] = [
    ("FLAC", &["flacdec"]),
    ("MP3", &["mpg123audiodec", "avdec_mp3float", "avdec_mp3"]),
    ("AAC", &["avdec_aac", "fdkaacdec", "faad"]),
    ("ALAC", &["avdec_alac"]),
    ("Opus", &["opusdec", "avdec_opus"]),
    ("Vorbis", &["vorbisdec", "avdec_vorbis"]),
    ("MP4/M4A", &["qtdemux"]),
    ("Ogg", &["oggdemux"]),
    ("Matroska", &["matroskademux"]),
    ("WAV", &["wavparse"]),
    ("AIFF", &["aiffparse"]),
    ("WavPack", &["wavpackdec", "avdec_wavpack"]),
    ("APE", &["avdec_ape"]),
    ("WMA", &["avdec_wmav2"]),
];

/// Logs GStreamer's version, the plugins playback uses, what it can decode,
/// and any plugin that failed to load. Call after `gstreamer::init()`.
pub fn report() {
    let registry = gst::Registry::get();
    let plugins = registry.plugins();
    let broken: Vec<_> = plugins.iter().filter(|p| p.plugin_flags().contains(gst::PluginFlags::BLACKLISTED)).collect();
    log::info!("{}: {} plugins", gst::version_string(), plugins.len() - broken.len());
    if !broken.is_empty() {
        let list: Vec<_> = broken
            .iter()
            .map(|p| p.filename().map(|f| f.display().to_string()).unwrap_or_else(|| p.plugin_name().to_string()))
            .collect();
        log::warn!(
            "GStreamer could not load {} plugins (after a system update, they may no longer match the libraries \
             they need): {}",
            list.len(),
            list.join(", ")
        );
    }

    // The plugins behind each element playback uses, once each.
    let mut seen: Vec<(String, Vec<&str>)> = Vec::new();
    for name in REQUIRED.iter().chain(&OUTPUTS) {
        match gst::ElementFactory::find(name).and_then(|f| f.plugin()) {
            Some(plugin) => {
                let about = format!(
                    "{} {} ({})",
                    plugin.plugin_name(),
                    plugin.version(),
                    plugin.filename().map(|f| f.display().to_string()).unwrap_or_else(|| "built in".into())
                );
                match seen.iter_mut().find(|(p, _)| *p == about) {
                    Some((_, elements)) => elements.push(name),
                    None => seen.push((about, vec![name])),
                }
            }
            None if REQUIRED.contains(name) => log::error!("GStreamer has no {name}: nothing can be played"),
            None => log::info!("GStreamer has no {name}"),
        }
    }
    for (plugin, elements) in seen {
        log::info!("plugin {plugin}: {}", elements.join(", "));
    }
    if !OUTPUTS.iter().any(|name| gst::ElementFactory::find(name).is_some()) {
        log::error!("GStreamer has no PipeWire, PulseAudio or ALSA output: nothing can be heard");
    }

    let (decodable, missing): (Vec<_>, Vec<_>) =
        FORMATS.iter().partition(|(_, elements)| elements.iter().any(|e| gst::ElementFactory::find(e).is_some()));
    let names = |formats: Vec<&(&str, &[&str])>| formats.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ");
    if missing.is_empty() {
        log::info!("decodes {}", names(decodable));
    } else {
        log::info!("decodes {}; not {}", names(decodable), names(missing));
    }
}

/// Most GStreamer messages logged in [`WINDOW`]; past that they are
/// counted, so a stream of warnings cannot flood the log (or the threads
/// that emit them, which include the audio thread).
const BUDGET: u32 = 20;
const WINDOW: Duration = Duration::from_secs(10);

struct Budget {
    since: Option<Instant>,
    used: u32,
    dropped: u32,
}

static BUDGET_STATE: Mutex<Budget> = Mutex::new(Budget { since: None, used: 0, dropped: 0 });

/// Whether a message may be logged now; also reports what was dropped in
/// the last window.
fn spend() -> bool {
    let mut budget = BUDGET_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if budget.since.is_none_or(|since| now - since > WINDOW) {
        if budget.dropped > 0 {
            log::warn!(target: "gst", "{} more GStreamer messages were not logged", budget.dropped);
        }
        *budget = Budget { since: Some(now), used: 0, dropped: 0 };
    }
    if budget.used < BUDGET {
        budget.used += 1;
        true
    } else {
        budget.dropped += 1;
        false
    }
}

/// Sends GStreamer's warnings and errors (which it otherwise keeps to
/// itself) to the log. With `GST_DEBUG` set, GStreamer prints its own
/// output as asked instead, and standard error is logged anyway.
pub fn forward_log() {
    if std::env::var_os("GST_DEBUG").is_some() {
        return;
    }
    gst::log::remove_default_log_function();
    gst::log::add_log_function(|category, level, _file, _function, _line, object, message| {
        let level = match level {
            gst::DebugLevel::Error => log::Level::Error,
            gst::DebugLevel::Warning => log::Level::Warn,
            _ => return,
        };
        if !log::log_enabled!(target: "gst", level) || !spend() {
            return;
        }
        let text = message.get();
        let text = text.as_deref().map(|t| t.as_str()).unwrap_or("");
        // Echoes of the warnings and errors elements post on the bus, which
        // the engine logs itself.
        if text.starts_with("warning: ") || text.starts_with("error: ") {
            return;
        }
        match object {
            Some(object) => log::log!(target: "gst", level, "{} {object}: {text}", category.name()),
            None => log::log!(target: "gst", level, "{}: {text}", category.name()),
        }
    });
    gst::log::set_default_threshold(gst::DebugLevel::Warning);
    gst::log::set_active(true);
}
