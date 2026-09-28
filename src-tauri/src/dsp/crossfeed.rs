//! Headphone crossfeed after bs2b, Boris Mikhaylov's Bauer
//! stereophonic-to-binaural DSP (libbs2b, MIT licensed,
//! <http://bs2b.sourceforge.net>). Each ear also hears the other channel
//! through a first-order low-pass, softer and later, as it would from a pair
//! of speakers; the presets are bs2b's (cut-off, and how far below the
//! direct sound the crossfed sound is at low frequencies).
//!
//! The direct path is a first-order high shelf that is the exact complement
//! of the cross path:
//!
//! ```text
//! cross(z)  = G · (1 − x) / (1 − x·z⁻¹)     low-pass, G at DC,  x = e^(−2π·fc/fs)
//! direct(z) = 1 − cross(z)                   high shelf, 1 − G at DC, → 1 up high
//! out_L = direct(L) + cross(R) = L − cross(L) + cross(R)
//! ```
//!
//! so a centred signal (L = R) comes out exactly as it went in, at every
//! frequency. The feed F (dB) sets G through cross/direct at DC,
//! G / (1 − G) = 10^(−F/20). libbs2b uses the same cut-offs and levels (a
//! hard-panned channel reaches the other ear at the same level at DC, e.g.
//! −8.56 dB for the default preset), but its shelf has a corner of its own
//! and it normalises only at DC, which leaves centred sound 1.1–1.8 dB duller
//! in the treble; this keeps its crossfeed without that tilt.
//!
//! Like bs2b, it can raise peaks: fast opposite-phase edges in the two
//! channels add up (by about +4.5 dB at worst for the default preset), so
//! the limiter runs while it is on.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CrossfeedPreset {
    /// bs2b's default: 700 Hz, 4.5 dB.
    #[default]
    Default,
    /// Chu Moy's: 700 Hz, 6 dB.
    ChuMoy,
    /// Jan Meier's: 650 Hz, 9.5 dB.
    JanMeier,
}

impl CrossfeedPreset {
    /// Low-pass cut-off (Hz) and feed level (dB).
    pub fn cut_and_feed(self) -> (f64, f64) {
        match self {
            Self::Default => (700.0, 4.5),
            Self::ChuMoy => (700.0, 6.0),
            Self::JanMeier => (650.0, 9.5),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Crossfeed {
    /// The low-pass pole.
    x: f64,
    /// The low-pass input gain, G · (1 − x).
    k: f64,
    /// cross(L) and cross(R) as last computed.
    lp: [f64; 2],
}

impl Crossfeed {
    pub fn new(preset: CrossfeedPreset, rate: f64) -> Self {
        let (cut, feed_db) = preset.cut_and_feed();
        let r = 10f64.powf(-feed_db / 20.0);
        let g = r / (1.0 + r);
        let x = (-2.0 * std::f64::consts::PI * cut / rate).exp();
        Self { x, k: g * (1.0 - x), lp: [0.0; 2] }
    }

    /// Takes the coefficients of `other` (another preset or rate) and keeps
    /// the memories, so a change mid-stream does not click.
    pub fn retune(&mut self, other: Crossfeed) {
        self.x = other.x;
        self.k = other.k;
    }

    /// Crossfeeds interleaved stereo samples in place.
    pub fn run(&mut self, samples: &mut [f64]) {
        for frame in samples.chunks_exact_mut(2) {
            let (l, r) = (frame[0], frame[1]);
            self.lp[0] = self.x * self.lp[0] + self.k * l;
            self.lp[1] = self.x * self.lp[1] + self.k * r;
            frame[0] = l - self.lp[0] + self.lp[1];
            frame[1] = r - self.lp[1] + self.lp[0];
        }
    }

    pub fn clear(&mut self) {
        self.lp = [0.0; 2];
    }

    pub fn flush_denormals(&mut self) {
        for v in &mut self.lp {
            if v.abs() < super::DENORMAL {
                *v = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Gain of each output channel for a sine at `freq` fed as (`left`,
    /// `right`) × the sine: the output's amplitude over the sine's, both by
    /// least squares over the same stretch, so the fit's own error cancels.
    fn response(preset: CrossfeedPreset, rate: f64, freq: f64, left: f64, right: f64) -> [f64; 2] {
        let mut c = Crossfeed::new(preset, rate);
        let n = rate as usize;
        let sine: Vec<f64> = (0..2 * n).map(|i| (2.0 * PI * freq * i as f64 / rate).sin()).collect();
        let mut samples: Vec<f64> = sine.iter().flat_map(|v| [left * v, right * v]).collect();
        c.run(&mut samples);
        let amplitude = |x: &dyn Fn(usize) -> f64| {
            let (mut s, mut co) = (0.0, 0.0);
            for i in n..2 * n {
                let w = 2.0 * PI * freq * i as f64 / rate;
                s += x(i) * w.sin();
                co += x(i) * w.cos();
            }
            (s * s + co * co).sqrt()
        };
        let reference = amplitude(&|i| sine[i]);
        [amplitude(&|i| samples[2 * i]) / reference, amplitude(&|i| samples[2 * i + 1]) / reference]
    }

    fn db(x: f64) -> f64 {
        20.0 * x.log10()
    }

    const PRESETS: [CrossfeedPreset; 3] =
        [CrossfeedPreset::Default, CrossfeedPreset::ChuMoy, CrossfeedPreset::JanMeier];

    /// Centred sound keeps its tone: the old crossfeed tilted it by about
    /// 2.1 dB from 50 Hz to 15 kHz, libbs2b by 1.1–1.7 dB.
    #[test]
    fn centred_sound_stays_flat() {
        for preset in PRESETS {
            for rate in [44_100.0, 48_000.0] {
                let levels: Vec<f64> = (0..=24)
                    .map(|i| 50.0 * 300f64.powf(f64::from(i) / 24.0))
                    .map(|f| db(response(preset, rate, f, 1.0, 1.0)[0]))
                    .collect();
                let deviation =
                    levels.iter().copied().fold(f64::MIN, f64::max) - levels.iter().copied().fold(f64::MAX, f64::min);
                assert!(deviation < 1e-6, "{preset:?} at {rate}: {deviation} dB from 50 Hz to 15 kHz");
            }
        }
    }

    /// A hard-panned channel reaches the other ear at bs2b's level at low
    /// frequencies, and hardly at all in the treble.
    #[test]
    fn a_hard_panned_channel_reaches_the_other_ear_below_it() {
        for (preset, feed) in
            [(CrossfeedPreset::Default, 4.5), (CrossfeedPreset::ChuMoy, 6.0), (CrossfeedPreset::JanMeier, 9.5)]
        {
            let [direct, cross] = response(preset, 48_000.0, 30.0, 1.0, 0.0);
            assert!((db(direct / cross) - feed).abs() < 0.1, "{preset:?}: {} dB apart", db(direct / cross));
            let [direct, cross] = response(preset, 48_000.0, 10_000.0, 1.0, 0.0);
            assert!(db(cross / direct) < -25.0, "{preset:?} leaks {} dB of treble", db(cross / direct));
        }
    }
}
