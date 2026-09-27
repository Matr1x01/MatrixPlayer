import { memo, useMemo, type CSSProperties, type ReactNode } from 'react';

import { Icon } from '@/components/icons/Icon';
import { Artwork } from '@/components/ui/Artwork';
import { contextMenu, type MenuItem } from '@/components/ui/ContextMenu';
import { EqIndicator } from '@/components/ui/EqIndicator';
import { useVirtualGrid } from '@/hooks/useVirtual';
import { libraryApi } from '@/services/ipc';
import { setFavorite } from '@/stores/library';
import { navigate } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import type { AlbumRow, TrackScope } from '@/types/library';

import { playScope } from './actions';
import { albumBadge, badgeClass, indexLetter, plural } from './format';
import { playlistSubmenu } from './trackMenu';

/** Track ids of a scope, for actions that need them (favorites, playlists). */
export async function scopeTrackIds(scope: TrackScope): Promise<number[]> {
  return (await libraryApi.trackList({ scope })).ids;
}

export function albumMenu(album: AlbumRow): MenuItem[] {
  const scope: TrackScope = { kind: 'album', id: album.id };
  const withIds = (act: (ids: number[]) => void) => () => void scopeTrackIds(scope).then(act);
  return [
    { label: 'Play', icon: 'play', filledIcon: true, onSelect: () => void playScope(scope) },
    { label: 'Shuffle', icon: 'shuffle', onSelect: () => void playScope(scope, 'replace', true) },
    { label: 'Play next', icon: 'playNext', onSelect: () => void playScope(scope, 'next', undefined, album.title) },
    { label: 'Add to queue', icon: 'queue', onSelect: () => void playScope(scope, 'append', undefined, album.title) },
    'separator',
    { label: 'Add to playlist', icon: 'playlist', submenu: playlistSubmenu(() => scopeTrackIds(scope)) },
    { label: 'Add to favorites', icon: 'heart', onSelect: withIds((ids) => void setFavorite(ids, true)) },
    'separator',
    { label: 'Open album', icon: 'album', onSelect: () => navigate('album', album.id) },
    {
      label: 'Open artist',
      icon: 'artist',
      disabled: album.artistId == null,
      onSelect: () => album.artistId != null && navigate('artists', album.artistId),
    },
  ];
}

export function usePlayingAlbum(): { albumId: number | null; paused: boolean } {
  const albumId = usePlayer((s) => s.track?.library?.albumId ?? null);
  const paused = usePlayer((s) => s.status !== 'playing' && s.status !== 'loading');
  return { albumId, paused };
}

interface AlbumTileProps {
  album: AlbumRow;
  playing: boolean;
  paused: boolean;
  /** Second line: the artist (default) or the year and type. */
  subtitle?: 'artist' | 'year';
  style?: CSSProperties;
  index?: number;
}

