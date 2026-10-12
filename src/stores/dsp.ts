// Mirror of the DSP settings owned by Rust.
//
// Controls update the store immediately so dragging stays smooth; changes
// are sent to the core one at a time, always the newest, and the applied
// (validated) result replaces the local copy once nothing newer is pending.

import type { UnlistenFn } from '@tauri-apps/api/event';
import { create } from 'zustand';

import { onCoreEvent } from '@/services/events';
import { dspApi } from '@/services/ipc';
import { errorMessage, showToast } from '@/stores/toasts';
import type { DspSettings, DspState, DspStatus, EqCurve, Preset } from '@/types/dsp';

interface DspStore {
  loaded: boolean;
  settings: DspSettings | null;
  presets: Preset[];
  status: DspStatus;
}

export const useDsp = create<DspStore>()(() => ({
  loaded: false,
  settings: null,
  presets: [],
  status: { replayGainSource: 'off', replayGainDb: 0, limiting: false, clipping: false, sampleRate: 0, channels: 0 },
}));

let pending: DspSettings | null = null;
let inflight = false;

async function flush() {
  if (inflight || !pending) return;
  const send = pending;
  pending = null;
  inflight = true;
  try {
    const applied = await dspApi.update(send);
    if (!pending) useDsp.setState({ settings: applied });
  } catch (e) {
    showToast({ tone: 'error', message: `Could not apply the sound settings: ${errorMessage(e)}` });
  } finally {
    inflight = false;
    void flush();
  }
}

/** Replaces the settings (optimistically) and sends them to the core. */
export function setDsp(next: DspSettings) {
  useDsp.setState({ settings: next });
  pending = next;
  void flush();
}

export function patchDsp(patch: Partial<DspSettings>) {
  const current = useDsp.getState().settings;
  if (current) setDsp({ ...current, ...patch });
}

/** The curve in the active compare slot. */
export function activeCurve(settings: DspSettings): EqCurve {
  return settings.slot === 'a' ? settings.a : settings.b;
}

/**
 * Changes the active slot's curve. Shaping the curve means wanting to hear
 * it, so an edit switches the EQ on and leaves bypass.
 */
export function patchCurve(patch: Partial<EqCurve>) {
  const current = useDsp.getState().settings;
  if (!current) return;
  const curve = { ...activeCurve(current), ...patch };
  const next = { ...current, eqEnabled: true, bypass: false };
  setDsp(current.slot === 'a' ? { ...next, a: curve } : { ...next, b: curve });
}

export function applyState(state: DspState) {
  useDsp.setState({ loaded: true, settings: state.settings, presets: state.presets, status: state.status });
}

export async function refreshDsp() {
  applyState(await dspApi.state());
}

export async function connectDsp(): Promise<UnlistenFn> {
  const unlisten = await onCoreEvent('dsp-status', (status) => useDsp.setState({ status }));
  await refreshDsp();
  return unlisten;
}
