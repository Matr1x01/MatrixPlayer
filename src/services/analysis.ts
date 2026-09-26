// One shared subscription to the core's `audio-analysis` stream.
//
// The core only analyses audio while something is watching, so the stream
// is switched on when the first component asks for frames and off when the
// last one goes away. Between frames (paused, stopped, seeking) the values
// fall back to silence smoothly instead of freezing.

import { useEffect, useSyncExternalStore } from 'react';

import { onCoreEvent } from '@/services/events';
import { dspApi } from '@/services/ipc';
import type { AnalysisFrame } from '@/types/dsp';

const BANDS = 56;
const SCOPE = 192;
/** A frame older than this is treated as silence falling away. */
const STALE_MS = 140;

export const SILENT_FRAME: AnalysisFrame = {
  bands: new Array<number>(BANDS).fill(0),
  peakDb: [-90, -90],
  rmsDb: [-90, -90],
  scope: [new Array<number>(SCOPE).fill(0), new Array<number>(SCOPE).fill(0)],
};

let current: AnalysisFrame = SILENT_FRAME;
let receivedAt = 0;
let watchers = 0;
let generation = 0;
let unlisten: (() => void) | null = null;
let raf = 0;
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach((listener) => listener());
}

function decay(frame: AnalysisFrame): AnalysisFrame {
  const bands = frame.bands.map((v) => (v < 0.01 ? 0 : v * 0.84));
  const fall = (db: number) => Math.max(-90, db - 2.4);
  const scope = frame.scope.map((ch) => ch.map((v) => (Math.abs(v) < 0.002 ? 0 : v * 0.7))) as [number[], number[]];
  const next = {
    bands,
    peakDb: [fall(frame.peakDb[0]), fall(frame.peakDb[1])] as [number, number],
    rmsDb: [fall(frame.rmsDb[0]), fall(frame.rmsDb[1])] as [number, number],
    scope,
  };
  const silent = bands.every((v) => v === 0) && next.peakDb[0] <= -90 && next.peakDb[1] <= -90;
  return silent ? SILENT_FRAME : next;
}

function tick(now: number) {
  raf = requestAnimationFrame(tick);
  if (current !== SILENT_FRAME && now - receivedAt > STALE_MS) {
    current = decay(current);
    emit();
  }
}

function start() {
  const token = ++generation;
  void onCoreEvent('audio-analysis', (frame) => {
    current = frame;
    receivedAt = performance.now();
    emit();
  }).then((stop) => {
    if (token === generation) unlisten = stop;
    else stop();
  });
  void dspApi.setAnalysis(true).catch(() => {});
  raf = requestAnimationFrame(tick);
}

function stop() {
  generation++;
  unlisten?.();
  unlisten = null;
  cancelAnimationFrame(raf);
  void dspApi.setAnalysis(false).catch(() => {});
  current = SILENT_FRAME;
}

function watch(): () => void {
  watchers += 1;
  if (watchers === 1) start();
  return () => {
    watchers -= 1;
    if (watchers === 0) stop();
  };
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The latest analysis frame, re-rendering on each one while `enabled`. */
export function useAnalysis(enabled = true): AnalysisFrame {
  useEffect(() => (enabled ? watch() : undefined), [enabled]);
  const frame = useSyncExternalStore(subscribe, () => current);
  return enabled ? frame : SILENT_FRAME;
}

/** Centre frequency (Hz) of analysis band `i`, matching the core. */
export function bandFrequency(i: number): number {
  return 30 * Math.pow(18_000 / 30, (i + 0.5) / BANDS);
}
