import { PageHeader } from '@/components/shell/PageHeader';
import { VIEW_TITLES } from '@/components/shell/navItems';
import { EmptyCard } from '@/components/ui/EmptyState';
import type { IconName } from '@/components/icons/paths';
import type { ViewId } from '@/stores/navigation';

import { LibraryWelcome } from './LibraryWelcome';
import { openFiles, openFolder } from './openActions';

type LibraryView = Extract<ViewId, 'artists' | 'albums' | 'songs' | 'genres' | 'folders' | 'playlists' | 'favorites' | 'recent'>;

const EMPTY: Record<LibraryView, { section: string; icon: IconName; title: string; body: string }> = {
  albums: { section: 'Library', icon: 'album', title: 'No albums yet', body: 'Albums appear here once your music folders are indexed.' },
  artists: { section: 'Library', icon: 'artist', title: 'No artists yet', body: 'Artists appear here once your music folders are indexed.' },
  songs: { section: 'Library', icon: 'songs', title: 'No songs yet', body: 'Every indexed track will be listed here, with its format and audio details.' },
  genres: { section: 'Library', icon: 'genre', title: 'No genres yet', body: 'Genres are gathered from your tags when the library is indexed.' },
  folders: { section: 'Library', icon: 'folder', title: 'No library folders', body: 'Folder browsing arrives with library indexing. You can already play a folder directly.' },
  playlists: { section: 'Collections', icon: 'playlist', title: 'No playlists yet', body: 'Playlists arrive with the library. For now, build a session in the queue.' },
  favorites: { section: 'Collections', icon: 'heart', title: 'No favorites yet', body: 'Favorites are stored in your library, which is coming next.' },
  recent: { section: 'Collections', icon: 'recent', title: 'Nothing played yet', body: 'Your listening history will be recorded once the library is available.' },
};

export function HomeView() {
  return (
    <div className="view">
      <LibraryWelcome />
    </div>
  );
}

/** Library and collection views before library indexing exists. */
export function LibraryPlaceholderView({ view }: { view: LibraryView }) {
  const empty = EMPTY[view];
  return (
    <div className="view">
      <PageHeader section={empty.section} title={VIEW_TITLES[view]} />
      <div className="view-body flex items-start justify-center border-t border-ln pt-16">
        <EmptyCard
          icon={empty.icon}
          title={empty.title}
          className="w-[340px]"
          actions={
            <>
              <button type="button" className="btn pri" onClick={() => void openFolder()}>
                Open music folder…
              </button>
              <button type="button" className="btn" style={{ fontWeight: 500 }} onClick={() => void openFiles()}>
                Open files…
              </button>
            </>
          }
        >
          {empty.body}
        </EmptyCard>
      </div>
    </div>
  );
}

export type { LibraryView };
