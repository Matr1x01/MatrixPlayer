//! A look-ahead limiter that keeps the true peak (between samples, as a DAC
//! reconstructs it) under a ceiling, without a step in gain.
//!
//! ```text
//! in ─┬───────────────────────────── delay Δ ───────────────────────────┐
//!     └─ 4× interpolate ─ gain needed ─ hold ─ release ─ moving average ─ × ─ out
//! ```
//!
//! Every frame, the gain that would bring its (interpolated) peak down to
//! the ceiling is computed, linked across channels. Holding the smallest
//! of those over a window, then averaging it over a slightly shorter one,
//! gives a gain that ramps down smoothly and reaches full reduction before
//! the peak gets out of the delay line, and stays there for a few samples
//! either side of it (an inter-sample peak is reconstructed from its
//! neighbours too). The release is the old limiter's ~80 ms.
//!
//! The delay always runs, so the limiter's latency never changes; while it
//! has nothing to do, the output is the input, delayed, bit for bit.
//!
//! Peaks are found on a 4× oversampled copy of the signal (for detection
//! only). The interpolator is a Kaiser-windowed sinc of odd length, so its
//! four phases fall on whole, quarter and half samples. ITU-R BS.1770-4's
//! example filter (Annex 2) was tried and not used: its even length puts
//! every phase an eighth of a sample off the grid, and with its ripple it
//! lets an fs/4 tone at 45° out 0.006 dB over the ceiling, where this one
//! holds it 0.016 dB under. The Recommendation allows "a method that gives
//! similar or superior results". Like any 4× detector it under-reads the
//! highest frequencies a little: a 20 kHz tone at 48 kHz comes out at up to
//! −0.2 dBTP.

use std::collections::VecDeque;
use std::sync::OnceLock;

/// −0.5 dBTP: headroom for the inter-sample overs a DAC or a lossy encoder
/// adds on top of the true peak measured here.
pub const CEILING: f64 = 0.944_060_876_285_923_4;
/// How far the limiter looks ahead (its latency).
const LOOKAHEAD_S: f64 = 0.0015;
const RELEASE_S: f64 = 0.08;
/// Taps per phase of the 4× interpolation filter.
const TAPS: usize = 12;
/// How many input samples the interpolated peaks lag the newest input
/// (the interpolation filter's group delay, rounded up).
const DETECT_DELAY: usize = 6;
/// Samples either side of a peak kept at its full reduction: they carry
/// most of what reconstructs an inter-sample peak.
const MARGIN: usize = 4;

pub struct Limiter {
    channels: usize,
    /// Input history: `frames` frames written twice, at `i` and at
    /// `i + frames`, so the newest `TAPS` are always contiguous.
    history: Vec<f64>,
    frames: usize,
    /// Where the next frame goes.
    pos: usize,
    delay: usize,
    hold_len: usize,
    avg_len: usize,
    /// Frames seen, to age entries out of the hold window.
    n: u64,
    /// Sliding minimum of the gains needed over `hold_len`, as a monotonic
    /// queue (front is the smallest); empty means nothing to hold (1.0).
    hold: VecDeque<(u64, f64)>,
    env: f64,
    release: f64,
    /// The last `avg_len` envelope values, their sum, and how many are
    /// below 1 (none: the gain is exactly 1, whatever rounding left in `sum`).
    avg: Vec<f64>,
    avg_pos: usize,
    sum: f64,
    below: usize,
    /// Frames the gain was below 1 since last asked.
    limited: u64,
}

impl Limiter {
    pub fn new(rate: f64, channels: usize) -> Self {
        let mut limiter = Self {
            channels: 0,
            history: Vec::new(),
            frames: 0,
            pos: 0,
            delay: 0,
            hold_len: 0,
            avg_len: 0,
            n: 0,
            hold: VecDeque::new(),
            env: 1.0,
            release: 0.0,
            avg: Vec::new(),
            avg_pos: 0,
            sum: 0.0,
            below: 0,
            limited: 0,
        };
        limiter.set_format(rate, channels);
        limiter
    }

    /// Sizes everything for a format and starts from silence.
    pub fn set_format(&mut self, rate: f64, channels: usize) {
        self.channels = channels;
        self.delay = ((LOOKAHEAD_S * rate).round() as usize).max(DETECT_DELAY + MARGIN + 1);
        // A peak detected at frame j needs its reduction on the output
        // frames holding j − DETECT_DELAY ± MARGIN (see `gain`).
        self.avg_len = self.delay + 1 - DETECT_DELAY - MARGIN;
        self.hold_len = self.avg_len + 2 * MARGIN;
        self.frames = (self.delay + 1).max(TAPS);
        self.history = vec![0.0; 2 * self.frames * channels];
        self.hold = VecDeque::with_capacity(self.hold_len + 1);
        self.avg = vec![1.0; self.avg_len];
        self.release = 1.0 - (-1.0 / (RELEASE_S * rate)).exp();
        self.clear();
    }

    /// Forgets the signal: silence in the delay line, no gain reduction.
    pub fn clear(&mut self) {
        self.history.fill(0.0);
        self.pos = 0;
        self.n = 0;
        self.hold.clear();
        self.env = 1.0;
        self.avg.fill(1.0);
        self.avg_pos = 0;
        self.sum = self.avg_len as f64;
        self.below = 0;
    }

