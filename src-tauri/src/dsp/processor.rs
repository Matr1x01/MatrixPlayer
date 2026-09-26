//! The per-buffer processing that runs on GStreamer's streaming thread.
//!
//! ```text
//! F64 in ─ ReplayGain × preamp ─ EQ biquads ─ balance ─ width ─ crossfeed
//!        ─ fade ─ peak limiter ─ F64 out (then volume and the output)
//! ```
//!
//! Stages that are neutral are skipped outright, so with everything off the
//! samples pass through untouched. ReplayGain values come from the tag events
//! that travel with the audio, so at a gapless join the new track's gain
//! starts on exactly its first sample.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gstreamer as gst;
use serde::Serialize;

use super::analysis::AnalysisTap;
use super::biquad::{Coeffs, State};
use super::settings::{EqBand, ReplayGainMode};

/// Samples above this are pulled down by the limiter (−0.1 dBFS).
const LIMIT: f64 = 0.988_553;
const LIMITER_RELEASE_S: f64 = 0.08;
const CROSSFEED_HZ: f64 = 700.0;
const CROSSFEED_LEVEL: f64 = 0.3;
/// Changes of static gain are spread over this long, to avoid zipper noise.
const GAIN_RAMP_S: f64 = 0.02;
/// Length of the soft fades on pause, resume and seek.
pub const FADE_S: f64 = 0.12;

/// Everything the audio thread needs, derived from the user's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub filters: Vec<EqBand>,
    pub preamp_db: f64,
    pub balance: f64,
    pub width: f64,
    pub crossfeed: bool,
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
            filters: Vec::new(),
            preamp_db: 0.0,
            balance: 0.0,
            width: 1.0,
            crossfeed: false,
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
    shuffle: std::sync::atomic::AtomicBool,
    fade: Mutex<Option<(f64, f64)>>,
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
            shuffle: std::sync::atomic::AtomicBool::new(false),
            fade: Mutex::new(None),
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
}

pub struct Processor {
    shared: Arc<Shared>,
    seen: u64,
    params: Params,
    rate: f64,
    channels: usize,
    coeffs: Vec<Coeffs>,
    /// `states[band * channels + channel]`
    states: Vec<State>,
    rg: RgTags,
    /// Target static gain (ReplayGain × preamp) and the gain now, ramped.
    gain_target: f64,
    gain_now: f64,
    fade_now: f64,
    fade_target: f64,
    fade_step: f64,
    limiter_env: f64,
    limiter_release: f64,
    limited_frames: u64,
    xfeed_lp: [f64; 2],
    xfeed_a: f64,
    segment: Option<gst::FormattedSegment<gst::ClockTime>>,
    status: DspStatus,
}

