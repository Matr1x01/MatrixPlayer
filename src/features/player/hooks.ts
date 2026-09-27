import { useCallback, useEffect, useRef, useState } from 'react';

import { playerApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';
import type { PlaybackStatus } from '@/types/player';

/** Whether the play button should offer "pause". Transitional states count
 * as playing so the button does not flicker while a track loads or seeks. */
export function showsPause(status: PlaybackStatus): boolean {
  return status === 'playing' || status === 'loading' || status === 'seeking';
}

/** True when the pipeline holds a stream that can seek. */
export function isSeekable(status: PlaybackStatus): boolean {
  return status === 'playing' || status === 'paused' || status === 'seeking';
}

/**
 * Sends slider positions as volume commands at most once per animation
 * frame while dragging, so the core applies volume live without a flood.
 */
export function useLiveVolume() {
  const pending = useRef<number | null>(null);
  const scheduled = useRef(false);

  const input = useCallback((level: number) => {
    pending.current = level;
    if (scheduled.current) return;
    scheduled.current = true;
    requestAnimationFrame(() => {
      scheduled.current = false;
      if (pending.current != null) void run(playerApi.setVolume(pending.current));
    });
  }, []);

  const commit = useCallback((level: number) => {
    pending.current = null;
    void run(playerApi.setVolume(level));
  }, []);

  return { input, commit };
}

/**
 * Where a seek was sent, shown until the reported position gets there (or
 * a moment passes), so the playhead does not jump back while the seek is on
 * its way to the core.
 */
export function useSeekTarget(positionMs: number) {
  const [target, setTarget] = useState<number | null>(null);
  useEffect(() => {
    if (target != null && Math.abs(positionMs - target) < 750) setTarget(null);
  }, [positionMs, target]);
  useEffect(() => {
    if (target == null) return;
    const timer = window.setTimeout(() => setTarget(null), 1500);
    return () => window.clearTimeout(timer);
  }, [target]);
  return [target, setTarget] as const;
}

/** Cycles repeat off → all → one → off. */
export function cycleRepeat() {
  const next = { off: 'all', all: 'one', one: 'off' } as const;
  const current = usePlayer.getState().queue.repeat;
  void run(playerApi.setRepeat(next[current]));
}

export function toggleShuffle() {
  void run(playerApi.setShuffle(!usePlayer.getState().queue.shuffle));
}

export function toggleMute() {
  void run(playerApi.setMuted(!usePlayer.getState().volume.muted));
}
