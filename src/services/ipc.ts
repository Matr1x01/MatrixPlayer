// Typed wrappers for every Tauri command. Components call these, never
// `invoke` directly, so the command surface is listed in one place.

import { invoke } from '@tauri-apps/api/core';

import type { InsertMode, PlayerSnapshot, RepeatMode } from '@/types/player';
import type { AppInfo, UiPreferences, UiPreferencesPatch } from '@/types/preferences';

export const playerApi = {
  snapshot: () => invoke<PlayerSnapshot>('get_player_snapshot'),
  play: () => invoke<void>('player_play'),
  pause: () => invoke<void>('player_pause'),
  toggle: () => invoke<void>('player_toggle'),
  stop: () => invoke<void>('player_stop'),
  next: () => invoke<void>('player_next'),
  previous: () => invoke<void>('player_previous'),
  seek: (positionMs: number) => invoke<void>('player_seek', { positionMs }),
  setVolume: (level: number) => invoke<void>('player_set_volume', { level }),
  setMuted: (muted: boolean) => invoke<void>('player_set_muted', { muted }),
  setRepeat: (mode: RepeatMode) => invoke<void>('player_set_repeat', { mode }),
  setShuffle: (enabled: boolean) => invoke<void>('player_set_shuffle', { enabled }),
};

export const queueApi = {
  playEntry: (id: number) => invoke<void>('queue_play_entry', { id }),
  removeEntry: (id: number) => invoke<void>('queue_remove_entry', { id }),
  clear: () => invoke<void>('queue_clear'),
  /** Opens the native file dialog; resolves to the number of tracks queued. */
  openFiles: (mode: InsertMode) => invoke<number>('open_audio_files', { mode }),
  /** Opens the native folder dialog; resolves to the number of tracks queued. */
  openFolder: (mode: InsertMode) => invoke<number>('open_audio_folder', { mode }),
};

export const systemApi = {
  appInfo: () => invoke<AppInfo>('get_app_info'),
  preferences: () => invoke<UiPreferences>('get_ui_preferences'),
  updatePreferences: (patch: UiPreferencesPatch) => invoke<UiPreferences>('update_ui_preferences', { patch }),
};
