//! Graphic EQ bands whose sum is what the sliders show.
//!
//! Neighbouring bands overlap, so setting each filter to its slider's gain
//! overshoots (every slider at +6 dB peaked at +8.8 dB) and blurs
//! alternating sliders into each other (±6 dB came out near ±4 dB). The
//! filter gains are solved for instead, after V. Välimäki and J. Liski,
//! "Accurate Cascade Graphic Equalizer", IEEE Signal Processing Letters
//! 24(2), 2017, and its third-octave follow-up (Liski and Välimäki, "The
//! Quest for the Best Graphic Equalizer", DAFx-17):
//!
//! - Each band is a peaking filter whose gain at its lower neighbour's
//!   centre is c times its own gain in dB: c = 0.3 for octaves, as in the
//!   paper; 0.35 for thirds, where the authors use 0.4 with bilinear bands
//!   (with bands matched to the analog shape, 0.4 lets the top bands overlap
//!   too much near Nyquist at 44.1 kHz; 0.35 measured best). The paper sets
//!   this on Orfanidis'
//!   bilinear peak (eq. 6–7); here the band filters are matched to their
//!   analog prototypes (see [`super::matched`]), so the rule is applied to
//!   the analog peak itself, whose Q then depends on the gain (see
//!   [`GraphicEq::band`]). That also keeps the c-points on the neighbours
//!   near Nyquist, where the paper had to set bandwidths by hand.
//! - The interaction matrix B holds each band's response in dB, per dB of a
//!   17 dB prototype gain, at the band centres and the geometric midpoints
//!   between them (2N − 1 points; eq. 8).
//! - The gains g = B⁺t for the slider targets t (midpoints interpolated) are
//!   refined once with a matrix built from those gains (eq. 9–10), since a
//!   filter's shape changes with its gain.
//!
//! The solved gains reach about ±23 dB (10 bands) and ±30 dB (31 bands) for
//! slider settings within ±12 dB; they are only clamped at ±30 dB, since
//! clamping to the sliders' range would undo the correction.
//!
//! Everything here depends on the sample rate (bands above 95% of Nyquist
//! are left out), so a solver is built per layout and rate and cached; a
//! solve reuses its scratch space and allocates nothing.

use serde::Serialize;

use super::biquad::{Coeffs, added_loudness_db, max_boost_db};
use super::settings::{EqBand, EqCurve, EqMode, FilterKind, GRAPHIC_10, GRAPHIC_31};

/// Gain (dB) the interaction matrix is sampled at (paper, Sec. III-A).
const PROTOTYPE_DB: f64 = 17.0;
/// Solved gains are kept within this (dB): a sanity bound, not a design
/// limit (the 31-band design needs up to about ±30 dB).
pub const FILTER_LIMIT_DB: f64 = 30.0;
/// Bands at or above this share of Nyquist cannot be shaped and are left out.
const TOP_BAND: f64 = 0.95;

/// The filter slots of `curve` at `rate`: its EQ bands, graphic ones solved
/// with `cache`, then bass and treble.
pub fn curve_slots(curve: &EqCurve, rate: f64, cache: &mut GraphicCache) -> Vec<EqBand> {
    let mut slots = match curve.mode {
        EqMode::Parametric => curve.parametric.clone(),
        EqMode::Graphic10 => cache.solver(&GRAPHIC_10, 0.3, rate).bands(&curve.graphic10),
        EqMode::Graphic31 => cache.solver(&GRAPHIC_31, 0.35, rate).bands(&curve.graphic31),
    };
    slots.extend(curve.tone_slots());
    slots
}

/// A curve's frequency response as the processor would play it, for
/// drawing: the UI shows exactly what is heard, at the stream's rate.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqResponse {
    /// The whole curve (dB) at each frequency asked for.
    pub total: Vec<f64>,
    /// Each parametric band on its own (dB), for the selected band's curve;
    /// empty in the graphic modes.
    pub bands: Vec<Vec<f64>>,
    /// The largest boost anywhere in the audible range (what auto preamp
    /// takes back beyond its allowance).
    pub max_boost_db: f64,
    /// The loudness the curve adds to music (auto preamp takes back at
    /// least this).
    pub loudness_db: f64,
}

