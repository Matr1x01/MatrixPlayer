import { describe, expect, it } from 'vitest';

import type { EqBand, EqCurve } from '@/types/dsp';

import { parseFreq, parseNumber, sameCurve } from './eqMath';

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
