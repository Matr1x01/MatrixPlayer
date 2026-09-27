//! The native processing chain between the decoder and the audio output.
//!
//! The chain is playbin's `audio-sink`: a bin that ends in the real output
//! sink. It lives inside the playback pipeline and survives gapless track
//! changes. All processing runs on 64-bit float samples:
//!
//! ```text
//! playsink queue ─ audioconvert ─ F64 ─ processor ─ volume ─ audioconvert ─ output sink
//!                                        │ ReplayGain, preamp, EQ, balance,
//!                                        │ width, crossfeed, fades, limiter
//!                                        └─ analysis tap (spectrum, meters)
//! ```
//!
//! It sits *after* playsink's internal queue on purpose. As an `audio-filter`
//! it would run in front of that queue, about 1.3 s ahead of the speakers:
//! every setting would be heard over a second late, and a soft pause would
//! stop the output before its fade was ever heard. Here the processor is
//! ahead only by the output's own buffer.
//!
//! The processor is a buffer probe on the F64 stage, so it sees the stream's
//! caps, tags and flushes in order with the samples. Settings reach it
//! through [`DspControl`] without ever blocking the audio thread for long.

pub mod analysis;
pub mod biquad;
pub mod processor;
pub mod service;
pub mod settings;

use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_audio as gst_audio;
use tokio::sync::broadcast;

use self::analysis::AnalysisFrame;
use self::processor::{DspStatus, Params, Processor, Shared};
use self::settings::DspSettings;
use crate::error::AppResult;

pub struct DspChain {
    bin: gst::Bin,
    volume: gst::Element,
    to_output: gst::Element,
    sink: Mutex<Option<gst::Element>>,
    control: Arc<DspControl>,
}

impl DspChain {
    pub fn build() -> AppResult<Self> {
        let bin = gst::Bin::builder().name("matrixplayer-dsp").build();

        let to_float = gst::ElementFactory::make("audioconvert").name("dsp-in").build()?;
        let float_caps = gst::ElementFactory::make("capsfilter")
            .name("dsp-f64")
            .property(
                "caps",
                gst_audio::AudioCapsBuilder::new_interleaved().format(gst_audio::AudioFormat::F64le).build(),
            )
            .build()?;
        let volume = gst::ElementFactory::make("volume").name("dsp-volume").build()?;
        // Converts back to whatever the output accepts. Dither is applied only
        // if asked for, and only matters when the device takes fewer bits.
        let to_output = gst::ElementFactory::make("audioconvert").name("dsp-out").build()?;
        to_output.set_property_from_str("dithering", "none");

        let chain = [&to_float, &float_caps, &volume, &to_output];
        bin.add_many(chain)?;
        gst::Element::link_many(chain)?;

        let sink_pad = to_float.static_pad("sink").expect("audioconvert has a sink pad");
        bin.add_pad(&gst::GhostPad::builder_with_target(&sink_pad)?.name("sink").build())?;

        let shared = Arc::new(Shared::new());
        install_processor(&float_caps.static_pad("src").expect("capsfilter has a src pad"), shared.clone());
        let (analysis, _) = broadcast::channel(8);
        let (status, _) = broadcast::channel(32);
        {
            let status = status.clone();
            *shared.on_status.lock().unwrap() = Some(Box::new(move |s| {
                let _ = status.send(s);
            }));
        }
        let control =
            Arc::new(DspControl { shared, to_output: to_output.clone(), analysis, status, settings: Mutex::default() });
        Ok(Self { bin, volume, to_output, sink: Mutex::new(None), control })
    }

    /// Makes `sink` the chain's output, replacing (and closing) the previous
    /// one. Only while the pipeline is READY or NULL.
    pub fn set_sink(&self, sink: &gst::Element) -> AppResult<()> {
        let mut current = self.sink.lock().unwrap();
        if let Some(old) = current.take() {
            self.to_output.unlink(&old);
            self.bin.remove(&old)?;
            let _ = old.set_state(gst::State::Null);
        }
        self.bin.add(sink)?;
        self.to_output.link(sink)?;
        sink.sync_state_with_parent()?;
        *current = Some(sink.clone());
        Ok(())
    }

    pub fn element(&self) -> &gst::Element {
        self.bin.upcast_ref()
    }

    /// The chain's input pad, where decoded stream caps arrive.
    pub fn input_pad(&self) -> gst::Pad {
        self.bin.static_pad("sink").expect("dsp bin has a sink ghost pad")
    }

    pub fn control(&self) -> Arc<DspControl> {
        self.control.clone()
    }

    /// Sets the playback gain as a linear amplitude factor (1.0 = unity).
    pub fn set_volume(&self, linear: f64) {
        self.volume.set_property("volume", linear.clamp(0.0, 1.0));
    }

    pub fn set_muted(&self, muted: bool) {
        self.volume.set_property("mute", muted);
    }
}

