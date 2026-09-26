//! Audio analysis for the visualizer and level meters.
//!
//! The processor copies what it outputs into a short ring buffer, stamped
//! with pipeline running time. Processing runs ahead of the speakers by the
//! output's buffer, so the analysis thread looks up the samples at the
//! running time that is audible *now*, which keeps the bars on the beat.
//! Nothing is copied unless something is watching.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use serde::Serialize;

/// Seconds of audio kept, enough to cover any output buffering.
const RING_SECONDS: f64 = 1.5;
pub const FFT_SIZE: usize = 4096;
pub const BANDS: usize = 56;
pub const SCOPE_POINTS: usize = 192;
const FLOOR_DB: f32 = -72.0;
const LOW_HZ: f64 = 30.0;
const HIGH_HZ: f64 = 18_000.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisFrame {
    /// Log-spaced band levels, 0 (silence) … 1 (full scale).
    pub bands: Vec<f32>,
    /// Peak and RMS level per channel (L, R) in dBFS.
    pub peak_db: [f32; 2],
    pub rms_db: [f32; 2],
    /// A short stretch of the waveform per channel (L, R; −1 … 1).
    pub scope: [Vec<f32>; 2],
}

#[derive(Default)]
struct Ring {
    frames: Vec<[f32; 2]>,
    /// Index the next frame is written to.
    head: usize,
    /// Frames written since the last reset (saturates at capacity).
    filled: usize,
    rate: u32,
    /// Running time (ns) just after the newest frame.
    end_rt: Option<u64>,
}

#[derive(Default)]
pub struct AnalysisTap {
    watched: AtomicBool,
    ring: Mutex<Ring>,
}

impl AnalysisTap {
    /// Starts or stops copying audio for analysis.
    pub fn set_watched(&self, on: bool) {
        let was = self.watched.swap(on, Ordering::SeqCst);
        if was && !on {
            // Stale audio must not show up when watching resumes.
            self.reset();
        }
    }

    pub fn watched(&self) -> bool {
        self.watched.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        let mut ring = self.ring.lock().unwrap();
        ring.filled = 0;
        ring.end_rt = None;
    }

    /// Copies processed samples (interleaved, `channels` wide) into the ring.
    pub fn push(&self, samples: &[f64], channels: usize, rate: u32, end_rt: Option<gstreamer::ClockTime>) {
        if !self.watched() || channels == 0 {
            return;
        }
        let mut ring = self.ring.lock().unwrap();
        let capacity = (f64::from(rate) * RING_SECONDS) as usize;
        if ring.rate != rate || ring.frames.len() != capacity {
            ring.frames = vec![[0.0; 2]; capacity.max(1)];
            ring.rate = rate;
            ring.head = 0;
            ring.filled = 0;
        }
        let cap = ring.frames.len();
        for frame in samples.chunks_exact(channels) {
            let (l, r) = if channels == 1 { (frame[0], frame[0]) } else { (frame[0], frame[1]) };
            let head = ring.head;
            ring.frames[head] = [l as f32, r as f32];
            ring.head = (head + 1) % cap;
        }
        ring.filled = (ring.filled + samples.len() / channels).min(cap);
        ring.end_rt = end_rt.map(|t| t.nseconds());
    }

    /// The `len` frames ending at running time `at` (ns), oldest first.
    fn window(&self, at: u64, len: usize) -> Option<(Vec<[f32; 2]>, u32)> {
        let ring = self.ring.lock().unwrap();
        let end_rt = ring.end_rt?;
        let rate = ring.rate;
        if rate == 0 || ring.filled < len {
            return None;
        }
        // How many frames ago `at` was, clamped to what the ring holds.
        let behind = (end_rt.saturating_sub(at) as f64 * f64::from(rate) / 1e9) as usize;
        let behind = behind.min(ring.filled - len);
        let cap = ring.frames.len();
        let end = (ring.head + cap - behind) % cap;
        let start = (end + cap - len) % cap;
        let out = (0..len).map(|i| ring.frames[(start + i) % cap]).collect();
        Some((out, rate))
    }
}

/// Turns ring windows into frames. Keeps its FFT plan and smoothing state.
pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    smooth: Vec<f32>,
}

impl Default for Analyzer {
    fn default() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        // Hann window.
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
            .collect();
        Self { fft, window, smooth: vec![0.0; BANDS] }
    }
}

impl Analyzer {
    /// Analyses the audio audible at running time `at` (ns).
    pub fn analyze(&mut self, tap: &AnalysisTap, at: u64) -> Option<AnalysisFrame> {
        let (frames, rate) = tap.window(at, FFT_SIZE)?;
        Some(self.frame(&frames, rate))
    }

