//! Biquads whose magnitude follows the analog prototype up to Nyquist.
//!
//! The cookbook's bilinear transform squeezes everything towards Nyquist: a
//! 16 kHz peak at 44.1 kHz comes out almost 4 dB short at 20 kHz. These
//! designs keep the analog shape instead:
//!
//! - peak, low pass and high pass: M. Vicanek, "Matched Second Order Digital
//!   Filters" (2016, <https://vicanek.de/articles/BiquadFits.pdf>). The poles
//!   come from impulse invariance (eq. 12), the numerator from matching the
//!   magnitude at DC and at the centre frequency (sections 4.1, 4.2, 4.4);
//! - low and high shelves: M. Vicanek, "Matched Two-Pole Digital Shelving
//!   Filters" (2024, rev. 2025, <https://vicanek.de/articles/2poleShelvingFits.pdf>),
//!   its appendix A for second-order Butterworth shelves (Q = 1/√2). A shelf
//!   with another Q has no matched design there and keeps the cookbook's;
//! - notch: in neither paper. Derived the way the first paper works: poles
//!   by impulse invariance, the zeros exactly where the analog ones map
//!   (e^±iω₀, so the notch is total), scaled for unity gain at DC.
//!
//! The analog prototypes are the cookbook's, so a band means the same thing
//! as before; only its digital shape changes, and only near Nyquist. Each
//! design returns `None` where its formulas give no real filter (a low pass
//! far below 20 Hz with a very high Q); the caller then uses the cookbook.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use super::biquad::Coeffs;

/// Poles of s² + 2qω₀s + ω₀² by impulse invariance, q = 1/(2Q) (eq. 11–12).
fn poles(w0: f64, q_factor: f64) -> (f64, f64) {
    let q = 1.0 / (2.0 * q_factor);
    let a1 = if q <= 1.0 {
        -2.0 * (-q * w0).exp() * ((1.0 - q * q).sqrt() * w0).cos()
    } else {
        -2.0 * (-q * w0).exp() * ((q * q - 1.0).sqrt() * w0).cosh()
    };
    (a1, (-2.0 * q * w0).exp())
}

/// φ₀, φ₁, φ₂ at ω (eq. 26).
fn phis(w: f64) -> (f64, f64, f64) {
    let p1 = (w / 2.0).sin().powi(2);
    let p0 = 1.0 - p1;
    (p0, p1, 4.0 * p0 * p1)
}

/// A₀, A₁, A₂ of the poles (eq. 27).
fn pole_terms(a1: f64, a2: f64) -> (f64, f64, f64) {
    ((1.0 + a1 + a2).powi(2), (1.0 - a1 + a2).powi(2), -4.0 * a2)
}

/// The minimum-phase numerator with |numerator|² given by B₀, B₁, B₂
/// (eq. 29); `sqrt_b0` is √B₀, known in closed form.
fn numerator(b1_term: f64, b2_term: f64, sqrt_b0: f64) -> Option<(f64, f64, f64)> {
    if b1_term < 0.0 {
        return None;
    }
    let sqrt_b1 = b1_term.sqrt();
    let w = 0.5 * (sqrt_b0 + sqrt_b1);
    let d = w * w + b2_term;
    if d < 0.0 {
        return None;
    }
    let b0 = 0.5 * (w + d.sqrt());
    Some((b0, 0.5 * (sqrt_b0 - sqrt_b1), -b2_term / (4.0 * b0)))
}

