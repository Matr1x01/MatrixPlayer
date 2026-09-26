// Development-only UI harness: runs the real frontend against mocked IPC so
// views can be rendered (and screenshotted) in an ordinary browser without
// the Rust core. Served by the Vite dev server at /dev/harness.html; never
// part of a production build. The sample data mirrors the design boards.
//
// Query parameters: view=<ViewId> id=<n> tab=<tab> theme=<Theme> accent=<Accent>
// empty=1 (no music), noroots=1 (first run), scanning=1, search=<text>, menu=1

import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';

import { useNavigation, type ViewId } from '@/stores/navigation';

import * as lib from './mockLibrary';
import type { PlayerSnapshot, QueueEntry, TrackInfo } from '@/types/player';
import type { UiPreferences } from '@/types/preferences';

const params = new URLSearchParams(location.search);

const ART: Record<string, string> = {
  ...lib.ART,
  lfw: svg(`
    <linearGradient id="a" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1b2a40"/><stop offset=".55" stop-color="#3c3a55"/><stop offset="1" stop-color="#151a26"/></linearGradient>
    <radialGradient id="b" cx="78" cy="24" r="36" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#f4dca8"/><stop offset="1" stop-color="#f4dca8" stop-opacity="0"/></radialGradient>
    <radialGradient id="c" cx="18" cy="112" r="72" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#ec8a3a"/><stop offset=".5" stop-color="#a3402b"/><stop offset="1" stop-color="#a3402b" stop-opacity="0"/></radialGradient>`,
    ['a', 'b', 'c']),
  vane: svg(`
    <linearGradient id="a" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#edd3ae"/><stop offset="1" stop-color="#d98e5a"/></linearGradient>
    <radialGradient id="b" cx="50" cy="118" r="46" gradientUnits="userSpaceOnUse"><stop offset=".99" stop-color="#b5542f"/><stop offset="1" stop-color="#b5542f" stop-opacity="0"/></radialGradient>`,
    ['a', 'b']),
};

