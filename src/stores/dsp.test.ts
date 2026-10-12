import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { DspSettings, EqCurve } from '@/types/dsp';

const update = vi.fn((settings: DspSettings) => Promise.resolve(settings));
vi.mock('@/services/ipc', () => ({ dspApi: { update: (s: DspSettings) => update(s) } }));
vi.mock('@/services/events', () => ({ onCoreEvent: () => Promise.resolve(() => {}) }));

const { patchCurve, useDsp } = await import('./dsp');

const curve: EqCurve = {
  mode: 'parametric',
  parametric: [{ enabled: true, kind: 'peak', freq: 1000, gainDb: 0, q: 1 }],
  graphic10: new Array(10).fill(0),
  graphic31: new Array(31).fill(0),
  bassDb: 0,
  trebleDb: 0,
};

const settings: DspSettings = {
  version: 3,
  eqEnabled: false,
  slot: 'b',
  bypass: true,
  a: curve,
  b: curve,
  preset: 'Flat',
  preampDb: 0,
  autoPreamp: false,
  autoPreampAllowanceDb: 3,
  limiter: false,
  balance: 0,
  width: 1,
  crossfeed: false,
  crossfeedPreset: 'default',
  replayGain: 'album',
  replayGainPreampDb: 0,
  replayGainFallbackDb: 0,
  clippingPrevention: true,
  softFades: true,
  dither: false,
};

describe('dsp store', () => {
  beforeEach(() => {
    update.mockClear();
    useDsp.setState({ settings });
  });

  it('switches the EQ on when the curve is edited, in the active slot only', () => {
    patchCurve({ bassDb: 3 });
    const next = useDsp.getState().settings!;
    expect(next.eqEnabled).toBe(true);
    expect(next.bypass).toBe(false);
    expect(next.b.bassDb).toBe(3);
    expect(next.a.bassDb).toBe(0);
    expect(update).toHaveBeenCalledTimes(1);
  });
});
