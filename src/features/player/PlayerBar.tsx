import { IconButton } from '@/components/ui/IconButton';
import { Artwork } from '@/components/ui/Artwork';
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
          <span className="el t3 text-[12px]">Open files with Ctrl+O, or drop them on the window</span>
        </div>
      </div>
    );
  }

  const { metadata, properties } = track;
  const openNowPlaying = () => navigate('nowPlaying');
  return (
    <div className="flex min-w-0 items-center gap-3">
      <button type="button" aria-label="Open Now Playing" onClick={openNowPlaying} className="rounded-[4px]">
        <Artwork artwork={track.artwork} size={56} title={metadata.album ?? metadata.title} />
      </button>
      <div className="flex min-w-0 flex-col gap-[3px]">
        <button type="button" className="el text-left text-[14px] font-semibold" onClick={openNowPlaying}>
          {metadata.title}
        </button>
        <div className="el t2 text-[12px]">{artistAlbum(track)}</div>
        <div className="mt-0.5 flex gap-[5px] overflow-hidden">
          <span className={isHiRes(properties) ? 'chip hr' : 'chip'}>{formatBadge(properties)}</span>
          {properties.bitrateKbps != null && <span className="chip">{formatKbps(properties.bitrateKbps)}</span>}
        </div>
      </div>
      <IconButton icon="heart" label="Favorites arrive with the library" iconSize={17} disabled />
    </div>
  );
}

/** The 84 px player bar (Library — Albums board). */
export function PlayerBar() {
  const view = useView();
  const navigate = useNavigation((s) => s.navigate);

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
        <IconButton icon="lyrics" label="Lyrics (not available yet)" iconSize={17} disabled />
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
