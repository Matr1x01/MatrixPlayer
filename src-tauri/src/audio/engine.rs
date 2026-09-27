use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_audio as gst_audio;
use tokio::sync::mpsc::UnboundedSender;

use super::{EngineError, EngineErrorKind, EngineEvent, EngineMessage, EngineState, StreamFormat, output};
use crate::dsp::analysis::Analyzer;
use crate::dsp::{DspChain, DspControl};
use crate::error::{AppError, AppResult};

/// How often position is reported while playing.
const POSITION_INTERVAL: Duration = Duration::from_millis(200);
/// Visualizer frame interval while something watches the analysis (~30 fps).
const ANALYSIS_INTERVAL: Duration = Duration::from_millis(33);

/// State shared between the engine, its position ticker and GStreamer's
/// streaming threads (which run the bus sync handler and `about-to-finish`).
#[derive(Default)]
struct Shared {
    /// Incremented every time the pipeline is reset. Every event carries the
    /// epoch it was produced in, so events still queued from a stream that
    /// was replaced can be recognised and dropped by the receiver.
    epoch: AtomicU64,
    /// URI to continue with when the current stream is about to end.
    next_uri: Mutex<Option<String>>,
    /// Streams handed to playbin that have not started yet, oldest first,
    /// flagged `true` when queued gaplessly. Each STREAM_START consumes the
    /// front entry. A queue rather than a single slot is needed because a
    /// short track can reach `about-to-finish` before its own STREAM_START.
    starting: Mutex<VecDeque<(String, bool)>>,
    /// The stream that started most recently.
    current_uri: Mutex<Option<String>>,
    /// The last error posted on the bus, so a load that fails synchronously
    /// can report why (file not found, not audio…) instead of only that the
    /// state change failed.
    last_error: Mutex<Option<EngineError>>,
    playing: AtomicBool,
    shutdown: AtomicBool,
}

impl Shared {
    fn send(&self, events: &UnboundedSender<EngineMessage>, event: EngineEvent) {
        let _ = events.send(EngineMessage { epoch: self.epoch.load(Ordering::SeqCst), event });
    }
}

