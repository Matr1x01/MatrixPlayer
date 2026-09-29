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

/// Switching on or off, or to another preset, glides over this long.
const GLIDE_S: f64 = 0.02;

/// The low-pass pole x and input gain G · (1 − x) of `preset` at `rate`.
fn coefficients(preset: CrossfeedPreset, rate: f64) -> (f64, f64) {
    let (cut, feed_db) = preset.cut_and_feed();
    let r = 10f64.powf(-feed_db / 20.0);
    let g = r / (1.0 + r);
    let x = (-2.0 * std::f64::consts::PI * cut / rate).exp();
    (x, g * (1.0 - x))
}

#[derive(Debug, Clone, Copy)]
pub struct Crossfeed {
    preset: CrossfeedPreset,
    rate: f64,
    /// The low-pass coefficients in use, and the preset's, glided to.
    x: f64,
    k: f64,
    x_to: f64,
    k_to: f64,
    glide: f64,
    /// How much crossfeed is heard (0 off, 1 on), gliding to `mix_to`.
    mix: f64,
    mix_to: f64,
    mix_step: f64,
    /// cross(L) and cross(R) as last computed.
    lp: [f64; 2],
}

impl Crossfeed {
    /// Crossfeed at `rate`, on with `preset` or off (`None`), already there.
    pub fn new(preset: Option<CrossfeedPreset>, rate: f64) -> Self {
        let on = preset.is_some();
        let preset = preset.unwrap_or_default();
        let (x, k) = coefficients(preset, rate);
        let mix = if on { 1.0 } else { 0.0 };
        Self {
            preset,
            rate,
            x,
            k,
            x_to: x,
            k_to: k,
            glide: 1.0 - (-1.0 / (GLIDE_S / 3.0 * rate)).exp(),
            mix,
            mix_to: mix,
            mix_step: 1.0 / (GLIDE_S * rate),
            lp: [0.0; 2],
        }
    }

    /// Moves to `preset`, or off (`None`): it glides in or out, and from one
    /// preset to another, so none of them clicks.
    pub fn set(&mut self, preset: Option<CrossfeedPreset>) {
        let Some(preset) = preset else {
            self.mix_to = 0.0;
            return;
        };
        self.preset = preset;
        (self.x_to, self.k_to) = coefficients(preset, self.rate);
        if !self.running() {
            // Coming in from silence: start clean, on the new preset.
            (self.x, self.k) = (self.x_to, self.k_to);
            self.lp = [0.0; 2];
        }
        self.mix_to = 1.0;
    }

    /// A new sample rate: the filter is redesigned and starts clean.
    pub fn set_format(&mut self, rate: f64) {
        let on = self.mix_to > 0.0;
        *self = Self::new(on.then_some(self.preset), rate);
    }

    /// Whether it affects the sound, now or while gliding out.
    pub fn running(&self) -> bool {
        self.mix > 0.0 || self.mix_to > 0.0
    }

    /// Crossfeeds interleaved stereo samples in place. A centred signal
    /// (L = R) passes exactly: both low-passes then hold the same value.
    pub fn run(&mut self, samples: &mut [f64]) {
        for frame in samples.chunks_exact_mut(2) {
            if self.mix != self.mix_to {
                self.mix = (self.mix + (self.mix_to - self.mix).clamp(-self.mix_step, self.mix_step)).clamp(0.0, 1.0);
            }
            if self.x != self.x_to || self.k != self.k_to {
                self.x += (self.x_to - self.x) * self.glide;
                self.k += (self.k_to - self.k) * self.glide;
                if (self.x_to - self.x).abs() < 1e-12 && (self.k_to - self.k).abs() < 1e-12 {
                    (self.x, self.k) = (self.x_to, self.k_to);
                }
            }
            let (l, r) = (frame[0], frame[1]);
            self.lp[0] = self.x * self.lp[0] + self.k * l;
            self.lp[1] = self.x * self.lp[1] + self.k * r;
            let across = self.mix * (self.lp[1] - self.lp[0]);
            frame[0] = l + across;
            frame[1] = r - across;
        }
        if !self.running() {
            // Out: nothing stale may come back when it is switched on again.
            self.lp = [0.0; 2];
        }
    }

    /// Forgets the signal (a flush or a new stream), landing any glide.
    pub fn clear(&mut self) {
        self.lp = [0.0; 2];
        self.mix = self.mix_to;
        (self.x, self.k) = (self.x_to, self.k_to);
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
        let mut c = Crossfeed::new(Some(preset), rate);
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
                assert!(deviation < 1e-9, "{preset:?} at {rate}: {deviation} dB from 50 Hz to 15 kHz");
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

    fn largest_step(samples: &[f64]) -> f64 {
        samples.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max)
    }

    /// Hard-panned bass (only the left channel), 997 Hz so changes land
    /// mid-waveform; returns the right channel, where the crossfeed is heard.
    fn right_ear(c: &mut Crossfeed, n: &mut usize, frames: usize) -> Vec<f64> {
        let mut samples: Vec<f64> =
            (*n..*n + frames).flat_map(|i| [0.8 * (2.0 * PI * 97.0 * i as f64 / 48_000.0).sin(), 0.0]).collect();
        *n += frames;
        c.run(&mut samples);
        samples.iter().skip(1).step_by(2).copied().collect()
    }

    /// Switching on, off, or to another preset glides: the far ear never
    /// steps.
    #[test]
    fn switching_glides_instead_of_stepping() {
        let mut c = Crossfeed::new(Some(CrossfeedPreset::Default), 48_000.0);
        let mut n = 0;
        right_ear(&mut c, &mut n, 48_000);
        let steady = largest_step(&right_ear(&mut c, &mut n, 4_801));
        for change in
            [None, Some(CrossfeedPreset::Default), Some(CrossfeedPreset::JanMeier), Some(CrossfeedPreset::ChuMoy)]
        {
            c.set(change);
            let heard = right_ear(&mut c, &mut n, 4_801);
            assert!(largest_step(&heard) <= steady * 1.05, "{change:?}: {} against {steady}", largest_step(&heard));
            right_ear(&mut c, &mut n, 24_000);
        }
    }

    /// Switched off, it keeps nothing: switching it on again during silence
    /// stays silent (it used to replay the old low-pass memory).
    #[test]
    fn nothing_stale_comes_back() {
        let mut c = Crossfeed::new(Some(CrossfeedPreset::Default), 48_000.0);
        let mut n = 0;
        right_ear(&mut c, &mut n, 4_800);
        c.set(None);
        right_ear(&mut c, &mut n, 4_800);
        assert!(!c.running());
        let mut silence = vec![0.0; 9_600];
        c.set(Some(CrossfeedPreset::ChuMoy));
        c.run(&mut silence);
        assert!(silence.iter().all(|s| *s == 0.0));
    }
}
