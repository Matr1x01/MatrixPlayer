//! Second-order IIR filters, run in 64-bit float.
//!
//! Bands are designed to match their analog prototypes (the Audio EQ
//! Cookbook's) all the way to Nyquist, where the cookbook's bilinear designs
//! bend away: see [`super::matched`]. The cookbook's own formulas remain
//! for what has no matched design here (a shelf whose Q is not Butterworth)
//! and for the rare band whose matched formulas give no real filter.
//! The UI draws its curve from these coefficients, so what is shown is
//! exactly what is heard.

use std::f64::consts::PI;

use super::matched;
use super::settings::{EqBand, FilterKind};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coeffs {
    /// Passes the signal through unchanged.
    pub const IDENTITY: Self = Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    /// Coefficients for `band` at sample rate `fs`: the matched design where
    /// there is one. A flat peak or shelf is exactly the identity.
    pub fn for_band(band: &EqBand, fs: f64) -> Self {
        let gain_kind = matches!(band.kind, FilterKind::Peak | FilterKind::LowShelf | FilterKind::HighShelf);
        if gain_kind && band.gain_db == 0.0 {
            return Self::IDENTITY;
        }
        // Kept just below Nyquist, where the designs are defined.
        let f0 = band.freq.min(fs * 0.49).max(1.0);
        let w0 = 2.0 * PI * f0 / fs;
        let q = band.q.max(0.05);
        let matched = match band.kind {
            FilterKind::Peak => matched::peak(w0, band.gain_db, q),
            FilterKind::LowPass => matched::low_pass(w0, q),
            FilterKind::HighPass => matched::high_pass(w0, q),
            FilterKind::Notch => matched::notch(w0, q),
            FilterKind::LowShelf | FilterKind::HighShelf if matched::is_butterworth(q) => {
                matched::shelf(2.0 * f0 / fs, 10f64.powf(band.gain_db / 20.0), band.kind == FilterKind::HighShelf)
            }
            FilterKind::LowShelf | FilterKind::HighShelf => None,
        };
        matched.unwrap_or_else(|| Self::cookbook(band, fs))
    }

    /// The Audio EQ Cookbook's bilinear design. The centre frequency is kept
    /// below Nyquist so high bands stay stable at low sample rates.
    pub fn cookbook(band: &EqBand, fs: f64) -> Self {
        let f0 = band.freq.min(fs * 0.45).max(1.0);
        let w0 = 2.0 * PI * f0 / fs;
        let (sin, cos) = w0.sin_cos();
        let q = band.q.max(0.05);
        let alpha = sin / (2.0 * q);
        let a = 10f64.powf(band.gain_db / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match band.kind {
            FilterKind::Peak => {
                (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
            }
            FilterKind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) + (a - 1.0) * cos + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - s,
                )
            }
            FilterKind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - s),
                    (a + 1.0) - (a - 1.0) * cos + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - s,
                )
            }
            FilterKind::LowPass => {
                ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            FilterKind::HighPass => {
                ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            FilterKind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// Gain in dB at frequency `f`.
    pub fn magnitude_db(&self, f: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let num = num_re * num_re + num_im * num_im;
        let den = den_re * den_re + den_im * den_im;
        10.0 * (num / den).max(1e-30).log10()
    }
}

/// Filter memory for one channel (transposed direct form II, which behaves
/// well numerically in floating point).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct State {
    z1: f64,
    z2: f64,
}

impl State {
    #[inline]
    pub fn run(&mut self, c: &Coeffs, x: f64) -> f64 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }

    /// Zeroes memories too small to matter, before they turn subnormal.
    pub fn flush_denormals(&mut self) {
        for z in [&mut self.z1, &mut self.z2] {
            if z.abs() < super::DENORMAL {
                *z = 0.0;
            }
        }
    }
}