pub struct AudioEngine {
    playbin: gst::Element,
    dsp: DspChain,
    sink: gst::Element,
    backend: output::OutputBackend,
    device: Option<output::OutputDevice>,
    events: UnboundedSender<EngineMessage>,
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl AudioEngine {
    pub fn new(events: UnboundedSender<EngineMessage>, out: output::Output) -> AppResult<Self> {
        let playbin = gst::ElementFactory::make("playbin3")
            .name("matrixplayer-player")
            .build()
            .or_else(|_| gst::ElementFactory::make("playbin").name("matrixplayer-player").build())
            .map_err(|_| AppError::Audio("GStreamer playbin is not installed".into()))?;
        // Audio only: never build video or subtitle branches, even for files
        // that carry cover art as a video stream.
        playbin.set_property_from_str("flags", "audio");

        let shared = Arc::new(Shared::default());
        let dsp = DspChain::build()?;
        watch_caps(&dsp.input_pad(), &shared, events.clone(), EngineEvent::DecodedFormat);
        attach_sink(&dsp, &out.sink, &shared, &events)?;
        playbin.set_property("audio-sink", dsp.element());

        connect_gapless(&playbin, &shared);
        install_bus_handler(&playbin, &shared, events.clone());
        let threads = vec![
            spawn_position_ticker(&playbin, &shared, events.clone())?,
            spawn_analyzer(&playbin, &shared, dsp.control())?,
        ];

        Ok(Self { playbin, dsp, sink: out.sink, backend: out.backend, device: out.device, events, shared, threads })
    }

    pub fn backend(&self) -> output::OutputBackend {
        self.backend
    }

    /// The device chosen for the output, `None` when following the default.
    pub fn device(&self) -> Option<&output::OutputDevice> {
        self.device.as_ref()
    }

    /// The output's buffer size, for the signal path readout.
    pub fn buffer_ms(&self) -> Option<u32> {
        output::Output::buffer_ms(&self.sink)
    }

    pub fn dsp(&self) -> Arc<DspControl> {
        self.dsp.control()
    }

    /// Replaces the output sink. The pipeline is reset, so the caller
    /// reloads whatever was playing.
    pub fn set_output(&mut self, out: output::Output) -> AppResult<()> {
        self.reset()?;
        attach_sink(&self.dsp, &out.sink, &self.shared, &self.events)?;
        self.sink = out.sink;
        self.backend = out.backend;
        self.device = out.device;
        Ok(())
    }

    /// Pipeline running time now (ns): what is audible, since the clock
    /// comes from the audio output.
    pub fn running_time(&self) -> Option<u64> {
        self.playbin.current_running_time().map(gst::ClockTime::nseconds)
    }

    /// The current pipeline epoch. Events from older epochs are stale.
    pub fn epoch(&self) -> u64 {
        self.shared.epoch.load(Ordering::SeqCst)
    }

    /// Replaces the current stream with `uri`, starting playback if `play`.
    ///
    /// `next` is installed before the pipeline starts so that even a very
    /// short track cannot reach `about-to-finish` without its continuation.
    ///
    /// When the pipeline refuses to start the stream, the error says why
    /// (from the bus, when GStreamer posted a reason), and the pipeline is
    /// brought all the way down so the next load starts clean.
    pub fn load(&self, uri: &str, next: Option<String>, play: bool) -> Result<(), EngineError> {
        let target = if play { gst::State::Playing } else { gst::State::Paused };
        self.reset().map_err(|e| EngineError {
            kind: EngineErrorKind::Other,
            message: e.to_string(),
            detail: None,
            uri: Some(uri.to_owned()),
        })?;
        *self.shared.next_uri.lock().unwrap() = next;
        self.start(uri);
        if self.playbin.set_state(target).is_ok() {
            return Ok(());
        }
        // After a stream fails to start, playbin3 (1.24) spends the next
        // attempt on the source that failed, whatever URI was set since, and
        // only the attempt after that opens the new one. So a failure here
        // may be the previous file's: try once more from NULL, with what
        // that attempt posted marked stale.
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        let _ = self.playbin.set_state(gst::State::Null);
        self.start(uri);
        if self.playbin.set_state(target).is_ok() {
            return Ok(());
        }
        let posted = self.shared.last_error.lock().unwrap().take();
        self.recover();
        let mut error = posted.unwrap_or_else(|| EngineError {
            kind: EngineErrorKind::Other,
            message: "The file could not be opened".into(),
            detail: None,
            uri: None,
        });
        error.uri = Some(uri.to_owned());
        Err(error)
    }

    /// Hands `uri` to playbin as the stream to start.
    fn start(&self, uri: &str) {
        *self.shared.starting.lock().unwrap() = VecDeque::from([(uri.to_owned(), false)]);
        *self.shared.current_uri.lock().unwrap() = Some(uri.to_owned());
        self.shared.last_error.lock().unwrap().take();
        self.playbin.set_property("uri", uri);
    }

    /// Takes the pipeline down to NULL after a playback error, releasing the
    /// output and every element of the failed stream. Starts a new epoch, so
    /// anything the failed stream posted is ignored.
    pub fn recover(&self) {
        let _ = self.playbin.set_state(gst::State::Null);
        self.shared.playing.store(false, Ordering::SeqCst);
        self.shared.starting.lock().unwrap().clear();
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
    }

    pub fn play(&self) -> AppResult<()> {
        self.playbin.set_state(gst::State::Playing)?;
        Ok(())
    }

    pub fn pause(&self) -> AppResult<()> {
        self.playbin.set_state(gst::State::Paused)?;
        Ok(())
    }

    /// Stops playback and releases the output device.
    pub fn stop(&self) -> AppResult<()> {
        self.reset()?;
        self.shared.starting.lock().unwrap().clear();
        Ok(())
    }

    /// Brings the pipeline to READY and starts a new epoch.
    ///
    /// Going to READY joins every streaming thread, so once it returns the
    /// old stream can post nothing more; bumping the epoch afterwards marks
    /// everything it did post as stale. Callers treat the engine as READY.
    fn reset(&self) -> AppResult<()> {
        self.playbin.set_state(gst::State::Ready)?;
        self.shared.playing.store(false, Ordering::SeqCst);
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub fn seek(&self, position: Duration) -> AppResult<()> {
        let target = gst::ClockTime::from_nseconds(u64::try_from(position.as_nanos()).unwrap_or(u64::MAX));
        self.playbin
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE, target)
            .map_err(|_| AppError::Audio("This stream cannot seek right now".into()))
    }

    /// Sets the URI to continue with, gaplessly, when the current stream ends.
    pub fn set_next(&self, uri: Option<String>) {
        *self.shared.next_uri.lock().unwrap() = uri;
    }

    pub fn set_volume(&self, linear: f64) {
        self.dsp.set_volume(linear);
    }

    pub fn set_muted(&self, muted: bool) {
        self.dsp.set_muted(muted);
    }

    pub fn position(&self) -> Option<Duration> {
        self.playbin.query_position::<gst::ClockTime>().map(Duration::from)
    }

    pub fn duration(&self) -> Option<Duration> {
        self.playbin.query_duration::<gst::ClockTime>().map(Duration::from)
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        let _ = self.playbin.set_state(gst::State::Null);
        if let Some(bus) = self.playbin.bus() {
            bus.unset_sync_handler();
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// Gapless playback: when playbin has fed the last data of the current
/// stream to its decoders it emits `about-to-finish` on a streaming thread.
/// Setting `uri` inside the handler queues the next stream behind the
/// current one in the same pipeline, so no silence is inserted between them.
/// The switch becomes audible at the next STREAM_START message.
fn connect_gapless(playbin: &gst::Element, shared: &Arc<Shared>) {
    let shared = shared.clone();
    playbin.connect("about-to-finish", false, move |args| {
        let playbin = args[0].get::<gst::Element>().expect("about-to-finish carries playbin");
        if let Some(next) = shared.next_uri.lock().unwrap().take() {
            // A file that has gone since it was queued would fail inside the
            // running pipeline; let the current stream end instead, and the
            // player reports and skips the missing file on its own.
            if uri_file_is_missing(&next) {
                log::debug!("gapless: not queueing missing {next}");
                return None;
            }
            log::debug!("gapless: queueing {next}");
            playbin.set_property("uri", &next);
            shared.starting.lock().unwrap().push_back((next, true));
        }
        None
    });
}

/// Whether `uri` names a local file that does not exist. Other URIs are
/// left for GStreamer to judge.
fn uri_file_is_missing(uri: &str) -> bool {
    gst::glib::filename_from_uri(uri).is_ok_and(|(path, _)| !path.exists())
}

/// Handles bus messages synchronously on the thread that posts them.
///
/// Doing this at post time, rather than popping messages on a worker, is
/// what makes the epoch tag trustworthy: a message is tagged with the epoch
/// of the stream that produced it, never with a later one.
fn install_bus_handler(playbin: &gst::Element, shared: &Arc<Shared>, events: UnboundedSender<EngineMessage>) {
    let bus = playbin.bus().expect("playbin has a bus");
    let weak = playbin.downgrade();
    let shared = shared.clone();
    bus.set_sync_handler(move |_, msg| {
        if let Some(playbin) = weak.upgrade()
            && let Some(event) = translate(msg, &playbin, &shared)
        {
            shared.send(&events, event);
        }
        gst::BusSyncReply::Drop
    });
}

/// Makes `sink` the DSP chain's output and reports the formats it receives.
fn attach_sink(
    dsp: &DspChain,
    sink: &gst::Element,
    shared: &Arc<Shared>,
    events: &UnboundedSender<EngineMessage>,
) -> AppResult<()> {
    let pad = sink.static_pad("sink").expect("audio sinks have a sink pad");
    watch_caps(&pad, shared, events.clone(), EngineEvent::OutputFormat);
    dsp.set_sink(sink)
}

/// Turns the DSP chain's analysis ring into visualizer frames while
/// something is watching and audio is playing.
fn spawn_analyzer(playbin: &gst::Element, shared: &Arc<Shared>, control: Arc<DspControl>) -> AppResult<JoinHandle<()>> {
    let weak = playbin.downgrade();
    let shared = shared.clone();
    std::thread::Builder::new()
        .name("matrixplayer-analysis".into())
        .spawn(move || {
            let mut analyzer = Analyzer::default();
            while !shared.shutdown.load(Ordering::SeqCst) {
                std::thread::sleep(ANALYSIS_INTERVAL);
                if !control.analysis_watched() || !shared.playing.load(Ordering::SeqCst) {
                    continue;
                }
                let Some(playbin) = weak.upgrade() else { break };
                let Some(now) = playbin.current_running_time() else { continue };
                drop(playbin);
                if let Some(frame) = analyzer.analyze(control.tap(), now.nseconds()) {
                    control.publish_analysis(frame);
                }
            }
        })
        .map_err(AppError::Io)
}

fn spawn_position_ticker(
    playbin: &gst::Element,
    shared: &Arc<Shared>,
    events: UnboundedSender<EngineMessage>,
) -> AppResult<JoinHandle<()>> {
    let weak = playbin.downgrade();
    let shared = shared.clone();
    std::thread::Builder::new()
        .name("matrixplayer-position".into())
        .spawn(move || {
            while !shared.shutdown.load(Ordering::SeqCst) && !events.is_closed() {
                std::thread::sleep(POSITION_INTERVAL);
                let Some(playbin) = weak.upgrade() else { break };
                if !shared.playing.load(Ordering::SeqCst) {
                    continue;
                }
                // Read the epoch before querying so a reset in between marks
                // this report stale instead of attributing it to the new stream.
                let epoch = shared.epoch.load(Ordering::SeqCst);
                if let Some(position) = playbin.query_position::<gst::ClockTime>() {
                    let duration = playbin.query_duration::<gst::ClockTime>().map(Duration::from);
                    let event = EngineEvent::Position { position: position.into(), duration };
                    let _ = events.send(EngineMessage { epoch, event });
                }
            }
        })
        .map_err(AppError::Io)
}

/// Turns a bus message into an engine event. Runs on the posting thread, so
/// it must not query the pipeline (that can deadlock on a streaming thread).
fn translate(msg: &gst::Message, playbin: &gst::Element, shared: &Shared) -> Option<EngineEvent> {
    use gst::MessageView as M;
    let from_playbin = msg.src().is_some_and(|s| s == playbin.upcast_ref::<gst::Object>());
    match msg.view() {
        M::Eos(_) => Some(EngineEvent::EndOfStream),
        M::Error(err) => {
            // The newest queued stream is the one being decoded.
            let queued = shared.starting.lock().unwrap().back().map(|(uri, _)| uri.clone());
            let uri = queued.or_else(|| shared.current_uri.lock().unwrap().clone());
            let from_output = msg.src().is_some_and(|s| s.name().starts_with("matrixplayer-output"));
            let (kind, message) = classify(&err.error(), from_output);
            let detail = err.debug().map(|d| d.to_string());
            log::error!("playback error: {} ({:?})", err.error(), detail);
            let error = EngineError { kind, message, detail, uri };
            // Keep the first, most specific reason: GStreamer follows a
            // failure with generic "state change failed" errors.
            let mut last = shared.last_error.lock().unwrap();
            if last.as_ref().is_none_or(|e| e.kind == EngineErrorKind::Other) {
                *last = Some(error.clone());
            }
            Some(EngineEvent::Error(error))
        }
        M::Warning(w) => {
            // playsink warns that it has no volume element: expected, since
            // volume is applied in the DSP chain instead.
            let expected = w.error().kind::<gst::StreamError>() == Some(gst::StreamError::NotImplemented)
                && msg.src().is_some_and(|s| s.name().starts_with("playsink"));
            if expected {
                log::debug!("gstreamer: {}", w.error());
            } else {
                log::warn!("gstreamer warning: {} ({:?})", w.error(), w.debug());
            }
            None
        }
        M::StateChanged(sc) if from_playbin => {
            let state = match sc.current() {
                gst::State::Playing => EngineState::Playing,
                gst::State::Paused => EngineState::Paused,
                gst::State::Ready => EngineState::Ready,
                _ => EngineState::Null,
            };
            shared.playing.store(state == EngineState::Playing, Ordering::SeqCst);
            Some(EngineEvent::StateChanged(state))
        }
        M::StreamStart(_) => {
            let (uri, gapless) = shared.starting.lock().unwrap().pop_front()?;
            *shared.current_uri.lock().unwrap() = Some(uri.clone());
            Some(EngineEvent::StreamStarted { uri, gapless })
        }
        M::AsyncDone(_) => Some(EngineEvent::AsyncDone),
        M::DurationChanged(_) => Some(EngineEvent::DurationChanged),
        _ => None,
    }
}

/// Turns a GStreamer error into something a listener can act on.
fn classify(err: &gst::glib::Error, from_output: bool) -> (EngineErrorKind, String) {
    use EngineErrorKind as K;
    if from_output {
        return (K::OutputUnavailable, "The audio output could not be opened".into());
    }
    if let Some(e) = err.kind::<gst::ResourceError>() {
        return match e {
            gst::ResourceError::NotFound => (K::NotFound, "The file could not be found".into()),
            gst::ResourceError::OpenRead | gst::ResourceError::Read | gst::ResourceError::NotAuthorized => {
                (K::Unreadable, "The file could not be read".into())
            }
            _ => (K::Other, err.message().to_owned()),
        };
    }
    if let Some(e) = err.kind::<gst::StreamError>() {
        return match e {
            gst::StreamError::CodecNotFound => (K::UnsupportedFormat, "No decoder is installed for this format".into()),
            gst::StreamError::TypeNotFound => {
                (K::UnsupportedFormat, "The file is not a recognised audio format".into())
            }
            _ => (K::Corrupt, "The file is damaged or is not valid audio".into()),
        };
    }
    if err.kind::<gst::CoreError>() == Some(gst::CoreError::MissingPlugin) {
        return (K::UnsupportedFormat, "A GStreamer plugin needed for this file is missing".into());
    }
    (K::Other, err.message().to_owned())
}

/// Reports every caps event passing `pad` as a [`StreamFormat`].
fn watch_caps(
    pad: &gst::Pad,
    shared: &Arc<Shared>,
    events: UnboundedSender<EngineMessage>,
    wrap: fn(StreamFormat) -> EngineEvent,
) {
    let shared = shared.clone();
    pad.add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, move |_, info| {
        if let Some(gst::EventView::Caps(caps)) = info.event().map(|e| e.view())
            && let Ok(audio) = gst_audio::AudioInfo::from_caps(caps.caps())
        {
            let format = StreamFormat {
                sample_format: audio.format().to_str().to_string(),
                bits: audio.depth(),
                float: audio.format_info().is_float(),
                sample_rate: audio.rate(),
                channels: audio.channels(),
            };
            shared.send(&events, wrap(format));
        }
        gst::PadProbeReturn::Ok
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::path_to_uri;
    use crate::test_support::{write_flac, write_flac_tags};
    use std::time::Instant;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    /// Collects events until `done` matches one, failing after `timeout`.
    fn collect_until(
        rx: &mut UnboundedReceiver<EngineMessage>,
        done: impl Fn(&EngineEvent) -> bool,
    ) -> Vec<EngineEvent> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut events = Vec::new();
        loop {
            match rx.try_recv().map(|m| m.event) {
                Ok(event) => {
                    let finished = done(&event);
                    events.push(event);
                    if finished {
                        return events;
                    }
                }
                Err(_) if Instant::now() > deadline => panic!("timed out; events so far: {events:#?}"),
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    #[test]
    fn plays_into_the_preloaded_next_stream_without_a_gap() {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.flac"), dir.path().join("b.flac"));
        write_flac(&a, 0.5, "A");
        write_flac(&b, 0.5, "B");
        let (ua, ub) = (path_to_uri(&a).unwrap(), path_to_uri(&b).unwrap());

        let (tx, mut rx) = unbounded_channel();
        let engine = AudioEngine::new(tx, output::Output::fake(true)).unwrap();
        engine.load(&ua, Some(ub.clone()), true).unwrap();
        let events = collect_until(&mut rx, |e| matches!(e, EngineEvent::EndOfStream));

        let starts: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::StreamStarted { uri, gapless } => Some((uri.clone(), *gapless)),
                _ => None,
            })
            .collect();
        assert_eq!(starts, [(ua, false), (ub, true)], "one gapless switch, then end of stream");
        assert!(events.iter().any(|e| matches!(e, EngineEvent::StateChanged(EngineState::Playing))));
        assert!(events.iter().any(|e| matches!(
            e,
            EngineEvent::DecodedFormat(f) if f.sample_rate == 44_100 && f.channels == 2 && f.bits == 16 && !f.float
        )));
        assert!(events.iter().any(|e| matches!(e, EngineEvent::OutputFormat(_))));
        assert!(events.iter().any(|e| matches!(e, EngineEvent::Position { .. })), "position is reported while playing");
    }

    #[test]
    fn reports_a_readable_error_for_files_that_are_not_audio() {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("broken.flac");
        std::fs::write(&bad, b"this is not a flac file at all, just some text").unwrap();
        let uri = path_to_uri(&bad).unwrap();

        let (tx, mut rx) = unbounded_channel();
        let engine = AudioEngine::new(tx, output::Output::fake(true)).unwrap();
        engine.load(&uri, None, true).unwrap();
        let events = collect_until(&mut rx, |e| matches!(e, EngineEvent::Error(_)));
        let Some(EngineEvent::Error(err)) = events.last() else { unreachable!() };
        assert!(matches!(err.kind, EngineErrorKind::UnsupportedFormat | EngineErrorKind::Corrupt), "{err:?}");
        assert_eq!(err.uri.as_deref(), Some(uri.as_str()));
    }

    #[test]
    fn applies_replay_gain_per_stream_across_a_gapless_join() {
        use crate::dsp::processor::RgSource;
        use crate::dsp::settings::DspSettings;

        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.flac"), dir.path().join("b.flac"));
        write_flac_tags(&a, 0.5, "title=\"A\",replaygain-album-gain=(double)-6.5,replaygain-album-peak=(double)0.2");
        write_flac(&b, 0.5, "B");

        let (tx, mut rx) = unbounded_channel();
        let engine = AudioEngine::new(tx, output::Output::fake(false)).unwrap();
        let dsp = engine.dsp();
        dsp.apply(&DspSettings::default());
        let mut status = dsp.subscribe_status();
        engine.load(&path_to_uri(&a).unwrap(), Some(path_to_uri(&b).unwrap()), true).unwrap();
        collect_until(&mut rx, |e| matches!(e, EngineEvent::EndOfStream));

        let mut seen = Vec::new();
        while let Ok(s) = status.try_recv() {
            if seen.last().map(|(src, _)| *src) != Some(s.replay_gain_source) {
                seen.push((s.replay_gain_source, (s.replay_gain_db * 10.0).round() / 10.0));
            }
        }
        assert!(seen.contains(&(RgSource::Album, -6.5)), "{seen:?}");
        assert_eq!(seen.last(), Some(&(RgSource::Untagged, 0.0)), "the untagged track plays as mastered: {seen:?}");
    }

    #[test]
    fn publishes_analysis_frames_while_watched() {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.flac");
        write_flac(&a, 1.5, "A");

        let (tx, mut rx) = unbounded_channel();
        let engine = AudioEngine::new(tx, output::Output::fake(true)).unwrap();
        let dsp = engine.dsp();
        dsp.set_analysis(true);
        let mut frames = dsp.subscribe_analysis();
        engine.load(&path_to_uri(&a).unwrap(), None, true).unwrap();
        collect_until(&mut rx, |e| matches!(e, EngineEvent::EndOfStream));

        let mut loudest = None::<crate::dsp::analysis::AnalysisFrame>;
        let mut count = 0;
        loop {
            let frame = match frames.try_recv() {
                Ok(frame) => frame,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    count += n as usize;
                    continue;
                }
                Err(_) => break,
            };
            count += 1;
            if loudest.as_ref().is_none_or(|l| frame.peak_db[0] > l.peak_db[0]) {
                loudest = Some(frame);
            }
        }
        assert!(count >= 10, "only {count} frames in 1.5 s");
        let frame = loudest.unwrap();
        // audiotestsrc volume 0.2 → −14 dBFS; its 440 Hz tone is the loudest band.
        assert!((frame.peak_db[0] + 14.0).abs() < 1.0, "peak {}", frame.peak_db[0]);
        let band = frame.bands.iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1)).unwrap().0;
        assert!((18..=26).contains(&band), "loudest band {band}");
    }

    /// With default settings and the volume at 100%, an untagged file reaches
    /// the output at the level it was mastered at, as in other players.
    #[test]
    fn default_settings_play_untagged_files_at_unity_gain() {
        use crate::dsp::settings::DspSettings;

        let db = output_peak_db(&DspSettings::default());
        // The fixture is a sine at 0.2 of full scale (−14 dBFS).
        assert!((db - 20.0 * 0.2f64.log10()).abs() < 0.2, "output peak {db:.2} dBFS");
    }

    /// An EQ boost at the tone's frequency raises what reaches the output
    /// by that much (auto preamp off, so nothing else moves).
    #[test]
    fn an_eq_boost_reaches_the_output() {
        use crate::dsp::settings::{DspSettings, EqBand, EqCurve};

        let mut settings = DspSettings { eq_enabled: true, auto_preamp: false, ..DspSettings::default() };
        settings.a = EqCurve { parametric: vec![EqBand::peak(440.0, 6.0, 1.0)], ..EqCurve::default() };
        let db = output_peak_db(&settings);
        assert!((db - (20.0 * 0.2f64.log10() + 6.0)).abs() < 0.3, "output peak {db:.2} dBFS");
    }

    /// Plays a 440 Hz fixture at 0.2 of full scale through the whole engine
    /// with `settings` and the volume at 100%; returns the output peak (dBFS).
    fn output_peak_db(settings: &crate::dsp::settings::DspSettings) -> f64 {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.flac");
        write_flac(&a, 1.0, "A");
        let sink = gst::ElementFactory::make("fakesink")
            .name("matrixplayer-output")
            .property("sync", false)
            .property("signal-handoffs", true)
            .build()
            .unwrap();
        let peak = Arc::new(Mutex::new(0.0f64));
        let seen = peak.clone();
        sink.connect("handoff", false, move |args| {
            let buffer = args[1].get::<gst::Buffer>().ok()?;
            let map = buffer.map_readable().ok()?;
            let p = map
                .as_slice()
                .chunks_exact(8)
                .map(|b| f64::from_ne_bytes(b.try_into().unwrap()).abs())
                .fold(0.0, f64::max);
            let mut max = seen.lock().unwrap();
            *max = max.max(p);
            None
        });
        let (tx, mut rx) = unbounded_channel();
        let engine =
            AudioEngine::new(tx, output::Output { sink, backend: output::OutputBackend::Auto, device: None }).unwrap();
        engine.dsp().apply(settings);
        engine.set_volume(1.0);
        engine.load(&path_to_uri(&a).unwrap(), None, true).unwrap();
        collect_until(&mut rx, |e| matches!(e, EngineEvent::EndOfStream));
        20.0 * peak.lock().unwrap().log10()
    }

    /// Settings (EQ, fades) are heard only once processed audio reaches the
    /// speakers, so the processor must not run far ahead of the output.
    #[test]
    fn processing_runs_just_ahead_of_the_output() {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.flac");
        write_flac(&a, 3.0, "A");
        let (tx, _rx) = unbounded_channel();
        let engine = AudioEngine::new(tx, output::Output::fake(true)).unwrap();
        engine.load(&path_to_uri(&a).unwrap(), None, true).unwrap();
        let mut leads = Vec::new();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(300));
            if let Some(now) = engine.running_time() {
                leads.push(engine.dsp().processed_running_time().saturating_sub(now) / 1_000_000);
            }
        }
        let worst = leads.iter().copied().max().expect("running time is known while playing");
        assert!(worst < 400, "processing ran {worst} ms ahead of the output ({leads:?})");
    }

