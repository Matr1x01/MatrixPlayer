// In-app navigation with back/forward history, as driven by the title bar
// arrows and Alt+Left / Alt+Right. A route is a view plus, for detail views,
// the id of what it shows (an album, an artist, a folder…).

import { create } from 'zustand';

export type ViewId =
  | 'home'
  | 'artists'
  | 'albums'
  | 'album'
  | 'songs'
  | 'genres'
  | 'genre'
  | 'folders'
  | 'playlists'
  | 'playlist'
  | 'favorites'
  | 'recent'
  | 'queue'
  | 'equalizer'
  | 'dsp'
  | 'settings'
  | 'nowPlaying';

export interface Route {
  view: ViewId;
  /** The item a detail view shows; for Artists and Folders, the selection. */
  id?: number;
  /** A sub-page, such as a settings section or an artist tab. */
  tab?: string;
}

const HISTORY_LIMIT = 80;

/** Where each detail view belongs in the sidebar. */
export const PARENT_VIEW: Partial<Record<ViewId, ViewId>> = {
  album: 'albums',
  genre: 'genres',
  playlist: 'playlists',
};

interface NavigationStore {
  history: Route[];
  index: number;
  navigate: (view: ViewId, id?: number, tab?: string) => void;
  /** Changes the current route without adding a history step. */
  replace: (route: Route) => void;
  back: () => void;
  forward: () => void;
}

const same = (a: Route | undefined, b: Route) => a?.view === b.view && a.id === b.id && a.tab === b.tab;

export const useNavigation = create<NavigationStore>()((set) => ({
  history: [{ view: 'home' }],
  index: 0,
  navigate: (view, id, tab) =>
    set((s) => {
      const route: Route = { view, id, tab };
      if (same(s.history[s.index], route)) return s;
      const history = [...s.history.slice(0, s.index + 1), route].slice(-HISTORY_LIMIT);
      return { history, index: history.length - 1 };
    }),
  replace: (route) =>
    set((s) => {
      const history = [...s.history];
      history[s.index] = route;
      return { history };
    }),
  back: () => set((s) => ({ index: Math.max(0, s.index - 1) })),
  forward: () => set((s) => ({ index: Math.min(s.history.length - 1, s.index + 1) })),
}));

export function useRoute(): Route {
  return useNavigation((s) => s.history[s.index] ?? { view: 'home' });
}

export function useView(): ViewId {
  return useNavigation((s) => s.history[s.index]?.view ?? 'home');
}

/** Shorthand for components that only navigate. */
export function navigate(view: ViewId, id?: number, tab?: string) {
  useNavigation.getState().navigate(view, id, tab);
}
