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

/** A response, and the curve and rate (as a key) it was computed for. */
interface Drawn {
  key: string;
  response: EqResponse;
}

/** The last response computed, kept across mounts so the graph opens drawn. */
let last: Drawn | null = null;

/**
 * The response of `curve` at `rate` (0: nothing playing), refetched when the
 * curve's content or the rate changes. The last one stays on screen until a
 * newer one arrives, and answers to superseded requests are dropped, so
 * dragging never flickers. `current` says whether `response` is for this very
 * curve (per-band curves are drawn only then).
 */
export function useEqResponse(curve: EqCurve | null, rate: number): { response: EqResponse | null; current: boolean } {
  const effectiveRate = rate || DEFAULT_RATE;
  const key = curve ? `${effectiveRate}:${JSON.stringify(curve)}` : '';
  const [drawn, setDrawn] = useState<Drawn | null>(last);
  useEffect(() => {
    if (!curve || last?.key === key) {
      if (last) setDrawn(last);
      return;
    }
    let wanted = true;
    dspApi
      .eqResponse(curve, effectiveRate, DRAW_FREQS)
      .then((response) => {
        if (!wanted) return;
        last = { key, response };
        setDrawn(last);
      })
      .catch((e: unknown) => console.warn('could not compute the EQ response', e));
    return () => {
      wanted = false;
    };
    // `key` stands for the curve's content and the rate: a curve that is
    // equal but a new object (every settings round trip) is not refetched.
  }, [key]);
  return { response: drawn?.response ?? null, current: drawn != null && drawn.key === key };
}