/// The response of `curve` at `rate`, at each of `freqs` (Hz). Frequencies
/// beyond Nyquist read as the response just below it.
pub fn curve_response(curve: &EqCurve, rate: f64, freqs: &[f64], cache: &mut GraphicCache) -> EqResponse {
    let slots = curve_slots(curve, rate, cache);
    let live: Vec<(usize, Coeffs)> = slots
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            b.enabled
                && !(b.gain_db == 0.0
                    && matches!(b.kind, FilterKind::Peak | FilterKind::LowShelf | FilterKind::HighShelf))
        })
        .map(|(i, b)| (i, Coeffs::for_band(b, rate)))
        .collect();
    let at = |c: &Coeffs, f: f64| c.magnitude_db(f.min(rate * 0.4999), rate);
    let total = freqs.iter().map(|&f| live.iter().map(|(_, c)| at(c, f)).sum()).collect();
    let bands = match curve.mode {
        EqMode::Parametric => (0..curve.parametric.len())
            .map(|i| {
                let band = EqBand { enabled: true, ..slots[i] };
                let c = Coeffs::for_band(&band, rate);
                freqs.iter().map(|&f| at(&c, f)).collect()
            })
            .collect(),
        _ => Vec::new(),
    };
    let centres: Vec<f64> = live.iter().map(|(i, _)| slots[*i].freq).collect();
    let coeffs: Vec<Coeffs> = live.into_iter().map(|(_, c)| c).collect();
    EqResponse {
        total,
        bands,
        max_boost_db: max_boost_db(&coeffs, &centres, rate),
        loudness_db: added_loudness_db(&coeffs, rate),
    }
}

/// The solver last used, rebuilt when the layout or the rate changes.
#[derive(Default)]
pub struct GraphicCache(Option<GraphicEq>);

impl GraphicCache {
    pub fn solver(&mut self, centres: &'static [f64], c: f64, rate: f64) -> &mut GraphicEq {
        let fits = self.0.as_ref().is_some_and(|s| std::ptr::eq(s.centres, centres) && s.rate == rate);
        if !fits {
            self.0 = Some(GraphicEq::new(centres, c, rate));
        }
        self.0.as_mut().expect("just built")
    }
}

/// The accurate cascade design for one layout at one sample rate.
pub struct GraphicEq {
    centres: &'static [f64],
    rate: f64,
    c: f64,
    /// Bands below the top limit: the ones solved for.
    active: usize,
    /// Each band's lower neighbour, as a share of its own centre frequency
    /// (the first band gets a virtual one, a step below).
    below: Vec<f64>,
    /// Design frequencies (Hz): centres and midpoints, interleaved.
    design: Vec<f64>,
    /// (B₀ᵀB₀)⁻¹B₀ᵀ for the prototype matrix, `active` × K, row-major.
    first: Vec<f64>,
    // Scratch, sized once: a solve allocates nothing.
    target: Vec<f64>,
    estimate: Vec<f64>,
    matrix: Vec<f64>,
    normal: Vec<f64>,
    solution: Vec<f64>,
}

impl GraphicEq {
    pub fn new(centres: &'static [f64], c: f64, rate: f64) -> Self {
        let active = centres.iter().take_while(|f| **f < TOP_BAND * rate / 2.0).count();
        let k = (2 * active).saturating_sub(1);
        let below = (0..centres.len())
            .map(|m| if m > 0 { centres[m - 1] / centres[m] } else { centres[0] / centres[1] })
            .collect();
        let design = (0..k)
            .map(|i| if i % 2 == 0 { centres[i / 2] } else { (centres[i / 2] * centres[i / 2 + 1]).sqrt() })
            .collect();
        let mut eq = Self {
            centres,
            rate,
            c,
            active,
            below,
            design,
            first: vec![0.0; active * k],
            target: vec![0.0; k],
            estimate: vec![0.0; active],
            matrix: vec![0.0; k * active],
            normal: vec![0.0; active * active],
            solution: vec![0.0; active],
        };
        eq.prepare();
        eq
    }

    /// Precomputes the first estimate's solver from the prototype matrix.
    fn prepare(&mut self) {
        let (m, k) = (self.active, self.design.len());
        if m == 0 {
            return;
        }
        self.fill_matrix(false);
        self.fill_normal();
        // Columns of (BᵀB)⁻¹Bᵀ: solve BᵀB x = (row k of B) for each k.
        let normal = self.normal.clone();
        for row in 0..k {
            let mut x: Vec<f64> = (0..m).map(|j| self.matrix[row * m + j]).collect();
            let mut a = normal.clone();
            if cholesky_solve(&mut a, &mut x, m) {
                for (j, v) in x.iter().enumerate() {
                    self.first[j * k + row] = *v;
                }
            }
        }
    }