    #[test]
    fn seeks_while_paused() {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.flac");
        write_flac(&a, 3.0, "A");

        let (tx, mut rx) = unbounded_channel();
        let engine = AudioEngine::new(tx, output::Output::fake(true)).unwrap();
        engine.load(&path_to_uri(&a).unwrap(), None, false).unwrap();
        collect_until(&mut rx, |e| matches!(e, EngineEvent::StateChanged(EngineState::Paused)));
        collect_until(&mut rx, |e| matches!(e, EngineEvent::AsyncDone));

        engine.seek(Duration::from_secs(2)).unwrap();
        collect_until(&mut rx, |e| matches!(e, EngineEvent::AsyncDone));
        let position = engine.position().unwrap();
        assert!((position.as_secs_f64() - 2.0).abs() < 0.05, "position after seek: {position:?}");
    }
}

/// Runs against the real system output, muted. Opt in with
/// `cargo test -- --ignored` on a machine with PipeWire or PulseAudio.
#[cfg(test)]
mod system_output_tests {
    use std::time::Instant;

    use tokio::sync::mpsc::unbounded_channel;

    use super::*;
    use crate::filesystem::path_to_uri;
    use crate::test_support::write_flac;

    #[test]
    #[ignore = "needs a running sound server; plays silence"]
    fn plays_gaplessly_through_the_system_output() {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.flac"), dir.path().join("b.flac"));
        write_flac(&a, 1.0, "A");
        write_flac(&b, 1.0, "B");

        let (tx, mut rx) = unbounded_channel();
        let engine = AudioEngine::new(tx, output::create(None).unwrap()).unwrap();
        engine.set_muted(true);
        let started = Instant::now();
        engine.load(&path_to_uri(&a).unwrap(), Some(path_to_uri(&b).unwrap()), true).unwrap();

        let mut gapless_at = None;
        let mut output = None;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match rx.try_recv().map(|m| m.event) {
                Ok(EngineEvent::StreamStarted { gapless: true, .. }) => gapless_at = Some(started.elapsed()),
                Ok(EngineEvent::OutputFormat(f)) => output = Some(f),
                Ok(EngineEvent::Error(e)) => panic!("playback error: {e:?}"),
                Ok(EngineEvent::EndOfStream) => break,
                Ok(_) => {}
                Err(_) if Instant::now() > deadline => panic!("timed out"),
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        let total = started.elapsed();
        let switch = gapless_at.expect("switched to the second stream without a reload");
        eprintln!("backend {:?}, output {output:?}, switch at {switch:?}, end at {total:?}", engine.backend());
        // Each file is 1 s long and played in real time.
        assert!(switch >= Duration::from_millis(800) && switch <= Duration::from_millis(1600), "switch at {switch:?}");
        assert!(total >= Duration::from_millis(1800) && total <= Duration::from_millis(3200), "ended at {total:?}");
    }

    /// Opt in with `cargo test -- --ignored` on a machine running PipeWire.
    #[test]
    #[ignore = "needs a running PipeWire daemon"]
    fn lists_and_switches_pipewire_devices_while_playing() {
        gst::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.flac");
        write_flac(&a, 6.0, "A");
        let uri = path_to_uri(&a).unwrap();

        let (tx, _rx) = unbounded_channel();
        let out = output::create(None).unwrap();
        assert_eq!(out.backend, output::OutputBackend::PipeWire);
        let mut engine = AudioEngine::new(tx, out).unwrap();
        engine.set_muted(true);
        engine.load(&uri, None, true).unwrap();
        std::thread::sleep(Duration::from_millis(500));

        // Listing twice while a stream is open used to hang for good.
        let devices = output::devices(output::OutputBackend::PipeWire);
        assert_eq!(output::devices(output::OutputBackend::PipeWire), devices);
        for device in &devices {
            let out = output::create(Some(&device.id)).unwrap();
            assert_eq!(out.device.as_ref(), Some(device));
            engine.set_output(out).unwrap();
            engine.load(&uri, None, true).unwrap();
            std::thread::sleep(Duration::from_millis(500));
            assert!(engine.position().is_some_and(|p| p > Duration::ZERO), "{} plays", device.name);
        }
    }
}
