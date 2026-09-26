import type { CSSProperties } from 'react';

import { Icon } from '@/components/icons/Icon';
import { Artwork } from '@/components/ui/Artwork';
import { contextMenu } from '@/components/ui/ContextMenu';
import { EmptyCard } from '@/components/ui/EmptyState';
import { IconButton } from '@/components/ui/IconButton';
import { channelLabel, formatDb, formatTime } from '@/features/player/format';
import { useLibraryQuery } from '@/hooks/useLibraryQuery';
import { libraryApi } from '@/services/ipc';
import { setFavorite, useLibrary } from '@/stores/library';
import { navigate } from '@/stores/navigation';
import type { AlbumDetail } from '@/types/library';
import type { Rgb } from '@/types/player';

import { playScope, playTracks } from './actions';
import { albumMenu } from './albums';
import { albumBadge, badgeClass, formatSize, isHiResAlbum, plural } from './format';
import { ALBUM_COLUMNS, TrackTable } from './TrackTable';

const rgba = ([r, g, b]: Rgb, a: number) => `rgba(${r}, ${g}, ${b}, ${a})`;

/** The hero glow, drawn from the cover's own colours like Now Playing's backdrop. */
function heroBackground(detail: AlbumDetail): CSSProperties {
  const palette = detail.artwork?.palette;
  if (!palette) return {};
  return {
    background: `radial-gradient(70% 140% at 12% 0%, ${rgba(palette[0], 0.24)}, transparent 60%), radial-gradient(50% 120% at 60% 0%, ${rgba(palette[1], 0.2)}, transparent 70%)`,
  };
}

export function AlbumView({ id }: { id: number }) {
  const { data, error } = useLibraryQuery(`album:${id}`, () => libraryApi.album(id), ['tracks', 'history']);
  const favorites = useLibrary((s) => s.favorites);

  if (error) {
    return (
      <div className="view items-center justify-center">
        <EmptyCard icon="album" title="This album is no longer in your library" className="w-[340px]" actions={
          <button type="button" className="btn" onClick={() => navigate('albums')}>Back to albums</button>
        }>
          It may have been removed or re-tagged during a rescan.
        </EmptyCard>
      </div>
    );
  }
  if (!data) return <div className="view"><div className="album-hero sk-hero" /></div>;

  const { album, tracks } = data;
  const ids = tracks.map((t) => t.id);
  const allFavorite = ids.length > 0 && ids.every((t) => favorites.has(t));
  const badge = albumBadge(album);
  const hiRes = isHiResAlbum(album);
  const kind = album.trackCount <= 3 ? 'Single' : album.trackCount <= 6 && album.durationMs < 30 * 60_000 ? 'EP' : 'Album';

  return (
    <div className="view">
      <div className="view-body">
        <section className="album-hero" style={heroBackground(data)}>
          <Artwork artwork={data.artwork} artworkId={album.artworkId} full size={224} radius={5} title={album.title} className="hero-art" />
          <div className="flex min-w-0 flex-1 flex-col gap-2.5">
            <div className="cap el">
              {[kind, album.year, data.genres.join(', ')].filter(Boolean).join(' · ')}
            </div>
            <h1 className="hero-title">{album.title}</h1>
            <button
              type="button"
              className="hero-artist text-left"
              disabled={album.artistId == null}
              onClick={() => album.artistId != null && navigate('artists', album.artistId)}
            >
              {album.artistName}
            </button>
            <div className="mono t3 flex flex-wrap gap-3.5 text-[11.5px]">
              <span>{plural(album.trackCount, 'track')}</span>
              {data.discCount > 1 && <span>{data.discCount} discs</span>}
              <span>{formatTime(album.durationMs)}</span>
              <span>{formatSize(album.totalSize)}</span>
              {album.playCount > 0 && <span>played {album.playCount}×</span>}
            </div>
            <div className="mt-1.5 flex items-center gap-2">
              <button type="button" className="btn pri h36" onClick={() => void playScope({ kind: 'album', id })}>
                <Icon name="play" size={16} filled />
                Play
              </button>
              <button type="button" className="btn h36" onClick={() => void playScope({ kind: 'album', id }, 'replace', true)}>
                <Icon name="shuffle" size={16} />
                Shuffle
              </button>
              <button type="button" className="btn h36" onClick={() => void playTracks(ids, 'append')}>
                <Icon name="playNext" size={16} />
                Add to queue
              </button>
              <IconButton
                icon="heart"
                label={allFavorite ? 'Remove album from favorites' : 'Add album to favorites'}
                active={allFavorite}
                filled={allFavorite}
                size={36}
                onClick={() => void setFavorite(ids, !allFavorite)}
              />
              <IconButton
                icon="more"
                label="More album actions"
                size={36}
                strokeWidth={2.8}
                onClick={(e) => contextMenu(() => albumMenu(album), { label: 'Album actions' })(e)}
              />
            </div>
          </div>
          <aside className="hero-info">
            <div className="flex flex-wrap gap-1.5">
              {badge && <span className={badgeClass(album)}>{badge.split(' ')[0]}</span>}
              {album.lossless && album.bitDepth && album.sampleRate && (
                <span className={hiRes ? 'chip hr' : 'chip'}>{badge?.split(' ')[1]}</span>
              )}
              {data.channels && <span className="chip">{channelLabel(data.channels).toUpperCase()}</span>}
            </div>
            <div className="mono grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[10.5px]">
              <span className="t3">Album gain</span>
              <span>{data.replayGain.albumGain != null ? formatDb(data.replayGain.albumGain) : 'not tagged'}</span>
              <span className="t3">Album peak</span>
              <span>{data.replayGain.albumPeak != null ? data.replayGain.albumPeak.toFixed(3) : '—'}</span>
              <span className="t3">Artwork</span>
              <span>{data.artwork ? `${data.artwork.width} × ${data.artwork.height}` : 'none'}</span>
              {data.folderId != null && (
                <>
                  <span className="t3">Folder</span>
                  <button type="button" className="el text-left hover:underline" onClick={() => navigate('folders', data.folderId!)}>
                    {data.folderPath?.split('/').pop()}
                  </button>
                </>
              )}
            </div>
          </aside>
        </section>

        <div className="album-tracks">
          <TrackTable ids={ids} columns={ALBUM_COLUMNS} rowHeight={34} autoHeight />
        </div>

        {data.moreByArtist.length > 0 && (
          <div className="more-by">
            <span className="cap whitespace-nowrap">More by {album.artistName}</span>
            {data.moreByArtist.slice(0, 5).map((a) => (
              <button key={a.id} type="button" className="more-chip" onClick={() => navigate('album', a.id)}>
                <Artwork artworkId={a.artworkId} size={36} radius={3} title={a.title} />
                <span className="flex min-w-0 flex-col text-left">
                  <span className="el text-[12.5px] font-semibold">{a.title}</span>
                  <span className="mono t3 text-[10px]">{[a.year, plural(a.trackCount, 'track')].filter(Boolean).join(' · ')}</span>
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
