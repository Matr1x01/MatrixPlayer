// Mirror of the library's summary state: scan progress, totals, folders,
// favorites and playlists. Views fetch their own data through
// `useLibraryQuery`, which refetches when the relevant `versions` change.

import type { UnlistenFn } from '@tauri-apps/api/event';
import { create } from 'zustand';

import { onCoreEvent } from '@/services/events';
import { libraryApi, playlistApi } from '@/services/ipc';
import type { LibraryChange, LibraryStats, PlaylistRow, RootInfo, ScanStatus } from '@/types/library';

import { run, showToast } from './toasts';

interface LibraryStore {
  connected: boolean;
  scan: ScanStatus;
  stats: LibraryStats | null;
  roots: RootInfo[];
  favorites: ReadonlySet<number>;
  playlists: PlaylistRow[];
  /** Bumped on every change of that kind; queries depend on these. */
  versions: Record<LibraryChange, number>;
}

export const IDLE_SCAN: ScanStatus = {
  phase: 'idle',
  root: null,
  current: null,
  processed: 0,
  total: 0,
  found: 0,
  added: 0,
  updated: 0,
  removed: 0,
  unreadable: 0,
  offline: [],
  finishedAt: null,
};

export const useLibrary = create<LibraryStore>()(() => ({
  connected: false,
  scan: IDLE_SCAN,
  stats: null,
  roots: [],
  favorites: new Set(),
  playlists: [],
  versions: { tracks: 0, roots: 0, favorites: 0, history: 0, playlists: 0 },
}));

async function loadStats() {
  useLibrary.setState({ stats: await libraryApi.stats() });
}
async function loadRoots() {
  useLibrary.setState({ roots: await libraryApi.roots() });
}
async function loadFavorites() {
  useLibrary.setState({ favorites: new Set(await libraryApi.favoriteIds()) });
}
async function loadPlaylists() {
  useLibrary.setState({ playlists: await playlistApi.list() });
}

/** What to reload for each kind of change. */
const RELOAD: Record<LibraryChange, (() => Promise<void>)[]> = {
  tracks: [loadStats, loadRoots, loadFavorites, loadPlaylists],
  roots: [loadStats, loadRoots],
  favorites: [loadStats, loadFavorites],
  history: [],
  playlists: [loadStats, loadPlaylists],
};

function bump(kind: LibraryChange) {
  useLibrary.setState((s) => ({ versions: { ...s.versions, [kind]: s.versions[kind] + 1 } }));
  for (const reload of RELOAD[kind]) void reload().catch(() => undefined);
}

export async function connectLibrary(): Promise<UnlistenFn> {
  const unlisteners = await Promise.all([
    onCoreEvent('library-scan', (scan) => {
      const before = useLibrary.getState().scan;
      useLibrary.setState({ scan });
      if (scan.phase === 'idle' && before.phase !== 'idle' && scan.unreadable > 0) {
        showToast({ tone: 'error', message: `${scan.unreadable} folders could not be read`, detail: 'Check their permissions' });
      }
    }),
    onCoreEvent('library-changed', bump),
  ]);
  const [scan] = await Promise.all([libraryApi.scanStatus(), loadStats(), loadRoots(), loadFavorites(), loadPlaylists()]);
  useLibrary.setState({ scan, connected: true });
  return () => unlisteners.forEach((u) => u());
}

export function isScanning(scan: ScanStatus): boolean {
  return scan.phase !== 'idle';
}

/** Favorites change optimistically, so hearts respond instantly. */
export async function setFavorite(ids: number[], favorite: boolean) {
  const previous = useLibrary.getState().favorites;
  const next = new Set(previous);
  for (const id of ids) {
    if (favorite) next.add(id);
    else next.delete(id);
  }
  useLibrary.setState({ favorites: next });
  const done = await run(libraryApi.setFavorite(ids, favorite).then(() => true));
  if (!done) useLibrary.setState({ favorites: previous });
}

export function useIsFavorite(id: number | null | undefined): boolean {
  return useLibrary((s) => id != null && s.favorites.has(id));
}