/// The largest boost the filters `coeffs` add anywhere from 20 Hz up to
/// Nyquist (what "auto preamp" compensates), sampled on a log grid and at
/// `centres`, the bands' own frequencies (a peak is at its highest there,
/// however narrow).
pub fn max_boost_db(coeffs: &[Coeffs], centres: &[f64], fs: f64) -> f64 {
    if coeffs.is_empty() {
        return 0.0;
    }
    let top = 0.499 * fs;
    (0u32..)
        .map(|i| 20.0 * 1000f64.powf(f64::from(i) / 240.0))
        .take_while(|f| *f < top)
        .chain([top])
        .chain(centres.iter().map(|f| f.min(top)))
        .map(|f| coeffs.iter().map(|c| c.magnitude_db(f, fs)).sum::<f64>())
        .fold(f64::MIN, f64::max)
        .max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f64 = 48_000.0;

    #[test]
    fn peak_filters_hit_their_gain_at_the_centre_and_stay_flat_far_away() {
        let band = EqBand::peak(1000.0, 6.0, 1.0);
        let c = Coeffs::for_band(&band, FS);
        assert!((c.magnitude_db(1000.0, FS) - 6.0).abs() < 0.01);
        assert!(c.magnitude_db(20.0, FS).abs() < 0.1);
        assert!(c.magnitude_db(20_000.0, FS).abs() < 0.2);
    }

    #[test]
    fn shelves_reach_their_gain_on_their_side() {
        let low = EqBand { enabled: true, kind: FilterKind::LowShelf, freq: 100.0, gain_db: 4.0, q: 0.7 };
        let c = Coeffs::for_band(&low, FS);
        assert!((c.magnitude_db(20.0, FS) - 4.0).abs() < 0.2);
        assert!(c.magnitude_db(10_000.0, FS).abs() < 0.05);
        let high = EqBand { kind: FilterKind::HighShelf, freq: 8000.0, gain_db: -3.0, ..low };
        let c = Coeffs::for_band(&high, FS);
        assert!((c.magnitude_db(20_000.0, FS) + 3.0).abs() < 0.3);
        assert!(c.magnitude_db(100.0, FS).abs() < 0.05);
    }

    #[test]
    fn filtering_a_sine_matches_the_predicted_gain() {
        let band = EqBand::peak(1000.0, -6.0, 2.0);
        let c = Coeffs::for_band(&band, FS);
        let mut state = State::default();
        let mut peak: f64 = 0.0;
        for n in 0..48_000 {
            let x = (2.0 * PI * 1000.0 * f64::from(n) / FS).sin();
            let y = state.run(&c, x);
            if n > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        assert!((20.0 * peak.log10() + 6.0).abs() < 0.05, "measured {peak}");
    }

    #[test]
    fn max_boost_finds_the_peak_of_the_combined_curve() {
        let coeffs = |bands: &[EqBand]| bands.iter().map(|b| Coeffs::for_band(b, FS)).collect::<Vec<_>>();
        let bands = [EqBand::peak(1000.0, 3.0, 1.0), EqBand::peak(1100.0, 2.0, 1.0), EqBand::peak(100.0, -8.0, 1.0)];
        let boost = max_boost_db(&coeffs(&bands), &[], FS);
        assert!((4.8..5.0).contains(&boost), "{boost}");
        assert_eq!(max_boost_db(&coeffs(&[EqBand::peak(500.0, -3.0, 1.0)]), &[], FS), 0.0);
        // A peak above 20 kHz, or too narrow for the grid, is still found.
        let high = [Coeffs::for_band(&EqBand::peak(21_500.0, 12.0, 2.0), 44_100.0)];
        assert!((max_boost_db(&high, &[21_500.0], 44_100.0) - 12.0).abs() < 0.01);
        let narrow = [EqBand::peak(1_234.0, 9.0, 20.0)];
        assert!((max_boost_db(&coeffs(&narrow), &[1_234.0], FS) - 9.0).abs() < 1e-6);
    }

    #[test]
    fn bands_above_nyquist_stay_stable() {
        let band = EqBand::peak(20_000.0, 12.0, 0.5);
        let c = Coeffs::for_band(&band, 22_050.0);
        let mut state = State::default();
        let out: Vec<f64> = (0..10_000).map(|n| state.run(&c, if n == 0 { 1.0 } else { 0.0 })).collect();
        assert!(out.iter().all(|y| y.is_finite()));
        assert!(out[9_000..].iter().all(|y| y.abs() < 1e-6), "impulse response decays");
    }
}
