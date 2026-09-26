import type { IconName } from '@/components/icons/paths';
import type { ViewId } from '@/stores/navigation';

export interface NavItem {
  view: ViewId;
  label: string;
  icon: IconName;
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

/** Sidebar structure from the design, in order. */
export const NAV_GROUPS: NavGroup[] = [
  {
    label: 'Library',
    items: [
      { view: 'home', label: 'Home', icon: 'home' },
      { view: 'artists', label: 'Artists', icon: 'artist' },
      { view: 'albums', label: 'Albums', icon: 'album' },
      { view: 'songs', label: 'Songs', icon: 'songs' },
      { view: 'genres', label: 'Genres', icon: 'genre' },
      { view: 'folders', label: 'Folders', icon: 'folder' },
    ],
  },
  {
    label: 'Collections',
    items: [
      { view: 'playlists', label: 'Playlists', icon: 'playlist' },
      { view: 'favorites', label: 'Favorites', icon: 'heart' },
      { view: 'recent', label: 'Recently Played', icon: 'recent' },
      { view: 'queue', label: 'Queue', icon: 'queue' },
    ],
  },
  {
    label: 'Sound',
    items: [
      { view: 'equalizer', label: 'Equalizer', icon: 'eq' },
      { view: 'dsp', label: 'Audio / DSP', icon: 'dsp' },
    ],
  },
];

export const VIEW_TITLES: Record<ViewId, string> = {
  home: 'Home',
  artists: 'Artists',
  albums: 'Albums',
  album: 'Album',
  songs: 'Songs',
  genres: 'Genres',
  genre: 'Genre',
  folders: 'Folders',
  playlists: 'Playlists',
  playlist: 'Playlist',
  favorites: 'Favorites',
  recent: 'Recently Played',
  queue: 'Queue',
  equalizer: 'Equalizer',
  dsp: 'Audio / DSP',
  settings: 'Settings',
  nowPlaying: 'Now Playing',
};
