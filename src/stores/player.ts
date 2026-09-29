// Mirror of the player state owned by Rust.
//
// Nothing here is computed locally: the store is filled from the snapshot
// and then kept current by core events. Components read it through hooks
// and change playback only by calling commands.

import type { UnlistenFn } from '@tauri-apps/api/event';
import { create } from 'zustand';

import { onCoreEvent } from '@/services/events';
import { playerApi } from '@/services/ipc';
import type { PlayerSnapshot } from '@/types/player';

interface PlayerStore extends PlayerSnapshot {
  /** True once the first snapshot has arrived. */
  connected: boolean;
}

export const usePlayer = create<PlayerStore>()(() => ({
  connected: false,
  status: 'idle',
  track: null,
  positionMs: 0,
  durationMs: null,
  volume: { level: 0.8, muted: false, gainDb: null, locked: false },
  queue: { entries: [], currentIndex: null, shuffle: false, repeat: 'off', gapless: true },
  signal: {
    backend: 'auto',
    device: null,
    bufferMs: null,
    decoded: null,
    output: null,
    exclusive: false,
    bitPerfect: false,
    verdict: { state: 'off' },
    pipewireClock: null,
  },
  error: null,
}));

async function refresh() {
  const snapshot = await playerApi.snapshot();
  usePlayer.setState({ ...snapshot, connected: true });
}

/**
 * Subscribes to player events, then loads the snapshot. Subscribing first
 * means no change can slip between the snapshot and the first event.
 */
export async function connectPlayer(): Promise<UnlistenFn> {
  const unlisteners = await Promise.all([
    onCoreEvent('playback-state-changed', ({ status, error }) => usePlayer.setState({ status, error })),
    onCoreEvent('track-changed', ({ track }) => usePlayer.setState({ track })),
    onCoreEvent('position-changed', ({ positionMs, durationMs }) => usePlayer.setState({ positionMs, durationMs })),
    onCoreEvent('volume-changed', (volume) => usePlayer.setState({ volume })),
    onCoreEvent('queue-changed', (queue) => usePlayer.setState({ queue })),
    onCoreEvent('queue-state-changed', (state) => usePlayer.setState((s) => ({ queue: { ...s.queue, ...state } }))),
    onCoreEvent('signal-path-changed', (signal) => usePlayer.setState({ signal })),
    onCoreEvent('player-resync', () => void refresh()),
  ]);
  await refresh();
  return () => unlisteners.forEach((unlisten) => unlisten());
}
