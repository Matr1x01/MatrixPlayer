// The EQ's frequency response as the core computes and plays it. Graphic
// bands are solved and every filter is designed in Rust, at the stream's
// sample rate, so the curve on screen is the one being heard.

import { useEffect, useState } from 'react';

import { dspApi } from '@/services/ipc';
import type { EqCurve, EqResponse } from '@/types/dsp';

/** Frequencies the response is drawn at: 241 points from 20 Hz to 20 kHz. */
export const DRAW_FREQS = Array.from({ length: 241 }, (_, i) => 20 * Math.pow(1000, i / 240));

/** Rate assumed while nothing is playing. */
const DEFAULT_RATE = 48_000;

/**
 * The response of `curve` at `rate` (0: nothing playing), refetched as the
 * curve changes. The last one stays on screen until a newer one arrives, and
 * answers to superseded requests are dropped, so dragging never flickers.
 */
export function useEqResponse(curve: EqCurve | null, rate: number): EqResponse | null {
  const [response, setResponse] = useState<EqResponse | null>(null);
  useEffect(() => {
    if (!curve) return;
    let current = true;
    dspApi
      .eqResponse(curve, rate || DEFAULT_RATE, DRAW_FREQS)
      .then((r) => current && setResponse(r))
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [curve, rate]);
  return response;
}
