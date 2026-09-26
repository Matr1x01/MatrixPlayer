// In-app navigation with back/forward history, as driven by the title bar
// arrows and Alt+Left / Alt+Right.

import { create } from 'zustand';

export type ViewId =
  | 'home'
  | 'artists'
  | 'albums'
  | 'songs'
  | 'genres'
  | 'folders'
  | 'playlists'
  | 'favorites'
  | 'recent'
  | 'queue'
  | 'equalizer'
  | 'dsp'
  | 'settings'
  | 'nowPlaying';

const HISTORY_LIMIT = 50;

interface NavigationStore {
  history: ViewId[];
  index: number;
  navigate: (view: ViewId) => void;
  back: () => void;
  forward: () => void;
}

export const useNavigation = create<NavigationStore>()((set) => ({
  history: ['home'],
  index: 0,
  navigate: (view) =>
    set((s) => {
      if (s.history[s.index] === view) return s;
      const history = [...s.history.slice(0, s.index + 1), view].slice(-HISTORY_LIMIT);
      return { history, index: history.length - 1 };
    }),
  back: () => set((s) => ({ index: Math.max(0, s.index - 1) })),
  forward: () => set((s) => ({ index: Math.min(s.history.length - 1, s.index + 1) })),
}));

export function useView(): ViewId {
  return useNavigation((s) => s.history[s.index] ?? 'home');
}
