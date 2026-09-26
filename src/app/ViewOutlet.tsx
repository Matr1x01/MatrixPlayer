import { AlbumsView } from '@/features/library/AlbumsView';
import { AlbumView } from '@/features/library/AlbumView';
import { ArtistsView } from '@/features/library/ArtistsView';
import { FavoritesView, HomeView, PlaylistsView, PlaylistView, RecentView } from '@/features/library/CollectionViews';
import { FoldersView } from '@/features/library/FoldersView';
import { GenresView, GenreView } from '@/features/library/GenresView';
import { SongsView } from '@/features/library/SongsView';
import { NowPlayingView } from '@/features/now-playing/NowPlayingView';
import { QueueView } from '@/features/queue/QueueView';
import { SettingsView } from '@/features/settings/SettingsView';
import { DspView } from '@/features/sound/DspView';
import { EqualizerView } from '@/features/sound/EqualizerView';
import type { Route } from '@/stores/navigation';

export function ViewOutlet({ route }: { route: Route }) {
  switch (route.view) {
    case 'home':
      return <HomeView />;
    case 'albums':
      return <AlbumsView />;
    case 'album':
      return route.id != null ? <AlbumView id={route.id} /> : <AlbumsView />;
    case 'artists':
      return <ArtistsView id={route.id} />;
    case 'songs':
      return <SongsView />;
    case 'genres':
      return <GenresView />;
    case 'genre':
      return route.id != null ? <GenreView id={route.id} /> : <GenresView />;
    case 'folders':
      return <FoldersView id={route.id} />;
    case 'playlists':
      return <PlaylistsView />;
    case 'playlist':
      return route.id != null ? <PlaylistView id={route.id} /> : <PlaylistsView />;
    case 'favorites':
      return <FavoritesView />;
    case 'recent':
      return <RecentView />;
    case 'queue':
      return <QueueView />;
    case 'nowPlaying':
      return <NowPlayingView />;
    case 'equalizer':
      return <EqualizerView />;
    case 'dsp':
      return <DspView />;
    case 'settings':
      return <SettingsView initial={route.tab} />;
  }
}