    /// The limiter's latency, in frames.
    pub fn delay(&self) -> usize {
        self.delay
    }

    /// Whether the gain is back at exactly 1 with nothing held.
    pub fn idle(&self) -> bool {
        self.below == 0 && self.env == 1.0 && self.hold.is_empty()
    }

    /// Frames it reduced the gain of since the last call.
    pub fn take_limited(&mut self) -> u64 {
        std::mem::take(&mut self.limited)
    }

    /// Delays `samples` (interleaved), limiting them when `detect` is set or
    /// while it still recovers from limiting.
    pub fn process(&mut self, samples: &mut [f64], detect: bool) {
        let ch = self.channels;
        let frames = self.frames;
        for frame in samples.chunks_exact_mut(ch) {
            let at = self.pos * ch;
            self.history[at..at + ch].copy_from_slice(frame);
            self.history[at + frames * ch..at + frames * ch + ch].copy_from_slice(frame);
            let gain = if detect || !self.idle() { self.gain() } else { 1.0 };
            let out = (self.pos + frames - self.delay) * ch;
            for (c, s) in frame.iter_mut().enumerate() {
                *s = self.history[out + c] * gain;
            }
            self.pos = (self.pos + 1) % frames;
            self.n += 1;
        }
    }

    /// The gain for the frame leaving the delay line now, from the gain
    /// the newest frame needs.
    ///
    /// The frame at index j needs gain r_j on the output frames that hold
    /// input j − DETECT_DELAY − MARGIN … j − DETECT_DELAY + MARGIN. Holding
    /// the minimum over `hold_len` and averaging over `avg_len` makes the
    /// gain at most r_j on the output frames j + avg_len − 1 … j + hold_len
    /// − 1, which hold exactly those inputs since
    /// delay = avg_len − 1 + DETECT_DELAY + MARGIN.
    fn gain(&mut self) -> f64 {
        let needed = self.needed();
        while self.hold.back().is_some_and(|&(_, v)| v >= needed) {
            self.hold.pop_back();
        }
        if needed < 1.0 {
            self.hold.push_back((self.n, needed));
        }
        while self.hold.front().is_some_and(|&(i, _)| i + self.hold_len as u64 <= self.n) {
            self.hold.pop_front();
        }
        let held = self.hold.front().map_or(1.0, |&(_, v)| v);

        if held < self.env {
            self.env = held;
        } else {
            self.env += (held - self.env) * self.release;
            // Close enough (−120 dB): land exactly, so an idle limiter is
            // bit-exact again.
            if held - self.env < 1e-6 {
                self.env = held;
            }
        }

        let old = std::mem::replace(&mut self.avg[self.avg_pos], self.env);
        self.avg_pos = (self.avg_pos + 1) % self.avg_len;
        self.below = self.below + usize::from(self.env < 1.0) - usize::from(old < 1.0);
        self.sum += self.env - old;
        if self.below == 0 {
            self.sum = self.avg_len as f64;
            return 1.0;
        }
        self.limited += 1;
        (self.sum / self.avg_len as f64).min(1.0)
    }

    /// The gain the newest frame needs: the ceiling over its largest
    /// interpolated peak (or sample), across channels.
    fn needed(&self) -> f64 {
        let ch = self.channels;
        // Newest `TAPS` frames, oldest first, contiguous.
        let newest = self.pos + self.frames;
        let window = &self.history[(newest + 1 - TAPS) * ch..(newest + 1) * ch];
        let mut peak = 0.0f64;
        for c in 0..ch {
            for phase in interpolator() {
                // Tap k weighs the frame k samples back from the newest.
                let v: f64 = phase.iter().enumerate().map(|(k, h)| h * window[(TAPS - 1 - k) * ch + c]).sum();
                peak = peak.max(v.abs());
            }
            peak = peak.max(window[(TAPS - 1 - DETECT_DELAY) * ch + c].abs());
        }
        if peak > CEILING { CEILING / peak } else { 1.0 }
    }
}

/// The 4× interpolator's taps, `[phase][k]` against the input k samples
/// back from the newest.
fn interpolator() -> &'static [[f64; TAPS]; 4] {
    static PHASES: OnceLock<[[f64; TAPS]; 4]> = OnceLock::new();
    PHASES.get_or_init(kaiser_phases)
}

