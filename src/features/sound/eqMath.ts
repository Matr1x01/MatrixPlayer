// Frequency response of the EQ, for drawing. These are the same cookbook
// formulas the core runs (src-tauri/src/dsp/biquad.rs), so the curve on
// screen is the curve being heard.

import type { EqBand, EqCurve, FilterKind } from '@/types/dsp';

export const GRAPHIC_10 = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
export const GRAPHIC_31 = [
  20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250, 1600, 2000, 2500, 3150,
  4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000,
];
export const GRAPHIC_10_Q = 1.41;
export const GRAPHIC_31_Q = 4.32;
export const MAX_GAIN_DB = 12;
export const MAX_BANDS = 10;
/** Sample rate the response is drawn for. */
export const DRAW_RATE = 48_000;

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

/** Whether two curves sound the same (for "modified" next to a preset). */
export function sameCurve(a: EqCurve, b: EqCurve): boolean {
  const near = (x: number, y: number) => Math.abs(x - y) < 0.05;
  const nearAll = (x: number[], y: number[]) => x.length === y.length && x.every((v, i) => near(v, y[i]!));
  if (a.mode !== b.mode || !near(a.bassDb, b.bassDb) || !near(a.trebleDb, b.trebleDb)) return false;
  if (a.mode === 'graphic10') return nearAll(a.graphic10, b.graphic10);
  if (a.mode === 'graphic31') return nearAll(a.graphic31, b.graphic31);
  const active = (c: EqCurve) => c.parametric.filter((band) => band.enabled && (Math.abs(band.gainDb) > 0.05 || band.kind !== 'peak'));
  const [x, y] = [active(a), active(b)];
  return (
    x.length === y.length &&
    x.every((band, i) => {
      const o = y[i]!;
      return band.kind === o.kind && near(band.freq, o.freq) && near(band.gainDb, o.gainDb) && near(band.q, o.q);
    })
  );
}

export const KIND_LABEL: Record<FilterKind, string> = {
  peak: 'Peak',
  lowShelf: 'Low shelf',
  highShelf: 'High shelf',
  lowPass: 'Low pass',
  highPass: 'High pass',
  notch: 'Notch',
};

export const KIND_SHORT: Record<FilterKind, string> = {
  peak: 'PK',
  lowShelf: 'LS',
  highShelf: 'HS',
  lowPass: 'LP',
  highPass: 'HP',
  notch: 'N',
};

/** Whether a filter kind has a gain (pass and notch filters do not). */
export function hasGain(kind: FilterKind): boolean {
  return kind === 'peak' || kind === 'lowShelf' || kind === 'highShelf';
}

export function formatFreq(hz: number): string {
  if (hz >= 1000) return `${(hz / 1000).toFixed(hz >= 10_000 ? 1 : 2)} kHz`;
  return `${Math.round(hz)} Hz`;
}

export function formatGain(db: number): string {
  const v = Math.abs(db) < 0.05 ? 0 : db;
  return `${v > 0 ? '+' : v < 0 ? '−' : ''}${Math.abs(v).toFixed(1)} dB`;
}

/** Parses "2.4k", "2400", "2.4 kHz" into Hz. */
export function parseFreq(text: string): number | null {
  const m = /^\s*([0-9]*\.?[0-9]+)\s*(k|khz|hz)?\s*$/i.exec(text);
  if (!m) return null;
  const v = parseFloat(m[1]!) * (m[2]?.toLowerCase().startsWith('k') ? 1000 : 1);
  return Number.isFinite(v) ? v : null;
}

/** Parses "+3.5", "−2", "-2 dB". */
export function parseNumber(text: string): number | null {
  const v = parseFloat(text.replace('−', '-').replace(/db/i, '').trim());
  return Number.isFinite(v) ? v : null;
}
