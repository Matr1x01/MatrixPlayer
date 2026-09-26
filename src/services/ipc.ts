// Typed wrappers for every Tauri command. Components call these, never
// `invoke` directly, so the command surface is listed in one place.

import { invoke } from '@tauri-apps/api/core';

import type {
  AlbumDetail,
  AlbumRow,
  ArtistDetail,
  ArtistKind,
  ArtistRow,
  DefaultRoot,
  FolderDetail,
  FolderNode,
  GenreDetail,
  GenreRow,
  HistoryEntry,
  LibraryStats,
  PlaylistRow,
  PlaySource,
  RootInfo,
  ScanStatus,
  SearchResults,
  TrackList,
  TrackQuery,
  TrackRow,
} from '@/types/library';
import type { DspSettings, DspState, Lyrics, OutputDevices } from '@/types/dsp';
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
  setGapless: (enabled: boolean) => invoke<void>('player_set_gapless', { enabled }),
  /** Lyrics of the current track (sidecar .lrc or tags), or null. */
  lyrics: () => invoke<Lyrics | null>('player_lyrics'),
  /** Peak levels (0–255) of the current track in `points` columns. */
  waveform: (points: number) => invoke<number[] | null>('player_waveform', { points }),
};

export const dspApi = {
  state: () => invoke<DspState>('dsp_state'),
  /** Applies settings immediately; resolves to them as applied (clamped). */
  update: (settings: DspSettings) => invoke<DspSettings>('dsp_update', { settings }),
  /** Resets processing to defaults, keeping the equalizer. */
  restoreDefaults: () => invoke<DspSettings>('dsp_restore_defaults'),
  savePreset: (name: string) => invoke<DspState>('dsp_save_preset', { name }),
  deletePreset: (name: string) => invoke<DspState>('dsp_delete_preset', { name }),
  /** Native save dialog; resolves to the number exported, or null if cancelled. */
  exportPresets: () => invoke<number | null>('dsp_export_presets'),
  /** Native open dialog; resolves to the number imported, or null if cancelled. */
  importPresets: () => invoke<number | null>('dsp_import_presets'),
  /** Starts or stops the `audio-analysis` event stream. */
  setAnalysis: (enabled: boolean) => invoke<void>('dsp_set_analysis', { enabled }),
};

export const outputApi = {
  devices: () => invoke<OutputDevices>('audio_output_devices'),
  /** null follows the system default. */
  setDevice: (id: string | null) => invoke<void>('player_set_output_device', { id }),
};

export const queueApi = {
  playEntry: (id: number) => invoke<void>('queue_play_entry', { id }),
  removeEntry: (id: number) => invoke<void>('queue_remove_entry', { id }),
  clear: () => invoke<void>('queue_clear'),
  clearOthers: () => invoke<void>('queue_clear_others'),
  savedSession: () => invoke<{ tracks: number } | null>('queue_saved_session'),
  /** Restores the queue from the last run, paused where it was. */
  restoreSession: () => invoke<number>('queue_restore_session'),
  /** Moves entries before `before`, or to the end when it is null. */
  move: (ids: number[], before: number | null) => invoke<void>('queue_move', { ids, before }),
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

export const libraryApi = {
  stats: () => invoke<LibraryStats>('library_stats'),
  scanStatus: () => invoke<ScanStatus>('library_scan_status'),
  roots: () => invoke<RootInfo[]>('library_roots'),
  /** Opens the native folder dialog; resolves to the new root's id, or null. */
  addRoot: () => invoke<number | null>('library_add_root'),
  addDefaultRoot: () => invoke<number>('library_add_default_root'),
  defaultRoot: () => invoke<DefaultRoot | null>('library_default_root'),
  removeRoot: (id: number) => invoke<void>('library_remove_root', { id }),
  /** `full` re-reads the tags of unchanged files too. */
  rescan: (root?: number, full = false) => invoke<void>('library_rescan', { root: root ?? null, full }),
  albums: () => invoke<AlbumRow[]>('library_albums'),
  album: (id: number) => invoke<AlbumDetail>('library_album', { id }),
  artists: (kind: ArtistKind) => invoke<ArtistRow[]>('library_artists', { kind }),
  artist: (id: number) => invoke<ArtistDetail>('library_artist', { id }),
  genres: () => invoke<GenreRow[]>('library_genres'),
  genre: (id: number) => invoke<GenreDetail>('library_genre', { id }),
  folderChildren: (id: number) => invoke<FolderNode[]>('library_folder_children', { id }),
  folder: (id: number) => invoke<FolderDetail>('library_folder', { id }),
  trackList: (query: TrackQuery) => invoke<TrackList>('library_track_list', { query }),
  tracks: (ids: number[]) => invoke<TrackRow[]>('library_tracks', { ids }),
  search: (text: string, limit?: number) => invoke<SearchResults>('library_search', { text, limit: limit ?? null }),
  play: (source: PlaySource, mode: InsertMode, start?: number, shuffle?: boolean) =>
    invoke<number>('library_play', { source, mode, start: start ?? null, shuffle: shuffle ?? null }),
  setFavorite: (ids: number[], favorite: boolean) => invoke<void>('library_set_favorite', { ids, favorite }),
  favoriteIds: () => invoke<number[]>('library_favorite_ids'),
  history: (limit?: number) => invoke<HistoryEntry[]>('library_history', { limit: limit ?? null }),
  clearHistory: () => invoke<void>('library_clear_history'),
  revealFolder: (id: number) => invoke<void>('library_reveal_folder', { id }),
  trackFolder: (id: number) => invoke<number>('library_track_folder', { id }),
};

export const playlistApi = {
  list: () => invoke<PlaylistRow[]>('playlists_list'),
  get: (id: number) => invoke<PlaylistRow>('playlist_get', { id }),
  create: (name: string, tracks?: number[]) => invoke<number>('playlist_create', { name, tracks: tracks ?? null }),
  rename: (id: number, name: string) => invoke<void>('playlist_rename', { id, name }),
  setPinned: (id: number, pinned: boolean) => invoke<void>('playlist_set_pinned', { id, pinned }),
  remove: (id: number) => invoke<void>('playlist_delete', { id }),
  add: (id: number, tracks: number[]) => invoke<number>('playlist_add', { id, tracks }),
  removeEntries: (id: number, positions: number[]) => invoke<void>('playlist_remove', { id, positions }),
  move: (id: number, positions: number[], to: number) => invoke<void>('playlist_move', { id, positions, to }),
};
