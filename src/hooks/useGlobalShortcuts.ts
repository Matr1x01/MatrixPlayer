import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect } from 'react';

import { addToNewPlaylist } from '@/features/library/trackMenu';
import { openFiles, openFolder } from '@/features/library/openActions';
import { openSearch } from '@/features/search/SearchDialog';
import { cycleRepeat, isSeekable, toggleMute, toggleShuffle } from '@/features/player/hooks';
import { playerApi } from '@/services/ipc';
import { useNavigation } from '@/stores/navigation';
import { setFavorite, useLibrary } from '@/stores/library';
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
      // Dialogs, menus and search handle their own keys.
      if (document.querySelector('.dlg-scrim, .search-scrim, .menu')) return;
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
          // Space means play/pause, even on a button left focused by a
          // click (pressing it again would be a surprise), but a control
          // reached with the keyboard takes Space as a press, as it does
          // everywhere else on the desktop.
          case ' ':
            if (!plain) return null;
            if (target?.matches('button:focus-visible, [role="switch"]:focus-visible, [role="checkbox"]:focus-visible, [role="tab"]:focus-visible')) {
              return null;
            }
            return () => void run(playerApi.toggle());
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
          case 'k':
          case 'K':
            return ctrl ? () => openSearch() : null;
          case 'f':
            return ctrl ? () => openSearch() : null;
          case 'n':
          case 'N':
            return ctrl ? () => void addToNewPlaylist([]) : null;
          case 'l':
          case 'L': {
            if (!plain) return null;
            const id = usePlayer.getState().track?.library?.trackId;
            if (id == null) return null;
            return () => void setFavorite([id], !useLibrary.getState().favorites.has(id));
          }
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
            const view = nav.history[nav.index]?.view;
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
