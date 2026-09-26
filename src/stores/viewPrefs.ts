// How each library view was last arranged (sort, filter chips, tile size).
// These are per-machine conveniences, so they live in local storage rather
// than the library database.

import { create } from 'zustand';

import type { ArtistKind, TrackSort } from '@/types/library';

export type AlbumSort = 'artist' | 'title' | 'year' | 'added' | 'played';
export type AlbumFilter = 'all' | 'hires' | 'lossless' | 'lossy' | 'recent' | 'unplayed';

export interface ViewPrefs {
  albumSort: AlbumSort;
  albumFilter: AlbumFilter;
  tileSize: number;
  songSort: TrackSort;
  songDescending: boolean;
  songHiRes: boolean;
  artistKind: ArtistKind;
  folderShowFiles: boolean;
}

const DEFAULTS: ViewPrefs = {
  albumSort: 'artist',
  albumFilter: 'all',
  tileSize: 168,
  songSort: 'artist',
  songDescending: false,
  songHiRes: false,
  artistKind: 'albumArtists',
  folderShowFiles: false,
};

const KEY = 'matrixplayer.viewPrefs';

function load(): ViewPrefs {
  try {
    const stored = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<ViewPrefs>;
    return { ...DEFAULTS, ...stored };
  } catch {
    return DEFAULTS;
  }
}

export const useViewPrefs = create<ViewPrefs>()(() => load());

export function setViewPrefs(patch: Partial<ViewPrefs>) {
  useViewPrefs.setState(patch);
  try {
    localStorage.setItem(KEY, JSON.stringify(useViewPrefs.getState()));
  } catch {
    // Storage may be unavailable; the preference then lasts for the session.
  }
}
