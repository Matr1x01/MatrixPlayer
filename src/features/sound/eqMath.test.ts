import { describe, expect, it } from 'vitest';

import type { EqBand, EqCurve } from '@/types/dsp';

import { coefficients, curveFilters, magnitudeDb, maxBoostDb, parseFreq, parseNumber, sameCurve } from './eqMath';

const peak = (freq: number, gainDb: number, q: number): EqBand => ({ enabled: true, kind: 'peak', freq, gainDb, q });

const flat: EqCurve = {
  mode: 'parametric',
  parametric: [peak(100, 0, 1), peak(1000, 0, 1)],
  graphic10: new Array(10).fill(0),
  graphic31: new Array(31).fill(0),
  bassDb: 0,
  trebleDb: 0,
};

describe('eq math', () => {
  it('matches the core: a peak reaches its gain at the centre', () => {
    const c = coefficients(peak(1000, 6, 1));
    expect(magnitudeDb(c, 1000)).toBeCloseTo(6, 2);
    expect(Math.abs(magnitudeDb(c, 20))).toBeLessThan(0.1);
  });

  it('finds the largest boost of the combined curve', () => {
    const boost = maxBoostDb([peak(1000, 3, 1), peak(1100, 2, 1), peak(100, -8, 1)]);
    expect(boost).toBeGreaterThan(4.8);
    expect(boost).toBeLessThan(5);
  });

  it('builds the same filters as the core', () => {
    expect(curveFilters(flat)).toEqual([]);
    const toned = { ...flat, bassDb: 3, mode: 'graphic10' as const, graphic10: [0, 0, 0, 0, 0, 2, 0, 0, 0, 0] };
    expect(curveFilters(toned).map((b) => [b.kind, b.freq])).toEqual([
      ['peak', 1000],
      ['lowShelf', 100],
    ]);
  });

  it('ignores flat bands when comparing curves', () => {
    expect(sameCurve(flat, { ...flat, parametric: [] })).toBe(true);
    expect(sameCurve(flat, { ...flat, parametric: [peak(100, 1, 1)] })).toBe(false);
    expect(sameCurve(flat, { ...flat, mode: 'graphic10' })).toBe(false);
  });

  it('parses typed values', () => {
    expect(parseFreq('2.4k')).toBe(2400);
    expect(parseFreq('120 Hz')).toBe(120);
    expect(parseFreq('abc')).toBeNull();
    expect(parseNumber('−3.5 dB')).toBe(-3.5);
    expect(parseNumber('+2')).toBe(2);
  });
});
