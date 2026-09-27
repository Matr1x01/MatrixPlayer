import { IconButton } from '@/components/ui/IconButton';
import { Artwork } from '@/components/ui/Artwork';
import { setFavorite, useIsFavorite } from '@/stores/library';
import { useNavigation, useView } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';

import { artistAlbum, formatBadge, formatKbps, isHiRes } from './format';
import { OutputButton } from './OutputButton';
import { SeekBar } from './SeekBar';
import { Transport } from './Transport';
import { VolumeControl } from './VolumeControl';

function CurrentTrack() {
  const track = usePlayer((s) => s.track);
  const navigate = useNavigation((s) => s.navigate);

  if (!track) {
    return (
      <div className="flex min-w-0 items-center gap-3">
        <Artwork artwork={null} size={56} />
        <div className="flex min-w-0 flex-col gap-[3px]">
          <span className="el text-[14px] font-semibold">Nothing playing</span>
          <span className="el t3 text-[12px]">Pick something from your library, or drop files on the window</span>
        </div>
      </div>
    );
  }

  const { metadata, properties, library } = track;
  const openNowPlaying = () => navigate('nowPlaying');
  const artist = metadata.artist ?? 'Unknown artist';
  const artistId = library?.artistId ?? null;
  const albumId = library?.albumId ?? null;
  return (
    <div className="flex min-w-0 items-center gap-3">
      <button type="button" aria-label="Open Now Playing" onClick={openNowPlaying} className="rounded-[4px]">
        <Artwork artwork={track.artwork} size={56} title={metadata.album ?? metadata.title} />
      </button>
      <div className="flex min-w-0 flex-col gap-[3px]">
        <button type="button" className="el text-left text-[14px] font-semibold" onClick={openNowPlaying}>
          {metadata.title}
        </button>
        <div className="el t2 text-[12px]" title={artistAlbum(track)}>
          {artistId != null ? (
            <button type="button" className="link" onClick={() => navigate('artists', artistId)}>
              {artist}
            </button>
          ) : (
            artist
          )}
          {metadata.album && (
            <>
              {' · '}
              {albumId != null ? (
                <button type="button" className="link" onClick={() => navigate('album', albumId)}>
                  {metadata.album}
                </button>
              ) : (
                metadata.album
              )}
            </>
          )}
        </div>
        <div className="mt-0.5 flex gap-[5px] overflow-hidden">
          <span className={isHiRes(properties) ? 'chip hr' : 'chip'}>{formatBadge(properties)}</span>
          {properties.bitrateKbps != null && <span className="chip">{formatKbps(properties.bitrateKbps)}</span>}
        </div>
      </div>
      <FavoriteButton />
    </div>
  );
}

/** The heart for the current track; only library tracks can be favorites. */
export function FavoriteButton({ size, iconSize = 17 }: { size?: number; iconSize?: number }) {
  const trackId = usePlayer((s) => s.track?.library?.trackId ?? null);
  const favorite = useIsFavorite(trackId);
  if (trackId == null) {
    return <IconButton icon="heart" label="Only tracks in your library can be favorites" iconSize={iconSize} size={size} disabled />;
  }
  return (
    <IconButton
      icon="heart"
      label={favorite ? 'Remove from favorites' : 'Add to favorites'}
      shortcut="L"
      iconSize={iconSize}
      size={size}
      active={favorite}
      filled={favorite}
      className="heart-btn"
      onClick={() => void setFavorite([trackId], !favorite)}
    />
  );
}

/** The 84 px player bar (Library — Albums board). */
export function PlayerBar() {
  const view = useView();
  const navigate = useNavigation((s) => s.navigate);
  const hasTrack = usePlayer((s) => s.track != null);

  return (
    <footer className="pb">
      <CurrentTrack />
      <div className="flex flex-col items-center gap-1.5">
        <Transport />
        <SeekBar />
      </div>
      <div className="flex items-center justify-end gap-1">
        <span className="pb-output">
          <OutputButton />
        </span>
        <IconButton icon="lyrics" label="Lyrics" iconSize={17} disabled={!hasTrack} onClick={() => navigate('nowPlaying', undefined, 'lyrics')} />
        <IconButton
          icon="queue"
          label="Queue"
          iconSize={17}
          open={view === 'queue'}
          active={view === 'queue'}
          onClick={() => navigate('queue')}
        />
        <IconButton
          icon="eq"
          label="Equalizer"
          iconSize={17}
          open={view === 'equalizer'}
          onClick={() => navigate('equalizer')}
        />
        <span className="pb-volume">
          <VolumeControl width={88} />
        </span>
      </div>
    </footer>
  );
}
