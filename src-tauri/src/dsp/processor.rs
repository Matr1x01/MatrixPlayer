//! The per-buffer processing that runs on GStreamer's streaming thread.
//!
//! ```text
//! F64 in ─ ReplayGain × preamp ─ EQ ─ balance ─ width ─ crossfeed
//!        ─ fade ─ look-ahead limiter ─ volume ─ F64 out
//! ```
//!
//! Stages that are neutral are skipped outright, so with everything off and
//! the volume at 100% the samples pass through untouched, full-scale peaks
//! included; they only come out 1.5 ms later, the limiter's constant
//! look-ahead. ReplayGain values come from the tag events
//! that travel with the audio, so at a gapless join the new track's gain
//! starts on exactly its first sample.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gstreamer as gst;
use serde::Serialize;

use super::analysis::AnalysisTap;
use super::crossfeed::{Crossfeed, CrossfeedPreset};
use super::eq::Equalizer;
use super::graphic::{GraphicCache, curve_slots};
use super::limiter::{CEILING, Limiter};
use super::settings::{AbSlot, EqCurve, ReplayGainMode};

/// Changes of static gain are spread over this long, to avoid zipper noise.
const GAIN_RAMP_S: f64 = 0.02;
/// Length of the soft fades on pause, resume and seek.
pub const FADE_S: f64 = 0.12;
/// Volume changes are smoothed with this time constant, so a moving slider
/// or a mute never steps the gain at a buffer boundary (zipper noise).
const VOLUME_SMOOTH_S: f64 = 0.01;
/// Closer than this, the smoothed volume lands exactly on its target
/// (−120 dB: far below anything audible), so unity and mute are exact.
const VOLUME_SNAP: f64 = 1e-6;

/// The EQ curve being heard.
#[derive(Debug, Clone, PartialEq)]
pub struct EqSpec {
    /// The compare slot it is in: switching slots crossfades.
    pub slot: AbSlot,
    pub curve: EqCurve,
}

/// Everything the audio thread needs, derived from the user's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// The EQ and tone controls; `None` while they are off or bypassed.
    pub eq: Option<EqSpec>,
    pub preamp_db: f64,
    /// With auto preamp on, the EQ boost allowed before the level is lowered
    /// by the rest of the boost.
    pub auto_preamp_allowance_db: Option<f64>,
    pub balance: f64,
    pub width: f64,
    /// The crossfeed preset, `None` while it is off.
    pub crossfeed: Option<CrossfeedPreset>,
    pub replay_gain: ReplayGainMode,
    pub replay_gain_preamp_db: f64,
    pub replay_gain_fallback_db: f64,
    pub clipping_prevention: bool,
    /// Fade in after seeks.
    pub soft_fades: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            eq: None,
            preamp_db: 0.0,
            auto_preamp_allowance_db: None,
            balance: 0.0,
            width: 1.0,
            crossfeed: None,
            replay_gain: ReplayGainMode::Off,
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            clipping_prevention: true,
            soft_fades: false,
        }
    }
}

/// ReplayGain values carried by the current stream's tags.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RgTags {
    pub track_gain: Option<f64>,
    pub track_peak: Option<f64>,
    pub album_gain: Option<f64>,
    pub album_peak: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RgSource {
    Off,
    Track,
    Album,
    /// ReplayGain is on but the file carries no gain tags.
    Untagged,
}

/// What the processor is doing right now, for the signal path readouts.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DspStatus {
    pub replay_gain_source: RgSource,
    /// Gain applied by ReplayGain (including its preamp and peak limiting).
    pub replay_gain_db: f64,
    /// The limiter caught overs recently.
    pub limiting: bool,
    pub sample_rate: u32,
    pub channels: u32,
}

impl Default for DspStatus {
    fn default() -> Self {
        Self { replay_gain_source: RgSource::Off, replay_gain_db: 0.0, limiting: false, sample_rate: 0, channels: 0 }
    }
}

pub type StatusListener = Box<dyn Fn(DspStatus) + Send>;