/** A square cover with title, artist and format (Library — Albums board). */
export const AlbumTile = memo(function AlbumTile({ album, playing, paused, subtitle = 'artist', style, index = 0 }: AlbumTileProps) {
  const badge = albumBadge(album);
  const open = () => navigate('album', album.id);
  return (
    <div
      className={playing ? 'tile now' : 'tile'}
      style={{ ...style, ['--i' as string]: Math.min(index, 24) }}
      onContextMenu={contextMenu(() => albumMenu(album), { label: `${album.title} actions` })}
    >
      <div className="tile-art" role="button" tabIndex={0} aria-label={`Open ${album.title}`} onClick={open} onKeyDown={(e) => e.key === 'Enter' && open()}>
        <Artwork artworkId={album.artworkId} title={album.title} />
        {playing && (
          <span className="tile-badge">
            <EqIndicator paused={paused} />
            {paused ? 'Paused' : 'Playing'}
          </span>
        )}
        <span className="tile-hover">
          <button
            type="button"
            className="play tile-play"
            aria-label={`Play ${album.title}`}
            onClick={(e) => {
              e.stopPropagation();
              void playScope({ kind: 'album', id: album.id });
            }}
          >
            <Icon name="play" filled />
          </button>
          <span className="flex gap-1.5">
            <button
              type="button"
              className="ib tile-mini"
              aria-label="Add to queue"
              data-tip="Add to queue"
              onClick={(e) => {
                e.stopPropagation();
                void playScope({ kind: 'album', id: album.id }, 'append', undefined, album.title);
              }}
            >
              <Icon name="playNext" size={16} />
            </button>
            <button
              type="button"
              className="ib tile-mini"
              aria-label="More album actions"
              onClick={(e) => {
                e.stopPropagation();
                contextMenu(() => albumMenu(album), { label: `${album.title} actions` })(e);
              }}
            >
              <Icon name="more" size={16} strokeWidth={2.8} />
            </button>
          </span>
        </span>
      </div>
      <div className="flex min-w-0 flex-col gap-0.5">
        <button type="button" className="ttl el text-left" onClick={open} title={album.title}>
          {album.title}
        </button>
        {subtitle === 'artist' ? (
          <button
            type="button"
            className="el t2 text-left text-[12px] hover:underline"
            disabled={album.artistId == null}
            onClick={() => album.artistId != null && navigate('artists', album.artistId)}
          >
            {album.artistName}
          </button>
        ) : (
          <span className="el t2 text-[12px]">{plural(album.trackCount, 'track')}</span>
        )}
        <div className="mt-1 flex items-center gap-1.5">
          {album.year && <span className="mono t3 text-[10.5px]">{album.year}</span>}
          {badge && <span className={badgeClass(album)}>{badge}</span>}
          {album.unavailable > 0 && (
            <span className="chip warn" data-tip="On a drive that is not connected">
              OFFLINE
            </span>
          )}
        </div>
      </div>
    </div>
  );
});

const CAPTION = 72;
const LETTERS = '#ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('');

interface AlbumGridProps {
  albums: AlbumRow[];
  /** Smallest tile width in px (the design's size slider). */
  tileSize: number;
  /** Which key the A–Z index jumps by; omit to hide the index. */
  indexBy?: (album: AlbumRow) => string;
  empty?: ReactNode;
}

/** The virtualized album grid with its A–Z jump index. */
export function AlbumGrid({ albums, tileSize, indexBy, empty }: AlbumGridProps) {
  const grid = useVirtualGrid<HTMLDivElement>(albums.length, {
    minTile: tileSize,
    gap: 20,
    rowGap: 24,
    caption: CAPTION,
    paddingX: 28 + (indexBy ? 56 : 28),
    paddingTop: 20,
  });
  const { albumId, paused } = usePlayingAlbum();
  const letters = useMemo(() => {
    const first = new Map<string, number>();
    if (indexBy) albums.forEach((a, i) => !first.has(indexLetter(indexBy(a))) && first.set(indexLetter(indexBy(a)), i));
    return first;
  }, [albums, indexBy]);
  const currentLetter = indexBy && albums[grid.start + grid.columns * 2] ? indexLetter(indexBy(albums[Math.min(albums.length - 1, grid.start + grid.columns * 2)]!)) : null;

  if (albums.length === 0 && empty) return <div className="flex flex-1 items-start justify-center pt-16">{empty}</div>;

  const tiles = [];
  for (let i = grid.start; i < grid.end; i++) {
    const album = albums[i]!;
    tiles.push(<AlbumTile key={album.id} album={album} playing={album.id === albumId} paused={paused} index={i - grid.start} />);
  }
  return (
    <div className="relative flex min-h-0 flex-1">
      <div ref={grid.ref} className="grid-scroll" style={{ paddingRight: indexBy ? 56 : 28 }}>
        <div style={{ height: grid.totalHeight + 40, position: 'relative' }}>
          <div
            className="album-grid"
            style={{
              transform: `translateY(${grid.offset}px)`,
              gridTemplateColumns: `repeat(${grid.columns}, minmax(0, 1fr))`,
              ['--tile' as string]: `${grid.tileWidth}px`,
            }}
          >
            {tiles}
          </div>
        </div>
      </div>
      {indexBy && albums.length > 24 && (
        <nav className="az" aria-label="Jump to letter">
          {LETTERS.map((l) => {
            const at = letters.get(l);
            return (
              <button
                key={l}
                type="button"
                className={['mono', l === currentLetter && 'on'].filter(Boolean).join(' ')}
                disabled={at == null}
                onClick={() => at != null && grid.scrollToIndex(at)}
              >
                {l}
              </button>
            );
          })}
        </nav>
      )}
    </div>
  );
}
