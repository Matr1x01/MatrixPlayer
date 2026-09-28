// A stand-in for the core's EQ response in the browser harness, which has
// no Rust behind it: plain cookbook filters, with graphic sliders used as
// filter gains. The app draws what src-tauri/src/dsp computes instead
// (dsp_eq_response), which solves the graphic bands and matches every filter
// to its analog shape; this only has to look plausible.

import { GRAPHIC_10, GRAPHIC_10_Q, GRAPHIC_31, GRAPHIC_31_Q } from '@/features/sound/eqMath';
import type { EqBand, EqCurve, EqResponse } from '@/types/dsp';

const DRAW_RATE = 48_000;

interface Coeffs {
  b0: number;
  b1: number;
  b2: number;
  a1: number;
  a2: number;
}

export function coefficients(band: EqBand, fs = DRAW_RATE): Coeffs {
  const f0 = Math.max(1, Math.min(band.freq, fs * 0.45));
  const w0 = (2 * Math.PI * f0) / fs;
  const sin = Math.sin(w0);
  const cos = Math.cos(w0);
  const alpha = sin / (2 * Math.max(band.q, 0.05));
  const a = Math.pow(10, band.gainDb / 40);
  let b0: number, b1: number, b2: number, a0: number, a1: number, a2: number;
  switch (band.kind) {
    case 'peak':
      [b0, b1, b2, a0, a1, a2] = [1 + alpha * a, -2 * cos, 1 - alpha * a, 1 + alpha / a, -2 * cos, 1 - alpha / a];
      break;
    case 'lowShelf': {
      const s = 2 * Math.sqrt(a) * alpha;
      [b0, b1, b2] = [a * (a + 1 - (a - 1) * cos + s), 2 * a * (a - 1 - (a + 1) * cos), a * (a + 1 - (a - 1) * cos - s)];
      [a0, a1, a2] = [a + 1 + (a - 1) * cos + s, -2 * (a - 1 + (a + 1) * cos), a + 1 + (a - 1) * cos - s];
      break;
    }
    case 'highShelf': {
      const s = 2 * Math.sqrt(a) * alpha;
      [b0, b1, b2] = [a * (a + 1 + (a - 1) * cos + s), -2 * a * (a - 1 + (a + 1) * cos), a * (a + 1 + (a - 1) * cos - s)];
      [a0, a1, a2] = [a + 1 - (a - 1) * cos + s, 2 * (a - 1 - (a + 1) * cos), a + 1 - (a - 1) * cos - s];
      break;
    }
    case 'lowPass':
      [b0, b1, b2, a0, a1, a2] = [(1 - cos) / 2, 1 - cos, (1 - cos) / 2, 1 + alpha, -2 * cos, 1 - alpha];
      break;
    case 'highPass':
      [b0, b1, b2, a0, a1, a2] = [(1 + cos) / 2, -(1 + cos), (1 + cos) / 2, 1 + alpha, -2 * cos, 1 - alpha];
      break;
    case 'notch':
      [b0, b1, b2, a0, a1, a2] = [1, -2 * cos, 1, 1 + alpha, -2 * cos, 1 - alpha];
      break;
  }
  return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
}

export function magnitudeDb(c: Coeffs, f: number, fs = DRAW_RATE): number {
  const w = (2 * Math.PI * f) / fs;
  const [c1, s1, c2, s2] = [Math.cos(w), Math.sin(w), Math.cos(2 * w), Math.sin(2 * w)];
  const numRe = c.b0 + c.b1 * c1 + c.b2 * c2;
  const numIm = -(c.b1 * s1 + c.b2 * s2);
  const denRe = 1 + c.a1 * c1 + c.a2 * c2;
  const denIm = -(c.a1 * s1 + c.a2 * s2);
  return 10 * Math.log10(Math.max((numRe * numRe + numIm * numIm) / (denRe * denRe + denIm * denIm), 1e-30));
}

/** A function giving the combined response (dB) of `bands` at a frequency. */
export function responseOf(bands: EqBand[], fs = DRAW_RATE): (f: number) => number {
  const cs = bands.map((b) => coefficients(b, fs));
  return (f) => cs.reduce((sum, c) => sum + magnitudeDb(c, f, fs), 0);
}

export function graphicBands(freqs: number[], gains: number[], q: number): EqBand[] {
  return freqs.map((freq, i) => ({ enabled: true, kind: 'peak', freq, gainDb: gains[i] ?? 0, q }));
}

/** The filters a curve amounts to, as the core builds them. */
export function curveFilters(curve: EqCurve): EqBand[] {
  const out: EqBand[] =
    curve.mode === 'parametric'
      ? curve.parametric.filter((b) => b.enabled)
      : curve.mode === 'graphic10'
        ? graphicBands(GRAPHIC_10, curve.graphic10, GRAPHIC_10_Q)
        : graphicBands(GRAPHIC_31, curve.graphic31, GRAPHIC_31_Q);
  if (Math.abs(curve.bassDb) > 0.05) out.push({ enabled: true, kind: 'lowShelf', freq: 100, gainDb: curve.bassDb, q: 0.7 });
  if (Math.abs(curve.trebleDb) > 0.05) out.push({ enabled: true, kind: 'highShelf', freq: 8000, gainDb: curve.trebleDb, q: 0.7 });
  return out.filter((b) => Math.abs(b.gainDb) > 0.05 || b.kind === 'lowPass' || b.kind === 'highPass' || b.kind === 'notch');
}

/** Largest boost anywhere from 20 Hz to 20 kHz (what auto preamp removes). */
export function maxBoostDb(bands: EqBand[], fs = DRAW_RATE): number {
  if (!bands.length) return 0;
  const response = responseOf(bands, fs);
  let max = 0;
  for (let i = 0; i <= 240; i++) {
    const f = 20 * Math.pow(1000, i / 240);
    if (f < fs / 2) max = Math.max(max, response(f));
  }
  return max;
}

/** What `dsp_eq_response` would answer, roughly. */
export function previewResponse(curve: EqCurve, freqs: number[]): EqResponse {
  const total = responseOf(curveFilters(curve));
  const bands = curve.mode === 'parametric' ? curve.parametric.map((b) => responseOf([{ ...b, enabled: true }])) : [];
  return {
    total: freqs.map(total),
    bands: bands.map((r) => freqs.map(r)),
    maxBoostDb: maxBoostDb(curveFilters(curve)),
  };
}