    /// The band filters for `sliders` (one per centre, dB).
    pub fn bands(&mut self, sliders: &[f64]) -> Vec<EqBand> {
        let mut gains = vec![0.0; self.centres.len()];
        self.solve(sliders, &mut gains);
        (0..self.centres.len()).map(|m| self.band(m, gains[m])).collect()
    }

    /// Filter gains (dB) whose cascade hits `sliders` at the band centres.
    pub fn solve(&mut self, sliders: &[f64], gains: &mut [f64]) {
        gains.fill(0.0);
        let (m, k) = (self.active, self.design.len());
        if m == 0 || sliders.iter().take(m).all(|s| *s == 0.0) {
            // All flat: exactly flat filters, which the EQ skips.
            return;
        }
        for i in 0..k {
            let s = |b: usize| sliders.get(b).copied().unwrap_or(0.0);
            self.target[i] = if i % 2 == 0 { s(i / 2) } else { (s(i / 2) + s(i / 2 + 1)) / 2.0 };
        }
        // First estimate with the prototype matrix (eq. 9).
        for j in 0..m {
            self.estimate[j] = (0..k).map(|i| self.first[j * k + i] * self.target[i]).sum();
        }
        gains[..m].copy_from_slice(&self.estimate);
        // One refinement with each filter at its own gain (eq. 10).
        self.fill_matrix(true);
        self.fill_normal();
        for j in 0..m {
            self.solution[j] = (0..k).map(|i| self.matrix[i * m + j] * self.target[i]).sum();
        }
        if cholesky_solve(&mut self.normal, &mut self.solution, m) && self.solution.iter().all(|g| g.is_finite()) {
            gains[..m].copy_from_slice(&self.solution);
        }
        for g in gains.iter_mut() {
            *g = g.clamp(-FILTER_LIMIT_DB, FILTER_LIMIT_DB);
        }
    }

    /// Band `m` as a filter with gain `gain_db`: the peak whose analog gain
    /// at the lower neighbour (x = f_below / f₀) is c · `gain_db`. With the
    /// analog peak's |H(ix)|² = (u + G·v) / (u + v/G), u = (1 − x²)²,
    /// v = x²/Q² and G the linear gain, setting it to G^2c gives
    /// Q = x · √((G − G^2c/G) / (u · (G^2c − 1))); as the gain goes to 0 this
    /// tends to x · √((1 − c)/c) / |1 − x²|, so the Q glides smoothly.
    pub fn band(&self, m: usize, gain_db: f64) -> EqBand {
        let x = self.below[m];
        let u = (1.0 - x * x).powi(2);
        let ratio = if gain_db.abs() < 1e-9 {
            (1.0 - self.c) / self.c
        } else {
            let g = 10f64.powf(gain_db / 20.0);
            let t = g.powf(2.0 * self.c);
            (g - t / g) / (t - 1.0)
        };
        let q = x * (ratio / u).sqrt();
        EqBand { enabled: true, kind: FilterKind::Peak, freq: self.centres[m], gain_db, q }
    }

    /// B[k][j] = band j's response (dB) at design frequency k, per dB of the
    /// gain it is sampled at: the prototype gain, or with `refine` the first
    /// estimate's (kept away from 0, where it would divide by nothing).
    fn fill_matrix(&mut self, refine: bool) {
        let m = self.active;
        for j in 0..m {
            let g = match self.estimate[j] {
                _ if !refine => PROTOTYPE_DB,
                g if g.abs() < 1e-3 => 1e-3f64.copysign(g),
                g => g,
            };
            let coeffs = Coeffs::for_band(&self.band(j, g), self.rate);
            for (i, f) in self.design.iter().enumerate() {
                self.matrix[i * m + j] = (coeffs.magnitude_db(*f, self.rate) / g).abs();
            }
        }
    }

    fn fill_normal(&mut self) {
        let (m, k) = (self.active, self.design.len());
        for a in 0..m {
            for b in 0..m {
                self.normal[a * m + b] = (0..k).map(|i| self.matrix[i * m + a] * self.matrix[i * m + b]).sum();
            }
        }
    }
}