/// State shared between the control side and the audio thread.
pub struct Shared {
    params: Mutex<Params>,
    version: AtomicU64,
    shuffle: AtomicBool,
    fade: Mutex<Option<(f64, f64)>>,
    /// Playback volume as a linear gain (the bits of an `f64`); 0 when muted.
    volume: AtomicU64,
    /// The pipeline was stopped: the next buffer starts a stream from rest.
    restart: AtomicBool,
    pub status: Mutex<DspStatus>,
    /// Running time (ns) of the last processed sample, to tell how far
    /// processing runs ahead of what is audible.
    pub processed_rt: AtomicU64,
    pub tap: AnalysisTap,
    pub on_status: Mutex<Option<StatusListener>>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            params: Mutex::new(Params::default()),
            version: AtomicU64::new(1),
            shuffle: AtomicBool::new(false),
            fade: Mutex::new(None),
            volume: AtomicU64::new(1f64.to_bits()),
            restart: AtomicBool::new(false),
            status: Mutex::new(DspStatus::default()),
            processed_rt: AtomicU64::new(0),
            tap: AnalysisTap::default(),
            on_status: Mutex::new(None),
        }
    }

    pub fn set_params(&self, params: Params) {
        let mut current = self.params.lock().unwrap();
        if *current != params {
            *current = params;
            self.version.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        if self.shuffle.swap(shuffle, Ordering::SeqCst) != shuffle {
            self.version.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Ramps the output to `target` (0–1) over `seconds`.
    pub fn fade(&self, target: f64, seconds: f64) {
        *self.fade.lock().unwrap() = Some((target.clamp(0.0, 1.0), seconds.max(0.0)));
    }

    /// The playback volume as a linear gain (0 when muted). The processor
    /// glides to it.
    pub fn set_volume(&self, linear: f64) {
        self.volume.store(linear.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn volume(&self) -> f64 {
        f64::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// The pipeline was reset: whatever plays next starts from rest, at the
    /// current volume, with nothing left of the previous stream.
    pub fn restart(&self) {
        self.restart.store(true, Ordering::SeqCst);
    }
}

pub struct Processor {
    shared: Arc<Shared>,
    seen: u64,
    params: Params,
    rate: f64,
    channels: usize,
    eq: Equalizer,
    /// The graphic EQ's solver for the layout and rate in use.
    graphic: GraphicCache,
    rg: RgTags,
    /// Target static gain (ReplayGain × preamp) and the gain now, ramped.
    gain_target: f64,
    gain_now: f64,
    fade_now: f64,
    fade_target: f64,
    fade_step: f64,
    limiter: Limiter,
    /// Something upstream of the limiter can push peaks up (a boost, the
    /// EQ, a wider image); otherwise it only delays the samples.
    limiter_needed: bool,
    /// The volume heard now, gliding towards the shared target.
    volume_now: f64,
    volume_coeff: f64,
    /// Nothing processed yet: the first buffer takes the volume as set,
    /// rather than gliding to it from unity.
    fresh: bool,
    crossfeed: Crossfeed,
    segment: Option<gst::FormattedSegment<gst::ClockTime>>,
    status: DspStatus,
}

impl Processor {
    pub fn new(shared: Arc<Shared>) -> Self {
        let mut processor = Self {
            shared,
            seen: 0,
            params: Params::default(),
            rate: 0.0,
            channels: 0,
            eq: Equalizer::new(48_000.0, 2),
            graphic: GraphicCache::default(),
            rg: RgTags::default(),
            gain_target: 1.0,
            gain_now: 1.0,
            fade_now: 1.0,
            fade_target: 1.0,
            fade_step: 0.0,
            limiter: Limiter::new(48_000.0, 2),
            limiter_needed: false,
            volume_now: 1.0,
            volume_coeff: 1.0,
            fresh: true,
            crossfeed: Crossfeed::new(CrossfeedPreset::Default, 48_000.0),
            segment: None,
            status: DspStatus::default(),
        };
        processor.set_format(48_000, 2);
        processor
    }

    /// New stream format: filters are recomputed for the sample rate. The
    /// same format again (a gapless join announces it anew) changes
    /// nothing, so filter memories and the limiter's delay line carry over.
    pub fn set_format(&mut self, rate: u32, channels: u32) {
        let (rate_hz, channels_n) = (f64::from(rate.max(1)), channels.max(1) as usize);
        if rate_hz == self.rate && channels_n == self.channels {
            return;
        }
        self.rate = rate_hz;
        self.channels = channels_n;
        self.limiter.set_format(self.rate, self.channels);
        self.volume_coeff = 1.0 - (-1.0 / (VOLUME_SMOOTH_S * self.rate)).exp();
        self.tune_crossfeed();
        self.status.sample_rate = rate;
        self.status.channels = channels;
        self.eq.set_format(self.rate, self.channels);
        self.update_eq();
        self.update_gain();
    }

    /// A new stream began (a new track, possibly gaplessly): its tags will
    /// follow, so forget the previous track's ReplayGain values.
    pub fn stream_start(&mut self) {
        self.rg = RgTags::default();
        self.update_gain();
    }

    pub fn tags(&mut self, tags: &gst::TagListRef) {
        use gst::tags::{AlbumGain, AlbumPeak, TrackGain, TrackPeak};
        let before = self.rg;
        if let Some(v) = tags.get::<TrackGain>() {
            self.rg.track_gain = Some(v.get());
        }
        if let Some(v) = tags.get::<TrackPeak>() {
            self.rg.track_peak = Some(v.get());
        }
        if let Some(v) = tags.get::<AlbumGain>() {
            self.rg.album_gain = Some(v.get());
        }
        if let Some(v) = tags.get::<AlbumPeak>() {
            self.rg.album_peak = Some(v.get());
        }
        if self.rg != before {
            self.update_gain();
        }
    }

    pub fn segment(&mut self, segment: &gst::Segment) {
        self.segment = segment.downcast_ref::<gst::ClockTime>().cloned();
    }

    /// A flush (seek): filter memories hold the old position's signal.
    pub fn flush(&mut self) {
        self.clear_memories();
        self.volume_now = self.shared.volume();
        self.shared.tap.reset();
        self.sync_params();
        // Glides landed and a waiting curve came in at once: the boost and
        // whether the limiter is needed may have changed with them.
        self.update_gain();
        if self.params.soft_fades {
            // The new position starts from silence instead of a jump.
            self.fade_now = 0.0;
            self.fade_step = 1.0 / (FADE_S * self.rate);
        }
    }

    /// Processes interleaved F64 samples in place. `pts` is the buffer's
    /// timestamp, used to line the analysis up with what is audible.
    pub fn process(&mut self, samples: &mut [f64], pts: Option<gst::ClockTime>) {
        if self.shared.restart.swap(false, Ordering::SeqCst) {
            self.restart();
        }
        if std::mem::take(&mut self.fresh) {
            self.volume_now = self.shared.volume();
        }
        self.sync_params();
        self.sync_fade();
        let ch = self.channels;
        let frames = samples.len() / ch;
        if frames == 0 {
            return;
        }
        // A gain ramping down from a boost can still push this buffer over.
        let limiter_on = self.limiter_needed || self.gain_now > 1.0;

        // Static gain, ramped towards its target.
        let ramp_step = 1.0 / (GAIN_RAMP_S * self.rate);
        let gain_neutral = self.gain_now == 1.0 && self.gain_target == 1.0;
        if !gain_neutral {
            for frame in samples.chunks_exact_mut(ch) {
                if self.gain_now != self.gain_target {
                    let delta = self.gain_target - self.gain_now;
                    self.gain_now += delta.clamp(-ramp_step, ramp_step);
                    if (self.gain_target - self.gain_now).abs() < 1e-9 {
                        self.gain_now = self.gain_target;
                    }
                }
                for s in frame {
                    *s *= self.gain_now;
                }
            }
        }

        if self.eq.process(samples) {
            // Settled: the limiter may no longer be needed, and a change that
            // waited for a crossfade may have brought another boost.
            self.update_gain();
        }

        if ch == 2 {
            self.stereo(samples);
        }

        // Fade (soft pause, seek and resume).
        if self.fade_now != 1.0 || self.fade_target != 1.0 {
            for frame in samples.chunks_exact_mut(ch) {
                if self.fade_now != self.fade_target {
                    let delta = self.fade_target - self.fade_now;
                    self.fade_now += delta.clamp(-self.fade_step, self.fade_step);
                }
                for s in frame {
                    *s *= self.fade_now;
                }
            }
        }

        // Always through the delay line, so the latency never changes; it
        // only looks for peaks when something can have raised them, or while
        // it is still recovering.
        self.limiter.process(samples, limiter_on);
        self.apply_volume(samples);
        self.flush_denormals();

        let rt_end = pts.and_then(|pts| {
            let duration = gst::ClockTime::from_nseconds((frames as f64 / self.rate * 1e9) as u64);
            self.segment.as_ref().and_then(|seg| seg.to_running_time(pts + duration))
        });
        if let Some(rt) = rt_end {
            // What happened before the limiter to the last sample in (a
            // fade, an EQ change) reaches the output one look-ahead later.
            let lookahead = (self.limiter.delay() as f64 / self.rate * 1e9) as u64;
            self.shared.processed_rt.store(rt.nseconds() + lookahead, Ordering::Relaxed);
        }
        // The samples out are rendered at this buffer's own timestamps, so
        // the analysis needs no shift.
        self.shared.tap.push(samples, ch, self.rate as u32, rt_end);
        self.publish_status();
    }

    fn stereo(&mut self, samples: &mut [f64]) {
        let p = &self.params;
        if p.balance != 0.0 {
            let (l, r) = ((1.0 - p.balance).min(1.0), (1.0 + p.balance).min(1.0));
            for frame in samples.chunks_exact_mut(2) {
                frame[0] *= l;
                frame[1] *= r;
            }
        }
        if p.width != 1.0 {
            let w = p.width;
            for frame in samples.chunks_exact_mut(2) {
                let mid = (frame[0] + frame[1]) * 0.5;
                let side = (frame[0] - frame[1]) * 0.5 * w;
                frame[0] = mid + side;
                frame[1] = mid - side;
            }
        }
        if p.crossfeed.is_some() {
            self.crossfeed.run(samples);
        }
    }

    /// Glides the volume towards its target, one step per frame.
    fn apply_volume(&mut self, samples: &mut [f64]) {
        let target = self.shared.volume();
        if self.volume_now == 1.0 && target == 1.0 {
            return;
        }
        for frame in samples.chunks_exact_mut(self.channels) {
            if self.volume_now != target {
                self.volume_now += (target - self.volume_now) * self.volume_coeff;
                if (target - self.volume_now).abs() < VOLUME_SNAP {
                    self.volume_now = target;
                }
            }
            for s in frame {
                *s *= self.volume_now;
            }
        }
    }

    /// Filter, crossfeed and limiter memories, as after silence.
    fn clear_memories(&mut self) {
        self.eq.clear();
        self.crossfeed.clear();
        self.limiter.clear();
    }

    /// A stream starting from rest: nothing of the previous one (its filter
    /// tails, a fade still pending) may leak into it.
    fn restart(&mut self) {
        self.clear_memories();
        self.shared.fade.lock().unwrap().take();
        self.fade_now = 1.0;
        self.fade_target = 1.0;
        self.volume_now = self.shared.volume();
        self.shared.tap.reset();
        self.update_gain();
    }

    fn flush_denormals(&mut self) {
        self.eq.flush_denormals();
        self.crossfeed.flush_denormals();
    }

    /// Crossfeed coefficients for the preset and rate, keeping its memory.
    fn tune_crossfeed(&mut self) {
        let preset = self.params.crossfeed.unwrap_or_default();
        self.crossfeed.retune(Crossfeed::new(preset, self.rate));
    }

    fn sync_params(&mut self) {
        let version = self.shared.version.load(Ordering::SeqCst);
        if version == self.seen {
            return;
        }
        self.seen = version;
        let params = self.shared.params.lock().unwrap().clone();
        let eq_changed = params.eq != self.params.eq;
        let crossfeed_changed = params.crossfeed != self.params.crossfeed;
        self.params = params;
        if eq_changed {
            self.update_eq();
        }
        if crossfeed_changed {
            self.tune_crossfeed();
        }
        self.update_gain();
    }

    fn sync_fade(&mut self) {
        if let Some((target, seconds)) = self.shared.fade.lock().unwrap().take() {
            self.fade_target = target;
            if seconds <= 0.0 {
                self.fade_now = target;
            } else {
                self.fade_step = (1.0 / (seconds * self.rate)).max(1e-9);
            }
        }
    }

    /// Hands the curve to the equalizer, which glides or crossfades to it.
    /// Graphic bands are solved at the stream's rate (a small least-squares
    /// solve, only when the curve or the rate changes).
    fn update_eq(&mut self) {
        let (curve, bands) = match &self.params.eq {
            Some(spec) => (Some((spec.slot, spec.curve.mode)), curve_slots(&spec.curve, self.rate, &mut self.graphic)),
            None => (None, Vec::new()),
        };
        self.eq.update(curve, bands);
    }

    fn update_gain(&mut self) {
        let p = &self.params;
        let mode = match p.replay_gain {
            ReplayGainMode::Smart if self.shared.shuffle.load(Ordering::SeqCst) => ReplayGainMode::Track,
            ReplayGainMode::Smart => ReplayGainMode::Album,
            other => other,
        };
        let rg = self.rg;
        let (source, gain_db, peak) = match mode {
            ReplayGainMode::Off => (RgSource::Off, 0.0, None),
            ReplayGainMode::Track | ReplayGainMode::Smart => match (rg.track_gain, rg.album_gain) {
                (Some(g), _) => (RgSource::Track, g + p.replay_gain_preamp_db, rg.track_peak),
                (None, Some(g)) => (RgSource::Album, g + p.replay_gain_preamp_db, rg.album_peak),
                (None, None) => (RgSource::Untagged, p.replay_gain_fallback_db, None),
            },
            ReplayGainMode::Album => match (rg.album_gain, rg.track_gain) {
                (Some(g), _) => (RgSource::Album, g + p.replay_gain_preamp_db, rg.album_peak),
                (None, Some(g)) => (RgSource::Track, g + p.replay_gain_preamp_db, rg.track_peak),
                (None, None) => (RgSource::Untagged, p.replay_gain_fallback_db, None),
            },
        };
        let mut rg_linear = db_to_linear(gain_db);
        // Boosted tracks land at the limiter's ceiling, so it never has to
        // touch a track whose peak tag is right. (Tags hold sample peaks;
        // what the true peak adds on top, the limiter takes.)
        if p.clipping_prevention
            && let Some(peak) = peak.filter(|p| *p > 0.0)
            && peak * rg_linear > CEILING
        {
            rg_linear = CEILING / peak;
        }
        // Auto preamp takes back only the boost beyond the allowance, and
        // the same for every track, so ReplayGain's levels stay matched.
        let boost = self.eq.max_boost_db();
        let auto_db = p.auto_preamp_allowance_db.map_or(0.0, |allowance| (boost - allowance).max(0.0));
        self.gain_target = rg_linear * db_to_linear(p.preamp_db - auto_db);
        // The EQ counts even when it only cuts: shifting phases can raise
        // peaks, and so can the crossfeed, adding up opposite edges of the
        // two channels. Balance, fades and a narrower image only lower them.
        self.limiter_needed = self.gain_target > 1.0 || self.eq.engaged() || p.width > 1.0 || p.crossfeed.is_some();
        self.status.replay_gain_source = source;
        self.status.replay_gain_db = linear_to_db(rg_linear);
    }

    fn publish_status(&mut self) {
        self.status.limiting = self.limiter.take_limited() > 0;
        let changed = {
            let mut shared = self.shared.status.lock().unwrap();
            let changed = *shared != self.status;
            *shared = self.status;
            changed
        };
        if changed && let Some(notify) = self.shared.on_status.lock().unwrap().as_ref() {
            notify(self.status);
        }
    }
}

pub fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

pub fn linear_to_db(linear: f64) -> f64 {
    20.0 * linear.max(1e-12).log10()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::biquad::State;
    use crate::dsp::settings::EqBand;

    /// A parametric curve of `bands` (tone controls flat).
    fn curve(bands: Vec<EqBand>) -> Option<EqSpec> {
        Some(EqSpec { slot: AbSlot::A, curve: EqCurve { parametric: bands, ..EqCurve::default() } })
    }

    fn processor(params: Params) -> Processor {
        let shared = Arc::new(Shared::new());
        shared.set_params(params);
        let mut p = Processor::new(shared);
        p.set_format(48_000, 2);
        p
    }

    fn sine(frames: usize, amplitude: f64) -> Vec<f64> {
        (0..frames)
            .flat_map(|n| {
                let v = amplitude * (2.0 * std::f64::consts::PI * 440.0 * n as f64 / 48_000.0).sin();
                [v, v]
            })
            .collect()
    }

    /// Processes `input` followed by enough silence to flush it through the
    /// limiter's look-ahead, and returns what came out for it.
    fn run(p: &mut Processor, input: &[f64]) -> Vec<f64> {
        let lag = p.limiter.delay() * p.channels;
        let mut samples = input.to_vec();
        samples.extend(std::iter::repeat_n(0.0, lag));
        p.process(&mut samples, None);
        samples.split_off(lag)
    }

    /// The output is exactly the input, one look-ahead later (silence first).
    fn assert_delayed(p: &Processor, output: &[f64], input: &[f64]) {
        let lag = p.limiter.delay() * p.channels;
        assert!(output[..lag].iter().all(|s| *s == 0.0));
        assert_eq!(&output[lag..], &input[..input.len() - lag]);
    }

    #[test]
    fn neutral_settings_leave_samples_untouched() {
        let mut p = processor(Params::default());
        let input = sine(4800, 0.5);
        let mut samples = input.clone();
        p.process(&mut samples, None);
        assert_delayed(&p, &samples, &input);
    }

    /// With everything off and the volume at 100%, full-scale masters come
    /// out bit for bit: the limiter has nothing to guard against.
    #[test]
    fn neutral_settings_leave_full_scale_peaks_untouched() {
        let square: Vec<f64> =
            (0..9600).flat_map(|n| if (n / 50) % 2 == 0 { [1.0, 1.0] } else { [-1.0, -1.0] }).collect();
        for input in [sine(9600, 1.0), square] {
            let mut p = processor(Params::default());
            let mut samples = input.clone();
            p.process(&mut samples, None);
            assert_delayed(&p, &samples, &input);
            assert!(!p.status.limiting);
        }
    }

    #[test]
    fn a_replay_gain_boost_whose_peak_fits_never_trips_the_limiter() {
        gst::init().unwrap();
        let mut p = processor(Params { replay_gain: ReplayGainMode::Track, ..Params::default() });
        let mut tags = gst::TagList::new();
        {
            let tags = tags.get_mut().unwrap();
            tags.add::<gst::tags::TrackGain>(&4.0, gst::TagMergeMode::Append);
            tags.add::<gst::tags::TrackPeak>(&0.5, gst::TagMergeMode::Append);
        }
        p.tags(&tags);
        let mut samples = sine(48_000, 0.5);
        p.process(&mut samples, None);
        let tail_peak = samples[48_000..].iter().fold(0.0f64, |m, s| m.max(s.abs()));
        assert!((linear_to_db(tail_peak / 0.5) - 4.0).abs() < 0.05, "the boost is heard in full");
        assert!(!p.status.limiting);
    }

    /// The processor on `shared`, warmed up at the volume set there.
    fn mono(shared: &Arc<Shared>) -> Processor {
        let mut p = Processor::new(shared.clone());
        p.set_format(48_000, 1);
        p.process(&mut [1.0; 480], None);
        p
    }

    fn largest_step(samples: &[f64]) -> f64 {
        samples.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max)
    }

    #[test]
    fn volume_changes_glide_instead_of_stepping() {
        let shared = Arc::new(Shared::new());
        let mut p = mono(&shared);
        shared.set_volume(0.5);
        // A constant input: the output is the gain applied to each sample.
        let mut samples = vec![1.0; 9600];
        p.process(&mut samples, None);
        assert!(samples[0] > 0.99, "no jump at the buffer boundary: {}", samples[0]);
        assert!(largest_step(&samples) < 0.0015, "step {}", largest_step(&samples));
        let at_50ms = samples[2400];
        assert!((linear_to_db(at_50ms) - linear_to_db(0.5)).abs() < 0.1, "{at_50ms} at 50 ms");
        assert_eq!(samples[9599], 0.5, "lands exactly on the target");
    }

    #[test]
    fn muting_ramps_to_exact_silence_and_back_to_unity() {
        let shared = Arc::new(Shared::new());
        let mut p = mono(&shared);
        shared.set_volume(0.0);
        let mut samples = vec![1.0; 9600];
        p.process(&mut samples, None);
        assert!(largest_step(&samples) < 0.003, "step {}", largest_step(&samples));
        assert_eq!(samples[9599], 0.0, "silent after 200 ms");

        shared.set_volume(1.0);
        let mut samples = vec![1.0; 9600];
        p.process(&mut samples, None);
        assert!(samples[0] < 0.01);
        assert_eq!(samples[9599], 1.0, "back to exact unity");
    }

    #[test]
    fn a_new_stream_starts_at_the_volume_set() {
        let shared = Arc::new(Shared::new());
        shared.set_volume(0.25);
        let mut p = Processor::new(shared.clone());
        p.set_format(48_000, 1);
        assert_eq!(run(&mut p, &[1.0; 4]), [0.25; 4], "no ramp from unity on the first buffer");

        shared.set_volume(1.0);
        shared.restart();
        assert_eq!(run(&mut p, &[1.0; 4]), [1.0; 4], "nor after the pipeline was reset");
    }

    #[test]
    fn decaying_filter_tails_are_flushed_to_zero() {
        let mut p = processor(Params { eq: curve(vec![EqBand::peak(31.0, 6.0, 1.41)]), ..Params::default() });
        let mut impulse = vec![0.0; 2048];
        impulse[..2].copy_from_slice(&[1.0, 1.0]);
        p.process(&mut impulse, None);
        let mut silence = vec![0.0; 2048];
        for _ in 0..150 {
            silence.fill(0.0);
            p.process(&mut silence, None);
        }
        assert!(p.eq.states().iter().all(|s| *s == State::default()), "{:?}", p.eq.states());
        assert!(silence.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn applies_album_gain_from_tags_and_prevents_clipping_with_the_peak() {
        gst::init().unwrap();
        let mut p = processor(Params { replay_gain: ReplayGainMode::Album, ..Params::default() });
        let mut tags = gst::TagList::new();
        {
            let tags = tags.get_mut().unwrap();
            tags.add::<gst::tags::AlbumGain>(&-6.0, gst::TagMergeMode::Append);
            tags.add::<gst::tags::AlbumPeak>(&0.9, gst::TagMergeMode::Append);
        }
        p.tags(&tags);
        let mut samples = sine(48_000, 0.5);
        p.process(&mut samples, None);
        let tail_peak = samples[48_000..].iter().fold(0.0f64, |m, s| m.max(s.abs()));
        assert!((linear_to_db(tail_peak / 0.5) + 6.0).abs() < 0.05, "gain {}", linear_to_db(tail_peak / 0.5));
        assert_eq!(p.status.replay_gain_source, RgSource::Album);

        // +10 dB would push a 0.9 peak over full scale; the peak caps it.
        {
            let mut boosted = gst::TagList::new();
            boosted.get_mut().unwrap().add::<gst::tags::AlbumGain>(&10.0, gst::TagMergeMode::Append);
            p.tags(&boosted);
        }
        assert!((p.status.replay_gain_db - linear_to_db(CEILING / 0.9)).abs() < 1e-6);

        p.stream_start();
        assert_eq!(p.status.replay_gain_source, RgSource::Untagged, "a new track forgets the old tags");
    }

    #[test]
    fn the_limiter_keeps_boosted_signals_below_full_scale() {
        let mut p = processor(Params { preamp_db: 12.0, ..Params::default() });
        let mut samples = sine(48_000, 0.9);
        p.process(&mut samples, None);
        let peak = samples.iter().fold(0.0f64, |m, s| m.max(s.abs()));
        assert!(peak <= CEILING + 1e-12, "peak {peak}");
        assert!(p.status.limiting);
    }

    #[test]
    fn width_zero_is_mono_and_balance_pans() {
        let mut p = processor(Params { width: 0.0, ..Params::default() });
        assert_eq!(run(&mut p, &[1.0, 0.0, 0.2, -0.2]), [0.5, 0.5, 0.0, 0.0]);

        let mut p = processor(Params { balance: 1.0, ..Params::default() });
        assert_eq!(run(&mut p, &[0.4, 0.4]), [0.0, 0.4]);
    }

    /// Steady-state gain (dB) the processor applies to a 1 kHz sine.
    fn gain_at_1k(params: Params) -> f64 {
        let mut p = processor(params);
        let mut samples: Vec<f64> = (0..48_000)
            .flat_map(|n| {
                let v = 0.1 * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / 48_000.0).sin();
                [v, v]
            })
            .collect();
        p.process(&mut samples, None);
        let tail_peak = samples[48_000..].iter().fold(0.0f64, |m, s| m.max(s.abs()));
        linear_to_db(tail_peak / 0.1)
    }

    #[test]
    fn auto_preamp_lowers_the_level_only_beyond_its_allowance() {
        let boost = |db| Params {
            eq: curve(vec![EqBand::peak(1000.0, db, 1.0)]),
            auto_preamp_allowance_db: Some(3.0),
            ..Params::default()
        };
        let measured = gain_at_1k(boost(2.0));
        assert!((measured - 2.0).abs() < 0.05, "a boost within the allowance is heard in full: {measured}");
        let measured = gain_at_1k(boost(8.0));
        assert!((measured - 3.0).abs() < 0.05, "the rest of the boost is taken back: {measured}");
        let off = Params { auto_preamp_allowance_db: None, ..boost(8.0) };
        let measured = gain_at_1k(off);
        assert!((measured - 8.0).abs() < 0.05, "without auto preamp: {measured}");
        let strict = Params { auto_preamp_allowance_db: Some(0.0), ..boost(8.0) };
        let measured = gain_at_1k(strict);
        assert!(measured.abs() < 0.05, "no allowance keeps the peak at unity: {measured}");
    }

    #[test]
    fn auto_preamp_is_the_same_for_every_track() {
        gst::init().unwrap();
        let mut p = processor(Params {
            eq: curve(vec![EqBand::peak(1000.0, 8.0, 1.0)]),
            auto_preamp_allowance_db: Some(3.0),
            replay_gain: ReplayGainMode::Track,
            ..Params::default()
        });
        let with_tags = |gain: f64, peak: f64| {
            let mut tags = gst::TagList::new();
            {
                let tags = tags.get_mut().unwrap();
                tags.add::<gst::tags::TrackGain>(&gain, gst::TagMergeMode::Append);
                tags.add::<gst::tags::TrackPeak>(&peak, gst::TagMergeMode::Append);
            }
            tags
        };
        p.process(&mut [0.0, 0.0], None);
        // Tracks with different gains and peaks lose the same 5 dB.
        p.tags(&with_tags(-9.0, 1.0));
        let loud = linear_to_db(p.gain_target);
        p.stream_start();
        p.tags(&with_tags(-1.0, 0.5));
        let dynamic = linear_to_db(p.gain_target);
        assert!((loud - (-9.0 - 5.0)).abs() < 0.01, "{loud}");
        assert!((dynamic - (-1.0 - 5.0)).abs() < 0.01, "{dynamic}");
    }

    /// A continuous 997 Hz stereo tone at 0.2, handed out block by block.
    /// Not a divisor of the rate, so changes land mid-waveform, where a
    /// click would show.
    struct Tone(usize);

    impl Tone {
        fn next(&mut self, frames: usize) -> Vec<f64> {
            let start = self.0;
            self.0 += frames;
            (start..start + frames)
                .flat_map(|n| {
                    let v = 0.2 * (2.0 * std::f64::consts::PI * 997.0 * n as f64 / 48_000.0).sin();
                    [v, v]
                })
                .collect()
        }
    }

    /// Largest sample-to-sample change of the left channel: a click shows
    /// up as a step far larger than the tone's own.
    fn roughness(samples: &[f64]) -> f64 {
        let left: Vec<f64> = samples.iter().step_by(2).copied().collect();
        largest_step(&left)
    }

    /// Plays the tone through `from`, switches to `to`, and checks that the
    /// switch is no rougher than the steady tone on either side of it.
    fn switches_without_a_click(from: Params, to: Params) {
        let shared = Arc::new(Shared::new());
        shared.set_params(from);
        let mut p = Processor::new(shared.clone());
        p.set_format(48_000, 2);
        let mut tone = Tone(0);
        let mut play = |p: &mut Processor, frames| {
            let mut samples = tone.next(frames);
            p.process(&mut samples, None);
            samples
        };
        play(&mut p, 24_011);
        let before = roughness(&play(&mut p, 4_801));
        shared.set_params(to);
        let during = roughness(&play(&mut p, 4_801));
        play(&mut p, 24_011);
        let after = roughness(&play(&mut p, 4_801));
        let steady = before.max(after);
        assert!(during <= steady * 1.03, "switching stepped by {during:.5}; steady tone {steady:.5}");
    }

    fn graphic(gains: [f64; 10], slot: AbSlot) -> Option<EqSpec> {
        use crate::dsp::settings::EqMode;
        Some(EqSpec {
            slot,
            curve: EqCurve { mode: EqMode::Graphic10, graphic10: gains.to_vec(), ..EqCurve::default() },
        })
    }

    #[test]
    fn discrete_eq_changes_crossfade_without_a_click() {
        let a = graphic([0.0, 2.0, 0.0, -3.0, 0.0, 6.0, 0.0, 4.0, 0.0, 0.0], AbSlot::A);
        let b = graphic([0.0, 0.0, 0.0, 0.0, -2.0, -6.0, 3.0, 0.0, 0.0, 0.0], AbSlot::B);
        let eq = |eq| Params { eq, ..Params::default() };
        // A/B compare, bypass and the EQ switched on.
        switches_without_a_click(eq(a.clone()), eq(b));
        switches_without_a_click(eq(a.clone()), eq(None));
        switches_without_a_click(eq(None), eq(a));
        // A band that shapes the sound switched off.
        let mut band = EqBand::peak(1000.0, 6.0, 1.0);
        let on = curve(vec![EqBand::peak(200.0, 3.0, 1.0), band]);
        band.enabled = false;
        switches_without_a_click(eq(on), eq(curve(vec![EqBand::peak(200.0, 3.0, 1.0), band])));
    }

    #[test]
    fn a_band_glides_to_a_new_gain_without_a_click() {
        let eq = |db| Params { eq: curve(vec![EqBand::peak(1000.0, db, 1.0)]), ..Params::default() };
        switches_without_a_click(eq(0.0), eq(12.0));
        switches_without_a_click(eq(12.0), eq(-12.0));
        assert!((gain_at_1k(eq(12.0)) - 12.0).abs() < 0.05, "lands on the new gain");
    }

    #[test]
    fn moving_one_slider_leaves_the_other_bands_memories_alone() {
        let shared = Arc::new(Shared::new());
        let mut gains = [3.0, 2.0, 0.0, -3.0, 1.0, 6.0, 0.0, 4.0, 2.0, 0.0];
        shared.set_params(Params { eq: graphic(gains, AbSlot::A), ..Params::default() });
        let mut p = Processor::new(shared.clone());
        p.set_format(48_000, 2);
        let mut tone = Tone(0);
        p.process(&mut tone.next(24_000), None);
        let before = p.eq.states().to_vec();
        assert!(before.iter().any(|s| *s != State::default()));

        gains[2] = 1.0;
        shared.set_params(Params { eq: graphic(gains, AbSlot::A), ..Params::default() });
        p.process(&mut [], None);
        assert!(!p.eq.crossfading(), "a slider move glides");
        assert_eq!(p.eq.states(), &before[..], "no band's memory was reset");
    }

    #[test]
    fn a_flat_or_switched_off_eq_leaves_samples_untouched() {
        let shared = Arc::new(Shared::new());
        let flat = Params { eq: graphic([0.0; 10], AbSlot::A), ..Params::default() };
        shared.set_params(flat);
        let mut p = Processor::new(shared.clone());
        p.set_format(48_000, 2);
        let input = sine(9600, 1.0);
        assert_eq!(run(&mut p, &input), input, "an EQ with every band flat");

        shared.set_params(Params { eq: graphic([6.0; 10], AbSlot::A), ..Params::default() });
        p.process(&mut sine(9600, 0.1), None);
        shared.set_params(Params::default());
        p.process(&mut sine(9600, 0.1), None);
        assert_eq!(run(&mut p, &input), input, "once the EQ has faded out, the limiter is out too");
        assert!(!p.status.limiting);
    }

    /// A gapless join announces the stream anew (stream start, the same
    /// caps): the EQ's memories, the limiter's delay line and its gain carry
    /// on as if nothing happened.
    #[test]
    fn a_gapless_join_keeps_every_memory() {
        let params = Params {
            preamp_db: 9.0,
            eq: graphic([3.0, 0.0, -2.0, 4.0, 0.0, 6.0, 0.0, 2.0, 0.0, -3.0], AbSlot::A),
            ..Params::default()
        };
        let (mut joined, mut straight) = (processor(params.clone()), processor(params));
        let mut tone = Tone(0);
        for block in 0..6 {
            let input = tone.next(4_801);
            let (mut a, mut b) = (input.clone(), input);
            if block == 3 {
                joined.stream_start();
                joined.set_format(48_000, 2);
            }
            joined.process(&mut a, None);
            straight.process(&mut b, None);
            assert_eq!(a, b, "block {block}");
        }
        assert!(joined.status.limiting, "the limiter was busy across the join");
    }

    fn one_band(db: f64, enabled: bool) -> Params {
        Params { eq: curve(vec![EqBand { enabled, ..EqBand::peak(1000.0, db, 1.0) }]), ..Params::default() }
    }

    fn play(p: &mut Processor, tone: &mut Tone, frames: usize) -> Vec<f64> {
        let mut samples = tone.next(frames);
        p.process(&mut samples, None);
        samples
    }

    /// A band gliding back to flat still shapes the sound: switching it off
    /// then crossfades, and leaves no memory to come back later.
    #[test]
    fn switching_a_band_off_mid_glide_neither_clicks_nor_leaves_memory() {
        // `a` is switched off 256 frames into its glide to 0 dB; `b` only
        // once the glide has landed.
        let (sa, sb) = (Arc::new(Shared::new()), Arc::new(Shared::new()));
        sa.set_params(one_band(6.0, true));
        sb.set_params(one_band(6.0, true));
        let (mut a, mut b) = (Processor::new(sa.clone()), Processor::new(sb.clone()));
        let (mut ta, mut tb) = (Tone(0), Tone(0));
        play(&mut a, &mut ta, 24_011);
        play(&mut b, &mut tb, 24_011);
        let steady = roughness(&play(&mut a, &mut ta, 4_801));
        play(&mut b, &mut tb, 4_801);

        sa.set_params(one_band(0.0, true));
        sb.set_params(one_band(0.0, true));
        let mut heard = play(&mut a, &mut ta, 256);
        play(&mut b, &mut tb, 256);
        sa.set_params(one_band(0.0, false));
        heard.extend(play(&mut a, &mut ta, 256));
        play(&mut b, &mut tb, 256);
        let during = roughness(&heard);
        assert!(during <= steady * 1.03, "switching off mid-glide stepped by {during:.5}; steady {steady:.5}");

        play(&mut a, &mut ta, 24_000);
        play(&mut b, &mut tb, 12_000);
        sb.set_params(one_band(0.0, false));
        play(&mut b, &mut tb, 12_000);
        assert!(a.eq.states().iter().all(|s| *s == State::default()), "no memory left in the band switched off");

        // Back on and raised again: both must sound the same.
        for shared in [&sa, &sb] {
            shared.set_params(one_band(0.0, true));
        }
        play(&mut a, &mut ta, 480);
        play(&mut b, &mut tb, 480);
        for shared in [&sa, &sb] {
            shared.set_params(one_band(6.0, true));
        }
        assert_eq!(play(&mut a, &mut ta, 4_800), play(&mut b, &mut tb, 4_800), "stale memory came back");
    }

    /// A seek or a new stream mid-glide lands the EQ at once; the limiter
    /// must follow it out, so a flat EQ is bit-exact again.
    #[test]
    fn a_seek_or_restart_mid_glide_leaves_a_flat_eq_bit_exact() {
        for restart in [false, true] {
            let shared = Arc::new(Shared::new());
            shared.set_params(one_band(6.0, true));
            let mut p = Processor::new(shared.clone());
            p.process(&mut sine(9600, 0.1), None);
            shared.set_params(one_band(0.0, true));
            p.process(&mut sine(480, 0.1), None);
            if restart {
                shared.restart();
            } else {
                p.flush();
            }
            let input = sine(9600, 1.0);
            p.process(&mut input.clone(), None);
            assert_eq!(run(&mut p, &input), input, "restart {restart}");
            assert!(!p.limiter_needed && !p.status.limiting);
        }
    }

    fn preamped(slot: AbSlot, db: f64) -> Params {
        Params {
            eq: Some(EqSpec {
                slot,
                curve: EqCurve { parametric: vec![EqBand::peak(1000.0, db, 1.0)], ..EqCurve::default() },
            }),
            auto_preamp_allowance_db: Some(0.0),
            ..Params::default()
        }
    }

    /// A curve waiting for a crossfade to end is allowed for before it is
    /// heard: auto preamp takes its boost back from the start.
    #[test]
    fn a_curve_waiting_for_a_crossfade_is_preamped_before_it_is_heard() {
        let shared = Arc::new(Shared::new());
        shared.set_params(preamped(AbSlot::A, 0.0));
        let mut p = Processor::new(shared.clone());
        let mut tone = Tone(0);
        play(&mut p, &mut tone, 4800);
        shared.set_params(preamped(AbSlot::B, 0.0));
        play(&mut p, &mut tone, 240);
        shared.set_params(preamped(AbSlot::A, 12.0));
        let out = play(&mut p, &mut tone, 4096);
        let tail = out[2 * 2000..].iter().fold(0.0f64, |m, s| m.max(s.abs()));
        let settled = play(&mut p, &mut tone, 9600)[2 * 4800..].iter().fold(0.0f64, |m, s| m.max(s.abs()));
        assert!(tail < settled * 1.1, "the waiting boost played unpreamped: {tail:.3} against {settled:.3}");

        // And when a seek brings it in at once.
        shared.set_params(preamped(AbSlot::B, 0.0));
        play(&mut p, &mut tone, 240);
        shared.set_params(preamped(AbSlot::A, 12.0));
        p.process(&mut [], None);
        p.flush();
        assert!((p.gain_target - db_to_linear(-p.eq.max_boost_db())).abs() < 1e-6, "auto preamp not refreshed");
        assert!(p.limiter_needed);
    }

    #[test]
    fn seeks_fade_in_when_soft_fades_are_on() {
        let mut p = processor(Params { soft_fades: true, ..Params::default() });
        p.flush();
        let samples = run(&mut p, &[0.5; 48_000 * 2]);
        assert!(samples[0] < 0.01, "starts from silence");
        let halfway = (FADE_S / 2.0 * 48_000.0) as usize * 2;
        assert!((samples[halfway] - 0.25).abs() < 0.01, "{}", samples[halfway]);
        assert_eq!(samples[samples.len() - 1], 0.5);
    }

    #[test]
    fn fades_ramp_to_their_target() {
        let shared = Arc::new(Shared::new());
        let mut p = Processor::new(shared.clone());
        p.set_format(1000, 1);
        shared.fade(0.0, 0.1);
        let samples = run(&mut p, &[1.0; 200]);
        assert!((samples[49] - 0.5).abs() < 0.02, "halfway at 50 ms: {}", samples[49]);
        assert_eq!(samples[150], 0.0);
    }
}
