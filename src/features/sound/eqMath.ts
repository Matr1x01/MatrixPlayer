// EQ layout constants and the helpers the equalizer's controls share. The
// response curve itself comes from the core (see eqResponse.ts), which
// solves the graphic bands and designs every filter as it plays them.

import type { EqCurve, FilterKind } from '@/types/dsp';

export const GRAPHIC_10 = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
export const GRAPHIC_31 = [
  20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250, 1600, 2000, 2500, 3150,
  4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000,
];
export const GRAPHIC_10_Q = 1.41;
export const GRAPHIC_31_Q = 4.32;
export const MAX_GAIN_DB = 12;
export const MAX_BANDS = 10;

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