    pub fn frame(&mut self, frames: &[[f32; 2]], rate: u32) -> AnalysisFrame {
        let mut buf: Vec<Complex<f32>> =
            frames.iter().zip(&self.window).map(|(f, w)| Complex::new((f[0] + f[1]) * 0.5 * w, 0.0)).collect();
        buf.resize(FFT_SIZE, Complex::new(0.0, 0.0));
        self.fft.process(&mut buf);

        // Sum power into log-spaced bands; normalise so a full-scale sine
        // reads 0 dB (the Hann window halves the amplitude).
        let bin_hz = f64::from(rate) / FFT_SIZE as f64;
        let high = HIGH_HZ.min(f64::from(rate) / 2.0 * 0.95);
        let norm = (FFT_SIZE as f32 / 4.0).powi(2);
        let mut bands = Vec::with_capacity(BANDS);
        for b in 0..BANDS {
            let f0 = LOW_HZ * (high / LOW_HZ).powf(b as f64 / BANDS as f64);
            let f1 = LOW_HZ * (high / LOW_HZ).powf((b + 1) as f64 / BANDS as f64);
            let i0 = ((f0 / bin_hz).floor() as usize).max(1);
            let i1 = ((f1 / bin_hz).ceil() as usize).clamp(i0 + 1, FFT_SIZE / 2);
            let power = buf[i0..i1].iter().map(|c| c.norm_sqr()).fold(0.0f32, f32::max) / norm;
            let db = 10.0 * power.max(1e-12).log10();
            let level = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
            // Fast attack, slower fall, like a meter's ballistics.
            let prev = self.smooth[b];
            let next = if level > prev { level } else { prev * 0.82 + level * 0.18 };
            self.smooth[b] = next;
            bands.push(next);
        }

        // Levels over the most recent ~50 ms.
        let recent = &frames[frames.len().saturating_sub((rate / 20) as usize)..];
        let mut peak = [0.0f32; 2];
        let mut sum = [0.0f32; 2];
        for f in recent {
            for (c, &v) in f.iter().enumerate() {
                peak[c] = peak[c].max(v.abs());
                sum[c] += v * v;
            }
        }
        let n = recent.len().max(1) as f32;
        let to_db = |v: f32| (20.0 * v.max(1e-6).log10()).max(-90.0);

        // A two-cycle-ish stretch of waveform for the oscilloscope.
        let span = ((rate / 40) as usize).min(frames.len());
        let tail = &frames[frames.len() - span..];
        let channel = |c: usize| (0..SCOPE_POINTS).map(|i| tail[i * span / SCOPE_POINTS][c]).collect();
        let scope = [channel(0), channel(1)];

        AnalysisFrame {
            bands,
            peak_db: [to_db(peak[0]), to_db(peak[1])],
            rms_db: [to_db((sum[0] / n).sqrt()), to_db((sum[1] / n).sqrt())],
            scope,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f64, amplitude: f64, rate: u32, seconds: f64) -> Vec<f64> {
        let n = (f64::from(rate) * seconds) as usize;
        (0..n)
            .flat_map(|i| {
                let v = amplitude * (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(rate)).sin();
                [v, v * 0.5]
            })
            .collect()
    }

    #[test]
    fn a_tone_lights_its_band_and_meters_read_its_level() {
        let tap = AnalysisTap::default();
        tap.set_watched(true);
        let samples = tone(1000.0, 0.5, 48_000, 0.5);
        tap.push(&samples, 2, 48_000, Some(gstreamer::ClockTime::from_mseconds(500)));
        let frame = Analyzer::default().analyze(&tap, 500_000_000).unwrap();

        let loudest = frame.bands.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        let centre = LOW_HZ * (HIGH_HZ / LOW_HZ).powf((loudest as f64 + 0.5) / BANDS as f64);
        assert!((700.0..1400.0).contains(&centre), "loudest band near {centre} Hz");
        assert!((frame.peak_db[0] + 6.02).abs() < 0.2, "L peak {}", frame.peak_db[0]);
        assert!((frame.peak_db[1] + 12.04).abs() < 0.2, "R peak {}", frame.peak_db[1]);
        assert!((frame.rms_db[0] + 9.03).abs() < 0.3, "L rms {}", frame.rms_db[0]);
        assert_eq!(frame.scope[0].len(), SCOPE_POINTS);
        assert!(frame.scope[1].iter().all(|v| v.abs() <= 0.26), "R is the quieter channel");
    }

    #[test]
    fn looks_back_to_the_audible_moment() {
        let tap = AnalysisTap::default();
        tap.set_watched(true);
        // 0.5 s of silence, then 0.5 s of tone; the ring ends at 1.0 s.
        let mut samples = vec![0.0; 48_000];
        samples.extend(tone(440.0, 0.8, 48_000, 0.5));
        tap.push(&samples, 2, 48_000, Some(gstreamer::ClockTime::from_mseconds(1000)));
        let mut analyzer = Analyzer::default();
        let now = analyzer.analyze(&tap, 1_000_000_000).unwrap();
        let earlier = analyzer.analyze(&tap, 400_000_000).unwrap();
        assert!(now.peak_db[0] > -3.0);
        assert!(earlier.peak_db[0] < -80.0, "at 0.4 s it was still silent");
    }

    #[test]
    fn copies_nothing_while_unwatched() {
        let tap = AnalysisTap::default();
        tap.push(&[0.5; 20_000], 2, 48_000, Some(gstreamer::ClockTime::ZERO));
        assert!(Analyzer::default().analyze(&tap, 0).is_none());
    }
}