/// A 47-tap sinc interpolator for 4× (cut-off at the input's Nyquist),
/// Kaiser-windowed (β = 6), split into four 12-tap phases, each scaled to
/// unity at DC. The odd length puts the phases on whole, quarter and half
/// samples (an even one leaves them an eighth off every peak).
fn kaiser_phases() -> [[f64; TAPS]; 4] {
    let len = 4 * TAPS - 1;
    let centre = (len - 1) as f64 / 2.0;
    let beta = 6.0;
    let i0 = |x: f64| {
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..50 {
            term *= (x / 2.0) / k as f64;
            sum += term * term;
        }
        sum
    };
    let mut h = vec![0.0; len];
    for (i, v) in h.iter_mut().enumerate() {
        let t = (i as f64 - centre) / 4.0;
        let sinc = if t == 0.0 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
        let r = (i as f64 - centre) / centre;
        *v = sinc * i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0(beta);
    }
    let mut phases = [[0.0; TAPS]; 4];
    for (p, phase) in phases.iter_mut().enumerate() {
        for (k, tap) in phase.iter_mut().enumerate() {
            *tap = h.get(k * 4 + p).copied().unwrap_or(0.0);
        }
        let dc: f64 = phase.iter().sum();
        phase.iter_mut().for_each(|t| *t /= dc);
    }
    phases
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// True peak by 32× oversampling with a long windowed sinc.
    fn true_peak(x: &[f64], ch: usize, c: usize) -> f64 {
        let s: Vec<f64> = x.iter().skip(c).step_by(ch).copied().collect();
        let half = 48isize;
        let mut peak = 0.0f64;
        for n in half as usize..s.len() - half as usize {
            for k in 0..32 {
                let t = n as f64 + k as f64 / 32.0;
                let mut v = 0.0;
                for m in -half..=half {
                    let idx = (n as isize + m) as usize;
                    let d = t - idx as f64;
                    let sinc = if d == 0.0 { 1.0 } else { (PI * d).sin() / (PI * d) };
                    let w = 0.5 + 0.5 * (PI * d / (half as f64 + 1.0)).cos();
                    v += s[idx] * sinc * w;
                }
                peak = peak.max(v.abs());
            }
        }
        peak
    }

    fn db(x: f64) -> f64 {
        20.0 * x.log10()
    }

    #[test]
    fn idle_output_is_the_input_delayed_bit_for_bit() {
        let mut l = Limiter::new(48_000.0, 2);
        let input: Vec<f64> = (0..9600).map(|n| ((n as f64) * 0.37).sin()).collect();
        let mut out = input.clone();
        l.process(&mut out, false);
        let d = l.delay() * 2;
        assert_eq!(&out[d..], &input[..input.len() - d]);
        assert!(out[..d].iter().all(|v| *v == 0.0));
        // Even with detection on, content under the ceiling passes exactly.
        let mut l = Limiter::new(48_000.0, 2);
        let quiet: Vec<f64> = input.iter().map(|v| v * 0.5).collect();
        let mut out = quiet.clone();
        l.process(&mut out, true);
        assert_eq!(&out[d..], &quiet[..quiet.len() - d]);
    }

    #[test]
    fn inter_sample_peaks_stay_under_the_ceiling() {
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            let mut l = Limiter::new(rate, 2);
            // fs/4 at 45°: samples at ±1, true peak √2 (+3 dB).
            let input: Vec<f64> = (0..(rate as usize / 10))
                .flat_map(|n| {
                    let v = (PI / 2.0 * n as f64 + PI / 4.0).sin() * std::f64::consts::SQRT_2;
                    [v, v]
                })
                .collect();
            let mut out = input.clone();
            l.process(&mut out, true);
            let tp = true_peak(&out, 2, 0);
            assert!(db(tp) <= db(CEILING) + 0.01, "{rate}: {:.3} dBTP", db(tp));
        }
    }

    #[test]
    fn the_gain_never_steps() {
        let rate = 48_000.0;
        let mut l = Limiter::new(rate, 1);
        // Quiet, then a burst at +6 dB over the ceiling, then quiet again.
        let input: Vec<f64> = (0..120_000)
            .map(|n| {
                let a = if (6_000..6_500).contains(&n) { 2.0 } else { 0.3 };
                a * (2.0 * PI * 997.0 * n as f64 / rate).sin()
            })
            .collect();
        let mut out = input.clone();
        l.process(&mut out, true);
        let d = l.delay();
        // The gain applied to each output sample, where the input is large
        // enough to read it from; steps only between neighbouring samples.
        let gain = |n: usize| (input[n - d].abs() > 0.05).then(|| out[n] / input[n - d]);
        let gains: Vec<(f64, f64)> = (d..input.len() - 1).filter_map(|n| Some((gain(n)?, gain(n + 1)?))).collect();
        let min = gains.iter().map(|g| g.0).fold(1.0, f64::min);
        let step = gains.iter().map(|(a, b)| (b - a).abs()).fold(0.0, f64::max);
        let bound = (1.0 - min) / l.avg_len as f64;
        assert!(step <= bound + 1e-12, "step {step} over {bound}");
        assert!(l.idle(), "recovered");
        // Once recovered, exact again.
        let tail = &out[out.len() - 100..];
        assert_eq!(tail, &input[input.len() - 100 - d..input.len() - d]);
    }

    #[test]
    fn full_scale_squares_come_out_at_the_ceiling() {
        let mut l = Limiter::new(48_000.0, 2);
        let input: Vec<f64> =
            (0..48_000).flat_map(|n| if (n / 24) % 2 == 0 { [1.0, 1.0] } else { [-1.0, -1.0] }).collect();
        let mut out = input.clone();
        l.process(&mut out, true);
        let peak = out.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(peak <= CEILING + 1e-12, "{peak}");
    }
}