fn coeffs(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Option<Coeffs> {
    let c = Coeffs { b0, b1, b2, a1, a2 };
    [b0, b1, b2, a1, a2].iter().all(|v| v.is_finite()).then_some(c)
}

/// Peaking EQ with linear gain G at ω₀ (section 4.4, eq. 42–45). The
/// prototype's poles have a Q of √G·Q.
pub fn peak(w0: f64, gain_db: f64, q: f64) -> Option<Coeffs> {
    let g = 10f64.powf(gain_db / 20.0);
    let (a1, a2) = poles(w0, q * g.sqrt());
    let (big_a0, big_a1, big_a2) = pole_terms(a1, a2);
    let (p0, p1, p2) = phis(w0);
    let r1 = (big_a0 * p0 + big_a1 * p1 + big_a2 * p2) * g * g;
    let r2 = (-big_a0 + big_a1 + 4.0 * (p0 - p1) * big_a2) * g * g;
    let big_b0 = big_a0;
    let big_b2 = (r1 - r2 * p1 - big_b0) / (4.0 * p1 * p1);
    let big_b1 = r2 + big_b0 + 4.0 * (p1 - p0) * big_b2;
    let (b0, b1, b2) = numerator(big_b1, big_b2, 1.0 + a1 + a2)?;
    coeffs(b0, b1, b2, a1, a2)
}

/// Low pass: unity at DC, |H(ω₀)| = Q, one zero (section 4.1, eq. 30–34).
pub fn low_pass(w0: f64, q: f64) -> Option<Coeffs> {
    let (a1, a2) = poles(w0, q);
    let (big_a0, big_a1, big_a2) = pole_terms(a1, a2);
    let (p0, p1, p2) = phis(w0);
    let r1 = (big_a0 * p0 + big_a1 * p1 + big_a2 * p2) * q * q;
    let big_b1 = (r1 - big_a0 * p0) / p1;
    if big_b1 < 0.0 {
        return None;
    }
    let sqrt_b0 = 1.0 + a1 + a2;
    let b0 = 0.5 * (sqrt_b0 + big_b1.sqrt());
    coeffs(b0, sqrt_b0 - b0, 0.0, a1, a2)
}

/// High pass: double zero at DC, |H(ω₀)| = Q (section 4.2, eq. 35–36).
pub fn high_pass(w0: f64, q: f64) -> Option<Coeffs> {
    let (a1, a2) = poles(w0, q);
    let (big_a0, big_a1, big_a2) = pole_terms(a1, a2);
    let (p0, p1, p2) = phis(w0);
    let b0 = (big_a0 * p0 + big_a1 * p1 + big_a2 * p2).sqrt() * q / (4.0 * p1);
    coeffs(b0, -2.0 * b0, b0, a1, a2)
}

/// Notch (derived, see the module notes): impulse-invariant poles, zeros on
/// the unit circle at ±ω₀, unity at DC.
pub fn notch(w0: f64, q: f64) -> Option<Coeffs> {
    let (a1, a2) = poles(w0, q);
    let b0 = (1.0 + a1 + a2) / (2.0 - 2.0 * w0.cos());
    coeffs(b0, -2.0 * w0.cos() * b0, b0, a1, a2)
}

/// Whether a shelf's Q is the Butterworth one the shelving design is for.
pub fn is_butterworth(q: f64) -> bool {
    shelf_design(q) == 0
}

/// Which design a shelf with this Q gets: 0 the matched Butterworth one,
/// −1 and 1 the cookbook's below and above it. The two differ near Nyquist,
/// so the EQ crossfades rather than glides a shelf's Q from one to another.
pub fn shelf_design(q: f64) -> i8 {
    if (q - FRAC_1_SQRT_2).abs() <= 0.01 {
        0
    } else if q < FRAC_1_SQRT_2 {
        -1
    } else {
        1
    }
}

/// Butterworth shelf with linear gain `gain` above (`high`) or below the
/// centre frequency `fc` (in units of Nyquist): appendix A.1 and A.2.
pub fn shelf(fc: f64, gain: f64, high: bool) -> Option<Coeffs> {
    // The low shelf is the high shelf of the inverse gain, scaled back up.
    let g = if high { gain } else { 1.0 / gain };
    let inv_g = 1.0 / g;
    let fc4 = fc.powi(4);
    // Squared magnitude of the analog shelf at frequency f (units of Nyquist).
    let analog = |f4: f64| (fc4 + f4 * g) / (fc4 + f4 * inv_g);
    let h_ny = analog(1.0);
    // The two matching frequencies the paper proposes (eq. 11).
    let f1 = fc / (0.160 + 1.543 * fc * fc).sqrt();
    let f2 = fc / (0.947 + 3.806 * fc * fc).sqrt();
    let (h1, h2) = (analog(f1.powi(4)), analog(f2.powi(4)));
    let (phi1, phi2) = ((PI / 2.0 * f1).sin().powi(2), (PI / 2.0 * f2).sin().powi(2));
    // Eq. 12–15.
    let d1 = (h1 - 1.0) * (1.0 - phi1);
    let c11 = -phi1 * d1;
    let c12 = phi1 * phi1 * (h_ny - h1);
    let d2 = (h2 - 1.0) * (1.0 - phi2);
    let c21 = -phi2 * d2;
    let c22 = phi2 * phi2 * (h_ny - h2);
    let alpha1 = (c22 * d1 - c12 * d2) / (c11 * c22 - c12 * c21);
    let aa1 = (d1 - c11 * alpha1) / c12;
    let bb1 = h_ny * aa1;
    let aa2 = 0.25 * (alpha1 - aa1);
    let bb2 = 0.25 * (alpha1 - bb1);
    // Eq. 5, normalised by a₀.
    let v = 0.5 * (1.0 + aa1.sqrt());
    let w = 0.5 * (1.0 + bb1.sqrt());
    let a0 = 0.5 * (v + (v * v + aa2).sqrt());
    let b0 = 0.5 * (w + (w * w + bb2).sqrt());
    let scale = if high { 1.0 } else { inv_g } / a0;
    coeffs(b0 * scale, (1.0 - w) * scale, -0.25 * bb2 / b0 * scale, (1.0 - v) / a0, -0.25 * aa2 / (a0 * a0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [f64; 3] = [44_100.0, 48_000.0, 96_000.0];

    /// The cookbook's analog prototypes (s normalised to ω₀), in dB.
    fn analog_peak(f: f64, f0: f64, gain_db: f64, q: f64) -> f64 {
        let a = 10f64.powf(gain_db / 40.0);
        let x = f / f0;
        let num = (1.0 - x * x).powi(2) + (x * a / q).powi(2);
        let den = (1.0 - x * x).powi(2) + (x / (a * q)).powi(2);
        10.0 * (num / den).log10()
    }

    fn analog_shelf(f: f64, f0: f64, gain_db: f64, high: bool) -> f64 {
        // Paper eq. 1: |H|² = (1 + G·x⁴) / (1 + x⁴/G) for the high shelf (√G at
        // f0); the low shelf is G² times the high shelf of 1/G.
        let g = 10f64.powf(gain_db / 20.0);
        let x4 = (f / f0).powi(4);
        let h2 = if high { (1.0 + g * x4) / (1.0 + x4 / g) } else { g * g * (1.0 + x4 / g) / (1.0 + g * x4) };
        10.0 * h2.log10()
    }

    fn w(f: f64, fs: f64) -> f64 {
        2.0 * PI * f / fs
    }

    #[test]
    fn a_high_peak_keeps_its_analog_shape_near_nyquist() {
        for fs in RATES {
            let c = peak(w(16_000.0, fs), 6.0, 1.41).unwrap();
            let error = c.magnitude_db(20_000.0, fs) - analog_peak(20_000.0, 16_000.0, 6.0, 1.41);
            assert!(error.abs() < 0.5, "{fs}: {error:.3} dB off the analog peak at 20 kHz");
        }
    }

    #[test]
    fn peaks_hit_their_gain_at_the_centre_and_unity_at_dc() {
        for (f0, db, q) in [(1_000.0, 6.0, 1.0), (60.0, -12.0, 4.0), (12_000.0, 9.0, 0.7), (18_000.0, -20.0, 2.0)] {
            let c = peak(w(f0, 48_000.0), db, q).unwrap();
            assert!((c.magnitude_db(f0, 48_000.0) - db).abs() < 1e-6, "{f0} Hz");
            assert!(c.magnitude_db(1.0, 48_000.0).abs() < 0.01, "{f0} Hz at DC");
        }
    }

    #[test]
    fn pass_filters_match_q_at_their_corner() {
        for (f0, q) in [(100.0, 0.707), (5_000.0, 3.0), (18_000.0, 0.707)] {
            let lp = low_pass(w(f0, 44_100.0), q).unwrap();
            let hp = high_pass(w(f0, 44_100.0), q).unwrap();
            let q_db = 20.0 * q.log10();
            assert!((lp.magnitude_db(f0, 44_100.0) - q_db).abs() < 1e-6);
            assert!((hp.magnitude_db(f0, 44_100.0) - q_db).abs() < 1e-6);
            assert!(lp.magnitude_db(1.0, 44_100.0).abs() < 1e-6, "low pass unity at DC");
        }
    }

    #[test]
    fn butterworth_shelves_follow_the_analog_curve() {
        // The paper: within 1 dB for a 20 dB shelf, even near Nyquist.
        for fs in RATES {
            for (f0, db, high) in
                [(100.0, 4.0, false), (8_000.0, -3.0, true), (12_000.0, 20.0, true), (200.0, -20.0, false)]
            {
                let c = shelf(2.0 * f0 / fs, 10f64.powf(db / 20.0), high).unwrap();
                let worst = (0..=200)
                    .map(|i| 20.0 * 1000f64.powf(f64::from(i) / 200.0))
                    .filter(|f| *f < fs / 2.0)
                    .map(|f| (c.magnitude_db(f, fs) - analog_shelf(f, f0, db, high)).abs())
                    .fold(0.0, f64::max);
                assert!(worst < 1.0, "{fs}: {f0} Hz {db} dB shelf off by {worst:.2} dB");
            }
        }
    }

    #[test]
    fn notches_are_total_and_leave_dc_alone() {
        let c = notch(w(1_000.0, 48_000.0), 4.0).unwrap();
        assert!(c.magnitude_db(1_000.0, 48_000.0) < -150.0);
        assert!(c.magnitude_db(0.0, 48_000.0).abs() < 1e-9);
    }

    /// Every design stays stable across the whole parameter range, and only
    /// a few extreme low passes need the cookbook.
    #[test]
    fn every_design_is_stable_everywhere() {
        let stable = |c: &Coeffs| c.a2.abs() < 1.0 && c.a1.abs() < 1.0 + c.a2;
        let mut fallbacks = 0;
        for fs in RATES {
            let mut f0 = 10.0;
            while f0 < 0.499 * fs {
                let w0 = w(f0, fs);
                for q in [0.1, 0.5, FRAC_1_SQRT_2, 1.41, 4.32, 20.0] {
                    for db in [-24.0, -6.0, -0.1, 0.1, 6.0, 12.0] {
                        assert!(peak(w0, db, q).is_some_and(|c| stable(&c)), "peak {f0} {q} {db} at {fs}");
                    }
                    for c in [low_pass(w0, q), high_pass(w0, q), notch(w0, q)] {
                        match c {
                            Some(c) => assert!(stable(&c), "{f0} {q} at {fs}"),
                            None => fallbacks += 1,
                        }
                    }
                }
                for db in [-24.0, -6.0, 6.0, 12.0] {
                    for high in [false, true] {
                        let c = shelf(2.0 * f0 / fs, 10f64.powf(db / 20.0), high);
                        assert!(c.is_some_and(|c| stable(&c)), "shelf {f0} {db} {high} at {fs}");
                    }
                }
                f0 *= 1.1;
            }
        }
        assert!(fallbacks < 20, "{fallbacks} designs fell back");
    }
}