/// Runs the processor on every buffer and event passing `pad`.
fn install_processor(pad: &gst::Pad, shared: Arc<Shared>) {
    let processor = Mutex::new(Processor::new(shared));
    pad.add_probe(
        gst::PadProbeType::BUFFER | gst::PadProbeType::EVENT_DOWNSTREAM | gst::PadProbeType::EVENT_FLUSH,
        move |_, info| {
            let mut p = processor.lock().unwrap();
            if let Some(event) = info.event() {
                match event.view() {
                    gst::EventView::Caps(caps) => {
                        if let Ok(audio) = gst_audio::AudioInfo::from_caps(caps.caps()) {
                            p.set_format(audio.rate(), audio.channels());
                        }
                    }
                    gst::EventView::StreamStart(_) => p.stream_start(),
                    gst::EventView::Tag(tag) => p.tags(tag.tag()),
                    gst::EventView::Segment(segment) => p.segment(segment.segment()),
                    gst::EventView::FlushStop(_) => p.flush(),
                    _ => {}
                }
                return gst::PadProbeReturn::Ok;
            }
            let Some(buffer) = info.buffer_mut() else { return gst::PadProbeReturn::Ok };
            let pts = buffer.pts();
            let buffer = buffer.make_mut();
            let Ok(mut map) = buffer.map_writable() else { return gst::PadProbeReturn::Ok };
            let bytes = map.as_mut_slice();
            // F64 samples are 8-byte aligned in practice; fall back to a copy if
            // an allocator ever hands over an unaligned buffer.
            let (head, samples, tail) = unsafe { bytes.align_to_mut::<f64>() };
            if head.is_empty() && tail.is_empty() {
                p.process(samples, pts);
            } else {
                let mut copy: Vec<f64> =
                    bytes.chunks_exact(8).map(|c| f64::from_ne_bytes(c.try_into().expect("8 bytes"))).collect();
                p.process(&mut copy, pts);
                for (chunk, v) in bytes.chunks_exact_mut(8).zip(copy) {
                    chunk.copy_from_slice(&v.to_ne_bytes());
                }
            }
            gst::PadProbeReturn::Ok
        },
    );
}

/// The control side of the chain, shared with the player and commands.
pub struct DspControl {
    shared: Arc<Shared>,
    to_output: gst::Element,
    analysis: broadcast::Sender<AnalysisFrame>,
    status: broadcast::Sender<DspStatus>,
    settings: Mutex<DspSettings>,
}

impl DspControl {
    /// Applies validated settings to the running chain.
    pub fn apply(&self, settings: &DspSettings) {
        let active = settings.eq_active();
        self.shared.set_params(Params {
            filters: if active { settings.curve().filters() } else { Vec::new() },
            preamp_db: if active { settings.preamp_db } else { 0.0 },
            auto_preamp_allowance_db: (active && settings.auto_preamp).then_some(settings.auto_preamp_allowance_db),
            balance: settings.balance,
            width: settings.width,
            crossfeed: settings.crossfeed,
            replay_gain: settings.replay_gain,
            replay_gain_preamp_db: settings.replay_gain_preamp_db,
            replay_gain_fallback_db: settings.replay_gain_fallback_db,
            clipping_prevention: settings.clipping_prevention,
            soft_fades: settings.soft_fades,
        });
        self.to_output.set_property_from_str("dithering", if settings.dither { "tpdf" } else { "none" });
        *self.settings.lock().unwrap() = settings.clone();
    }

    pub fn settings(&self) -> DspSettings {
        self.settings.lock().unwrap().clone()
    }

    /// Whether shuffle is on, for "Smart" ReplayGain.
    pub fn set_shuffle(&self, shuffle: bool) {
        self.shared.set_shuffle(shuffle);
    }

    pub fn fade(&self, target: f64, seconds: f64) {
        self.shared.fade(target, seconds);
    }

    pub fn status(&self) -> DspStatus {
        *self.shared.status.lock().unwrap()
    }

    /// Running time (ns) of the newest processed sample.
    pub fn processed_running_time(&self) -> u64 {
        self.shared.processed_rt.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Starts or stops producing analysis frames.
    pub fn set_analysis(&self, on: bool) {
        self.shared.tap.set_watched(on);
    }

    pub fn analysis_watched(&self) -> bool {
        self.shared.tap.watched()
    }

    pub fn subscribe_analysis(&self) -> broadcast::Receiver<AnalysisFrame> {
        self.analysis.subscribe()
    }

    pub fn subscribe_status(&self) -> broadcast::Receiver<DspStatus> {
        self.status.subscribe()
    }

    pub(crate) fn tap(&self) -> &analysis::AnalysisTap {
        &self.shared.tap
    }

    pub(crate) fn publish_analysis(&self, frame: AnalysisFrame) {
        let _ = self.analysis.send(frame);
    }
}
