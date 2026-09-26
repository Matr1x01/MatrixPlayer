// Names and payloads of the events the Rust core emits (src-tauri/src/events.rs).

import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type { LibraryChange, ScanStatus } from '@/types/library';
import type { PlaybackStatus, PlayerError, QueueSnapshot, SignalPath, TrackInfo, VolumeState } from '@/types/player';

export interface CoreEvents {
  'playback-state-changed': { status: PlaybackStatus; error: PlayerError | null };
  'track-changed': { track: TrackInfo | null };
  'position-changed': { positionMs: number; durationMs: number | null };
  'volume-changed': VolumeState;
  'queue-changed': QueueSnapshot;
  'queue-state-changed': Omit<QueueSnapshot, 'entries'>;
  'signal-path-changed': SignalPath;
  'audio-error': PlayerError;
  'player-resync': null;
  'tracks-queued': { count: number };
  'library-scan': ScanStatus;
  'library-changed': LibraryChange;
}

export function onCoreEvent<K extends keyof CoreEvents>(name: K, handler: (payload: CoreEvents[K]) => void): Promise<UnlistenFn> {
  return listen<CoreEvents[K]>(name, (event) => handler(event.payload));
}