function svg(defs: string, fills: string[]): string {
  const rects = fills.map((id) => `<rect width="100" height="100" fill="url(#${id})"/>`).join('');
  return `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><defs>${defs}</defs>${rects}</svg>`)}`;
}

function track(title: string, n: number, ms: number, over: Partial<TrackInfo['metadata']> = {}, art = 'lfw'): TrackInfo {
  return {
    library: { trackId: n, albumId: 3, artistId: 3, folderId: 10 },
    displayPath: `~/Music/Electronic/Halden Reach/2021 - Low Frequency Weather/${String(n).padStart(2, '0')} - ${title}.flac`,
    fileName: `${String(n).padStart(2, '0')} - ${title}.flac`,
    metadata: {
      title,
      artist: 'Halden Reach',
      album: 'Low Frequency Weather',
      albumArtist: 'Halden Reach',
      genre: 'Ambient',
      year: 2021,
      trackNumber: n,
      trackTotal: 10,
      discNumber: 1,
      discTotal: 1,
      composer: null,
      comment: null,
      copyright: null,
      replayGain: { trackGain: -5.1, trackPeak: 0.991, albumGain: -6.2, albumPeak: 0.991 },
      tagFormat: 'Vorbis comments',
      tagItemCount: 14,
      derivedFields: [],
      ...over,
    },
    properties: {
      codec: 'FLAC',
      container: 'FLAC',
      lossless: true,
      sampleRate: 96000,
      bitDepth: 24,
      channels: 2,
      bitrateKbps: 3104,
      durationMs: ms,
      fileSize: Math.round(ms * 388),
    },
    artwork: {
      id: art,
      width: 1200,
      height: 1200,
      source: 'embedded',
      palette: art === 'lfw' ? [[163, 64, 43], [90, 86, 130], [236, 138, 58]] : [[217, 142, 90], [237, 211, 174], [181, 84, 47]],
    },
    probeError: null,
  };
}

const album: [string, number][] = [
  ['Pressure Front', 258000],
  ['Isobar', 342186],
  ['Cloud Ceiling', 236000],
  ['Doldrums', 370000],
  ['Barometer', 273000],
  ['Katabatic', 422000],
  ['Rain Shadow', 201000],
  ['Dew Point', 315000],
  ['Lee Wave', 287000],
  ['Clear Night, Still Air', 484000],
];
const entries: QueueEntry[] = album.map(([title, ms], i) => ({ id: i + 1, track: track(title, i + 1, ms), added: false }));
entries.push(
  { id: 20, track: track('Harvest Line', 3, 242000, { artist: 'Tarn & Vale', album: 'Weathervane', year: 2024 }, 'vane'), added: true },
  { id: 21, track: track('Weathervane', 5, 337000, { artist: 'Tarn & Vale', album: 'Weathervane', year: 2024 }, 'vane'), added: true },
);

const empty = params.has('empty');
const noRoots = params.has('noroots');
const snapshot: PlayerSnapshot = {
  status: empty ? 'idle' : 'playing',
  track: empty ? null : entries[1]!.track,
  positionMs: empty ? 0 : 134000,
  durationMs: empty ? null : 342186,
  volume: { level: 0.9, muted: false, gainDb: 60 * Math.log10(0.9) },
  queue: { entries: empty ? [] : entries, currentIndex: empty ? null : 1, shuffle: true, repeat: 'off' },
  signal: {
    backend: 'pipeWire',
    decoded: empty ? null : { sampleFormat: 'S24_32LE', bits: 24, float: false, sampleRate: 96000, channels: 2 },
    output: empty ? null : { sampleFormat: 'F32LE', bits: 32, float: true, sampleRate: 96000, channels: 2 },
  },
  error: null,
};

const prefs: UiPreferences = {
  theme: (params.get('theme') as UiPreferences['theme']) ?? 'graphite',
  accent: (params.get('accent') as UiPreferences['accent']) ?? null,
  ambient: 'subtle',
  density: 'compact',
  sidebarExpanded: true,
  reduceMotion: false,
  interfaceScale: 1,
};

mockWindows('main');
mockIPC(
  (cmd, args) => {
    switch (cmd) {
      case 'get_player_snapshot':
        return snapshot;
      case 'get_ui_preferences':
        return prefs;
      case 'update_ui_preferences':
        return Object.assign(prefs, (args as { patch: Partial<UiPreferences> }).patch);
      case 'get_app_info':
        return { name: 'MatrixPlayer', version: '0.1.0', gstreamerVersion: 'GStreamer 1.28.7' };
      case 'plugin:window|is_fullscreen':
        return false;
      case 'library_stats':
        return noRoots ? { ...lib.STATS, tracks: 0, albums: 0, roots: 0 } : lib.STATS;
      case 'library_scan_status':
        return params.has('scanning') ? lib.SCANNING : lib.IDLE_SCAN;
      case 'library_roots':
        return noRoots ? [] : lib.ROOTS;
      case 'library_default_root':
        return { display: '~/Music', files: 12408 };
      case 'library_albums':
        return noRoots || params.has('scanning') ? [] : lib.ALBUMS;
      case 'library_album':
        return lib.albumDetail((args as { id: number }).id);
      case 'library_artists':
        return lib.artistRows();
      case 'library_artist':
        return lib.artistDetail((args as { id: number }).id);
      case 'library_genres':
        return lib.GENRES;
      case 'library_genre':
        return { genre: lib.GENRES[0], albums: lib.ALBUMS.slice(0, 9), artists: lib.artistRows().slice(0, 6) };
      case 'library_folder_children':
        return lib.FOLDER_CHILDREN[(args as { id: number }).id] ?? [];
      case 'library_folder':
        return lib.folderDetail((args as { id: number }).id);
      case 'library_track_list':
        return lib.trackList((args as { query: Parameters<typeof lib.trackList>[0] }).query);
      case 'library_tracks':
        return (args as { ids: number[] }).ids.map((id) => lib.TRACKS.get(id)).filter(Boolean);
      case 'library_search':
        return lib.search((args as { text: string }).text);
      case 'library_favorite_ids':
        return lib.FAVORITES;
      case 'library_history':
        return lib.history();
      case 'playlists_list':
        return lib.PLAYLISTS;
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);
window.__TAURI_INTERNALS__.convertFileSrc = (id: string) => ART[id.replace(/\/thumb$/, '')] ?? '';

const view = params.get('view') as ViewId | null;
const id = params.get('id');
if (view) useNavigation.getState().navigate(view, id ? Number(id) : undefined, params.get('tab') ?? undefined);

void import('../main').then(async () => {
  const text = params.get('search');
  if (text != null) {
    const { openSearch } = await import('@/features/search/SearchDialog');
    window.setTimeout(() => openSearch(text), 300);
  }
  if (params.has('menu')) {
    window.setTimeout(() => {
      const row = document.querySelectorAll<HTMLElement>('.tr-row:not(.tr-head)')[8];
      if (!row) return;
      const r = row.getBoundingClientRect();
      row.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: r.left + 200, clientY: r.top + 10, button: 2 }));
      row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: r.left + 200, clientY: r.top + 10, button: 2 }));
    }, 900);
  }
});

declare global {
  interface Window {
    __TAURI_INTERNALS__: { convertFileSrc: (path: string, protocol?: string) => string };
  }
}