impl Processor {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            seen: 0,
            params: Params::default(),
            rate: 48_000.0,
            channels: 2,
            coeffs: Vec::new(),
            states: Vec::new(),
            rg: RgTags::default(),
            gain_target: 1.0,
            gain_now: 1.0,
            fade_now: 1.0,
            fade_target: 1.0,
            fade_step: 0.0,
            limiter_env: 1.0,
            limiter_release: 0.0,
            limited_frames: 0,
            xfeed_lp: [0.0; 2],
            xfeed_a: 0.0,
            segment: None,
            status: DspStatus::default(),
        }
    }

    /// New stream format: filters are recomputed for the sample rate.
    pub fn set_format(&mut self, rate: u32, channels: u32) {
        self.rate = f64::from(rate.max(1));
        self.channels = channels.max(1) as usize;
        self.limiter_release = 1.0 - (-1.0 / (LIMITER_RELEASE_S * self.rate)).exp();
        self.xfeed_a = (-2.0 * std::f64::consts::PI * CROSSFEED_HZ / self.rate).exp();
        self.status.sample_rate = rate;
        self.status.channels = channels;
        self.rebuild();
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
        self.states.iter_mut().for_each(|s| *s = State::default());
        self.xfeed_lp = [0.0; 2];
        self.limiter_env = 1.0;
        self.shared.tap.reset();
        self.sync_params();
        if self.params.soft_fades {
            // The new position starts from silence instead of a jump.
            self.fade_now = 0.0;
            self.fade_step = 1.0 / (FADE_S * self.rate);
        }
    }

    /// Processes interleaved F64 samples in place. `pts` is the buffer's
    /// timestamp, used to line the analysis up with what is audible.
    pub fn process(&mut self, samples: &mut [f64], pts: Option<gst::ClockTime>) {
        self.sync_params();
        self.sync_fade();
        let ch = self.channels;
        let frames = samples.len() / ch;
        if frames == 0 {
            return;
        }

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

        // Equalizer.
        for (band, coeffs) in self.coeffs.iter().enumerate() {
            let states = &mut self.states[band * ch..(band + 1) * ch];
            for frame in samples.chunks_exact_mut(ch) {
                for (s, state) in frame.iter_mut().zip(states.iter_mut()) {
                    *s = state.run(coeffs, *s);
                }
            }
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

        self.limit(samples);

        let rt_end = pts.and_then(|pts| {
            let duration = gst::ClockTime::from_nseconds((frames as f64 / self.rate * 1e9) as u64);
            self.segment.as_ref().and_then(|seg| seg.to_running_time(pts + duration))
        });
        if let Some(rt) = rt_end {
            self.shared.processed_rt.store(rt.nseconds(), Ordering::Relaxed);
        }
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
        if p.crossfeed {
            let a = self.xfeed_a;
            let norm = 1.0 / (1.0 + CROSSFEED_LEVEL);
            for frame in samples.chunks_exact_mut(2) {
                self.xfeed_lp[0] = self.xfeed_lp[0] * a + frame[0] * (1.0 - a);
                self.xfeed_lp[1] = self.xfeed_lp[1] * a + frame[1] * (1.0 - a);
                let (l, r) = (frame[0], frame[1]);
                frame[0] = (l + CROSSFEED_LEVEL * self.xfeed_lp[1]) * norm;
                frame[1] = (r + CROSSFEED_LEVEL * self.xfeed_lp[0]) * norm;
            }
        }
    }

    /// Instant attack, smooth release: overs are caught on their first
    /// sample and the gain recovers over ~80 ms.
    fn limit(&mut self, samples: &mut [f64]) {
        let ch = self.channels;
        for frame in samples.chunks_exact_mut(ch) {
            let peak = frame.iter().fold(0.0f64, |m, s| m.max(s.abs()));
            let needed = if peak > LIMIT { LIMIT / peak } else { 1.0 };
            if needed < self.limiter_env {
                self.limiter_env = needed;
            } else if self.limiter_env < 1.0 {
                // Recover, but never so far that this frame goes over.
                self.limiter_env = (self.limiter_env + (1.0 - self.limiter_env) * self.limiter_release).min(needed);
                if self.limiter_env > 0.999_999 {
                    self.limiter_env = 1.0;
                }
            }
            if self.limiter_env < 1.0 {
                self.limited_frames += 1;
                for s in frame {
                    *s *= self.limiter_env;
                }
            }
        }
    }

    fn sync_params(&mut self) {
        let version = self.shared.version.load(Ordering::SeqCst);
        if version == self.seen {
            return;
        }
        self.seen = version;
        let params = self.shared.params.lock().unwrap().clone();
        let filters_changed = params.filters != self.params.filters;
        self.params = params;
        if filters_changed {
            self.rebuild();
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

    fn rebuild(&mut self) {
        let old_bands = self.coeffs.len();
        self.coeffs = self.params.filters.iter().map(|b| Coeffs::for_band(b, self.rate)).collect();
        // Keep filter memories when only coefficients change, so dragging a
        // band does not click; reset them when the layout changes.
        if old_bands != self.coeffs.len() || self.states.len() != self.coeffs.len() * self.channels {
            self.states = vec![State::default(); self.coeffs.len() * self.channels];
        }
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
        if p.clipping_prevention
            && let Some(peak) = peak.filter(|p| *p > 0.0)
            && peak * rg_linear > 1.0
        {
            rg_linear = 1.0 / peak;
        }
        self.gain_target = rg_linear * db_to_linear(p.preamp_db);
        self.status.replay_gain_source = source;
        self.status.replay_gain_db = linear_to_db(rg_linear);
    }

    fn publish_status(&mut self) {
        self.status.limiting = self.limited_frames > 0;
        self.limited_frames = 0;
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

    #[test]
    fn neutral_settings_leave_samples_untouched() {
        let mut p = processor(Params::default());
        let input = sine(4800, 0.5);
        let mut samples = input.clone();
        p.process(&mut samples, None);
        assert_eq!(samples, input);
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
        assert!((p.status.replay_gain_db - linear_to_db(1.0 / 0.9)).abs() < 1e-6);

        p.stream_start();
        assert_eq!(p.status.replay_gain_source, RgSource::Untagged, "a new track forgets the old tags");
    }

    #[test]
    fn the_limiter_keeps_boosted_signals_below_full_scale() {
        let mut p = processor(Params { preamp_db: 12.0, ..Params::default() });
        let mut samples = sine(48_000, 0.9);
        p.process(&mut samples, None);
        let peak = samples.iter().fold(0.0f64, |m, s| m.max(s.abs()));
        assert!(peak <= LIMIT + 1e-9, "peak {peak}");
        assert!(p.status.limiting);
    }

    #[test]
    fn width_zero_is_mono_and_balance_pans() {
        let mut p = processor(Params { width: 0.0, ..Params::default() });
        let mut samples = vec![1.0, 0.0, 0.2, -0.2];
        p.process(&mut samples, None);
        assert_eq!(samples, [0.5, 0.5, 0.0, 0.0]);

        let mut p = processor(Params { balance: 1.0, ..Params::default() });
        let mut samples = vec![0.4, 0.4];
        p.process(&mut samples, None);
        assert_eq!(samples, [0.0, 0.4]);
    }

    #[test]
    fn seeks_fade_in_when_soft_fades_are_on() {
        let mut p = processor(Params { soft_fades: true, ..Params::default() });
        p.flush();
        let mut samples = vec![0.5; 48_000 * 2];
        p.process(&mut samples, None);
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
        let mut samples = vec![1.0; 200];
        p.process(&mut samples, None);
        assert!((samples[49] - 0.5).abs() < 0.02, "halfway at 50 ms: {}", samples[49]);
        assert_eq!(samples[150], 0.0);
    }
}