/// Solves A x = b for a symmetric positive definite `n` × `n` A (row-major),
/// in place: A becomes its Cholesky factor, b the solution. Returns false if
/// A is not positive definite.
fn cholesky_solve(a: &mut [f64], b: &mut [f64], n: usize) -> bool {
    for j in 0..n {
        let mut d = a[j * n + j];
        for k in 0..j {
            d -= a[j * n + k] * a[j * n + k];
        }
        if d <= 0.0 || !d.is_finite() {
            return false;
        }
        let d = d.sqrt();
        a[j * n + j] = d;
        for i in j + 1..n {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= a[i * n + k] * a[j * n + k];
            }
            a[i * n + j] = s / d;
        }
    }
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i * n + k] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in i + 1..n {
            s -= a[k * n + i] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cascade's response (dB) at each band centre.
    fn at_centres(eq: &mut GraphicEq, sliders: &[f64]) -> Vec<f64> {
        let bands = eq.bands(sliders);
        let coeffs: Vec<Coeffs> = bands.iter().map(|b| Coeffs::for_band(b, eq.rate)).collect();
        eq.centres[..eq.active].iter().map(|f| coeffs.iter().map(|c| c.magnitude_db(*f, eq.rate)).sum()).collect()
    }

    /// A fixed pseudo-random sequence in [0, 1).
    fn random(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    fn worst_over(centres: &'static [f64], c: f64, rate: f64, settings: &[Vec<f64>]) -> f64 {
        let mut eq = GraphicEq::new(centres, c, rate);
        settings
            .iter()
            .map(|s| at_centres(&mut eq, s).iter().zip(s).map(|(got, want)| (got - want).abs()).fold(0.0, f64::max))
            .fold(0.0, f64::max)
    }

    /// Worst error at the centres over the paper's test patterns and random
    /// settings.
    fn worst_error(centres: &'static [f64], c: f64, rate: f64) -> f64 {
        let n = centres.len();
        let mut settings: Vec<Vec<f64>> = vec![
            vec![12.0; n],
            vec![-12.0; n],
            (0..n).map(|i| if i % 2 == 0 { 12.0 } else { -12.0 }).collect(),
            (0..n).map(|i| if i % 2 == 0 { 6.0 } else { -6.0 }).collect(),
        ];
        for band in 0..n {
            settings.push((0..n).map(|i| if i == band { 12.0 } else { 0.0 }).collect());
        }
        let mut seed = 0x2017_0201u64;
        for _ in 0..300 {
            settings.push((0..n).map(|_| random(&mut seed) * 24.0 - 12.0).collect());
        }
        worst_over(centres, c, rate, &settings)
    }

    /// Worst error over extreme settings: every slider at −12, 0 or +12.
    fn worst_corner_error(centres: &'static [f64], c: f64, rate: f64) -> f64 {
        let mut seed = 0x0daf_0017u64;
        let settings: Vec<Vec<f64>> = (0..600)
            .map(|_| centres.iter().map(|_| [-12.0, 0.0, 12.0][(random(&mut seed) * 3.0) as usize]).collect())
            .collect();
        worst_over(centres, c, rate, &settings)
    }

    #[test]
    fn the_bands_add_up_to_the_sliders() {
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            let ten = worst_error(&GRAPHIC_10, 0.3, rate);
            let thirty_one = worst_error(&GRAPHIC_31, 0.35, rate);
            eprintln!("worst error at the band centres, {rate} Hz: 10-band {ten:.2} dB, 31-band {thirty_one:.2} dB");
            assert!(ten <= 1.0, "10-band at {rate}: {ten:.2} dB");
            assert!(thirty_one <= 1.0, "31-band at {rate}: {thirty_one:.2} dB");
        }
    }

    /// At extreme settings (every slider at −12, 0 or +12) the 10-band design
    /// still holds 1 dB; the 31-band one misses it by up to about 0.15 dB, at
    /// the unevenly spaced ISO centres and near Nyquist (the authors' own
    /// design misses by 0.2 dB there).
    #[test]
    fn extreme_settings_stay_close() {
        for rate in [44_100.0, 48_000.0] {
            let ten = worst_corner_error(&GRAPHIC_10, 0.3, rate);
            let thirty_one = worst_corner_error(&GRAPHIC_31, 0.35, rate);
            eprintln!("worst error at extreme settings, {rate} Hz: 10-band {ten:.2} dB, 31-band {thirty_one:.2} dB");
            assert!(ten <= 1.0, "10-band at {rate}: {ten:.2} dB");
            assert!(thirty_one <= 1.2, "31-band at {rate}: {thirty_one:.2} dB");
        }
    }

    /// The sum of the uncorrected filters was far off: every slider at +6
    /// peaked at +8.8 dB, alternating ±6 gave about ±4 dB.
    #[test]
    fn equal_and_alternating_sliders_come_out_as_set() {
        let mut eq = GraphicEq::new(&GRAPHIC_10, 0.3, 48_000.0);
        for (sliders, what) in [([6.0; 10], "all +6"), ([6.0, -6.0, 6.0, -6.0, 6.0, -6.0, 6.0, -6.0, 6.0, -6.0], "±6")]
        {
            let got = at_centres(&mut eq, &sliders);
            for (g, s) in got.iter().zip(sliders) {
                assert!((g - s).abs() < 0.5, "{what}: {got:?}");
            }
        }
    }

    #[test]
    fn every_band_keeps_its_slot_then_bass_and_treble() {
        let mut cache = GraphicCache::default();
        let mut curve = EqCurve::default();
        let slots = curve_slots(&curve, 48_000.0, &mut cache);
        assert_eq!(slots.len(), 8 + 2);
        assert!(slots.iter().all(|b| b.gain_db == 0.0), "a flat curve has only flat slots");
        curve.parametric[3].enabled = false;
        assert_eq!(curve_slots(&curve, 48_000.0, &mut cache).len(), 10, "disabled bands keep their slot");
        curve.bass_db = 3.0;
        let bass = curve_slots(&curve, 48_000.0, &mut cache)[8];
        assert_eq!((bass.kind, bass.freq, bass.gain_db), (FilterKind::LowShelf, 100.0, 3.0));
        curve.mode = EqMode::Graphic31;
        let slots = curve_slots(&curve, 48_000.0, &mut cache);
        assert_eq!(slots.len(), 31 + 2);
        assert_eq!(slots[32].kind, FilterKind::HighShelf);
    }

    /// The response drawn is the one played: a graphic curve reads its
    /// sliders back at the band centres, and costs little enough to redraw
    /// while a slider is dragged.
    #[test]
    fn the_drawn_response_is_the_one_played() {
        let mut cache = GraphicCache::default();
        let mut curve = EqCurve { mode: EqMode::Graphic31, ..EqCurve::default() };
        curve.graphic31 = (0..31).map(|i| if i % 3 == 0 { 9.0 } else { -4.0 }).collect();
        let draw: Vec<f64> = (0..=240).map(|i| 20.0 * 1000f64.powf(f64::from(i) / 240.0)).collect();
        let started = std::time::Instant::now();
        let response = curve_response(&curve, 48_000.0, &draw, &mut cache);
        let first = started.elapsed();
        curve.graphic31[10] = 3.0;
        let started = std::time::Instant::now();
        curve_response(&curve, 48_000.0, &draw, &mut cache);
        eprintln!("31-band response: {first:?} with a new solver, {:?} after", started.elapsed());
        let at_centres = curve_response(&curve, 48_000.0, &GRAPHIC_31, &mut cache);
        for (got, want) in at_centres.total.iter().zip(&curve.graphic31) {
            assert!((got - want).abs() < 1.0, "{got:.2} for {want}");
        }
        assert_eq!(response.total.len(), draw.len());
        assert!(response.bands.is_empty() && response.max_boost_db > 8.0);

        let flat = curve_response(&EqCurve::default(), 44_100.0, &draw, &mut cache);
        assert!(flat.total.iter().all(|v| *v == 0.0) && flat.max_boost_db == 0.0);
        assert_eq!(flat.bands.len(), 8, "one curve per parametric band");
    }

    #[test]
    fn flat_sliders_give_exactly_flat_filters() {
        let mut eq = GraphicEq::new(&GRAPHIC_31, 0.35, 44_100.0);
        assert!(eq.bands(&[0.0; 31]).iter().all(|b| b.gain_db == 0.0));
    }

    #[test]
    fn bands_near_nyquist_are_left_out() {
        let mut eq = GraphicEq::new(&GRAPHIC_31, 0.35, 32_000.0);
        assert_eq!(eq.active, 29, "16 and 20 kHz are above 95% of 16 kHz");
        let bands = eq.bands(&[6.0; 31]);
        assert_eq!((bands[29].gain_db, bands[30].gain_db), (0.0, 0.0));
        assert!(bands.iter().all(|b| b.q.is_finite() && b.gain_db.is_finite()));
    }
}
