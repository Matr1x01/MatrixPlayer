import { useMemo, useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import { PageHeader } from '@/components/shell/PageHeader';
import { Artwork } from '@/components/ui/Artwork';
import { contextMenu, type MenuItem } from '@/components/ui/ContextMenu';
import { EmptyCard } from '@/components/ui/EmptyState';
import { useLibraryQuery } from '@/hooks/useLibraryQuery';
import { libraryApi } from '@/services/ipc';
import { useLibrary } from '@/stores/library';
import { navigate } from '@/stores/navigation';
import { useViewPrefs } from '@/stores/viewPrefs';
import type { GenreRow } from '@/types/library';

import { playScope } from './actions';
import { AlbumGrid, scopeTrackIds } from './albums';
import { fold } from './AlbumsView';
import { ArtistAvatar } from './ArtistsView';
import { FilterField, useDebounced } from './controls';
import { count, formatLength, plural } from './format';
import { LibraryWelcome } from './LibraryWelcome';
import { ScanBanner } from './ScanBanner';
import { playlistSubmenu } from './trackMenu';

function genreMenu(genre: GenreRow): MenuItem[] {
  const scope = { kind: 'genre' as const, id: genre.id };
  return [
    { label: 'Play', icon: 'play', filledIcon: true, onSelect: () => void playScope(scope) },
    { label: 'Shuffle', icon: 'shuffle', onSelect: () => void playScope(scope, 'replace', true) },
    { label: 'Add to queue', icon: 'queue', onSelect: () => void playScope(scope, 'append', undefined, genre.name) },
    'separator',
    { label: 'Add to playlist', icon: 'playlist', submenu: playlistSubmenu(() => scopeTrackIds(scope)) },
  ];
}

/** Up to four covers of the genre's biggest albums, as a 2×2 mosaic. */
function GenreMosaic({ genre, size }: { genre: GenreRow; size?: number }) {
  const covers = genre.artworkIds.slice(0, 4);
  if (covers.length < 4) return <Artwork artworkId={covers[0] ?? null} size={size} title={genre.name} />;
  return (
    <span className="mosaic" style={size ? { width: size, height: size } : undefined}>
      {covers.map((id) => (
        <Artwork key={id} artworkId={id} />
      ))}
    </span>
  );
}

export function GenresView() {
  const { data: genres, loading } = useLibraryQuery('genres', libraryApi.genres);
  const [filter, setFilter] = useState('');
  const text = fold(useDebounced(filter).trim());
  const stats = useLibrary((s) => s.stats);
  const connected = useLibrary((s) => s.connected);
  const hasRoots = useLibrary((s) => s.roots.length > 0);
  const shown = useMemo(() => (genres ?? []).filter((g) => !text || fold(g.name).includes(text)), [genres, text]);

  if (connected && !hasRoots && !genres?.length) {
    return (
      <div className="view">
        <LibraryWelcome />
      </div>
    );
  }
  return (
    <div className="view">
      <PageHeader
        section="Library"
        title="Genres"
        meta={stats && `${count(stats.genres)} genres · gathered from your tags`}
        actions={<FilterField value={filter} onChange={setFilter} placeholder="Filter genres" />}
      />
      <ScanBanner />
      <div className="view-body border-t border-ln">
        {!loading && shown.length === 0 ? (
          <div className="flex justify-center pt-16">
            <EmptyCard icon="genre" title={text ? 'No genres match' : 'No genres yet'} className="w-[340px]">
              {text ? 'Try another filter.' : 'Genres are gathered from your tags as folders are indexed.'}
            </EmptyCard>
          </div>
        ) : (
          <div className="genre-grid">
            {shown.map((g, i) => (
              <button
                key={g.id}
                type="button"
                className="genre-tile"
                style={{ ['--i' as string]: Math.min(i, 24) }}
                onClick={() => navigate('genre', g.id)}
                onContextMenu={contextMenu(() => genreMenu(g), { label: `${g.name} actions` })}
              >
                <span className="genre-art">
                  <GenreMosaic genre={g} />
                  <span className="genre-name">{g.name}</span>
                </span>
                <span className="mono t3 el text-[10.5px]">
                  {plural(g.albumCount, 'album')} · {count(g.trackCount)} trk
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export function GenreView({ id }: { id: number }) {
  const { data, error } = useLibraryQuery(`genre:${id}`, () => libraryApi.genre(id));
  const tileSize = useViewPrefs((s) => s.tileSize);
  if (error) {
    return (
      <div className="view items-center justify-center">
        <EmptyCard icon="genre" title="Genre not found">It is no longer used by any track.</EmptyCard>
      </div>
    );
  }
  if (!data) return <div className="view" />;
  const { genre } = data;
  return (
    <div className="view">
      <section className="genre-hero">
        <GenreMosaic genre={genre} size={132} />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <span className="cap">Genre</span>
          <h1 className="hero-title el">{genre.name}</h1>
          <span className="mono t3 text-[11.5px]">
            {plural(genre.albumCount, 'album')} · {plural(genre.trackCount, 'track')} · {formatLength(genre.durationMs)}
          </span>
          <div className="mt-1 flex gap-2">
            <button type="button" className="btn pri h36" onClick={() => void playScope({ kind: 'genre', id })}>
              <Icon name="play" size={16} filled />
              Play
            </button>
            <button type="button" className="btn h36" onClick={() => void playScope({ kind: 'genre', id }, 'replace', true)}>
              <Icon name="shuffle" size={16} />
              Shuffle
            </button>
          </div>
        </div>
        {data.artists.length > 0 && (
          <div className="flex max-w-[420px] flex-col gap-2">
            <span className="cap">Artists</span>
            <div className="flex flex-wrap gap-1.5">
              {data.artists.slice(0, 8).map((a) => (
                <button key={a.id} type="button" className="artist-chip" onClick={() => navigate('artists', a.id)}>
                  <ArtistAvatar name={a.name} artworkId={a.artworkId} size={22} />
                  <span className="el">{a.name}</span>
                </button>
              ))}
            </div>
          </div>
        )}
      </section>
      <AlbumGrid albums={data.albums} tileSize={tileSize} />
    </div>
  );
}
