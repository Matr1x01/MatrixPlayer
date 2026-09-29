import { describe, expect, it } from 'vitest';

import { verdictText } from './bitPerfect';

describe('bit-perfect verdicts', () => {
  it('say how the samples get there', () => {
    expect(verdictText({ state: 'yes', shared: false })).toMatchObject({ label: 'Bit-perfect', ok: true });
    expect(verdictText({ state: 'yes', shared: true }).detail).toContain('PipeWire');
  });

  it('say what breaks it', () => {
    expect(verdictText({ state: 'resampled', from: 44100, to: 48000 })).toEqual({
      label: 'Not bit-perfect',
      detail: 'Resampled 44.1 → 48 kHz',
      ok: false,
    });
    expect(verdictText({ state: 'truncated', from: 32, to: 24 }).detail).toBe('32-bit cut to 24-bit');
    expect(verdictText({ state: 'remixed', from: 1, to: 2 }).ok).toBe(false);
    expect(verdictText({ state: 'fromFloat' }).detail).toContain('Lossy');
  });
});
