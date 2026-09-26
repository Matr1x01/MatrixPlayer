import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect } from 'react';

import { openFiles, openFolder } from '@/features/library/openActions';
import { cycleRepeat, isSeekable, toggleMute, toggleShuffle } from '@/features/player/hooks';
import { playerApi } from '@/services/ipc';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';

const SEEK_STEP_MS = 10_000;
const VOLUME_STEP = 0.05;

function seekBy(deltaMs: number) {
  const { positionMs, durationMs, status } = usePlayer.getState();
  if (!durationMs || !isSeekable(status)) return;
  void run(playerApi.seek(Math.min(durationMs, Math.max(0, positionMs + deltaMs))));
}

function nudgeVolume(delta: number) {
  const level = usePlayer.getState().volume.level;
  void run(playerApi.setVolume(Math.min(1, Math.max(0, level + delta))));
}

async function toggleFullscreen() {
  const win = getCurrentWindow();
  await win.setFullscreen(!(await win.isFullscreen()));
}

/**
 * App-wide keyboard shortcuts (listed in Settings › Keyboard shortcuts).
 * Media keys work while the window is focused; system-wide media keys come
 * with MPRIS.
 */
export function useGlobalShortcuts() {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target?.closest('input, textarea, select, [contenteditable="true"]')) return;
      const ctrl = e.ctrlKey || e.metaKey;
      const plain = !ctrl && !e.altKey && !e.shiftKey;
      const nav = useNavigation.getState();

      const action = ((): (() => void) | null => {
        switch (e.key) {
          case 'MediaPlayPause':
            return () => void run(playerApi.toggle());
          case 'MediaTrackNext':
            return () => void run(playerApi.next());
          case 'MediaTrackPrevious':
            return () => void run(playerApi.previous());
          case 'MediaStop':
            return () => void run(playerApi.stop());
          // Space always means play/pause here, even on a focused button;
          // Enter still activates buttons for keyboard users.
          case ' ':
            return plain ? () => void run(playerApi.toggle()) : null;
          case 'ArrowRight':
            if (ctrl) return () => void run(playerApi.next());
            if (e.shiftKey) return () => seekBy(SEEK_STEP_MS);
            if (e.altKey) return nav.forward;
            return null;
          case 'ArrowLeft':
            if (ctrl) return () => void run(playerApi.previous());
            if (e.shiftKey) return () => seekBy(-SEEK_STEP_MS);
            if (e.altKey) return nav.back;
            return null;
          case 'ArrowUp':
            return ctrl ? () => nudgeVolume(VOLUME_STEP) : null;
          case 'ArrowDown':
            return ctrl ? () => nudgeVolume(-VOLUME_STEP) : null;
          case 'o':
          case 'O':
            if (!ctrl) return null;
            return e.shiftKey ? () => void openFolder() : () => void openFiles();
          case 'p':
            return ctrl ? () => nav.navigate('nowPlaying') : null;
          case ',':
            return ctrl ? () => nav.navigate('settings') : null;
          case 'm':
            return plain ? toggleMute : null;
          case 's':
            return plain ? toggleShuffle : null;
          case 'r':
            return plain ? cycleRepeat : null;
          case 'F11':
            return () => void toggleFullscreen();
          case 'Escape': {
            const view = nav.history[nav.index];
            return (view === 'nowPlaying' || view === 'settings') && nav.index > 0 ? nav.back : null;
          }
          default:
            return null;
        }
      })();

      if (action) {
        e.preventDefault();
        action();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
