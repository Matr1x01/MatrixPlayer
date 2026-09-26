import { useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import { Artwork } from '@/components/ui/Artwork';
import { EmptyCard } from '@/components/ui/EmptyState';
import { IconButton } from '@/components/ui/IconButton';
import { openFiles, openFolder } from '@/features/library/openActions';
import { OutputButton } from '@/features/player/OutputButton';
import { SeekBar } from '@/features/player/SeekBar';
import { Transport } from '@/features/player/Transport';
import { VolumeControl } from '@/features/player/VolumeControl';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import type { TrackInfo } from '@/types/player';

import { SidePanel, type SideTab } from './SidePanel';
import { TrackChips } from './TrackChips';
import { Visualizer } from './Visualizer';

function context(track: TrackInfo, position: number | null, total: number): string {
  const { album, year, trackNumber, trackTotal } = track.metadata;
  const number = trackNumber
    ? `Track ${trackNumber}${trackTotal ? ` of ${trackTotal}` : ''}`
    : position != null && `Queue ${position + 1} of ${total}`;
  return [album, year, number].filter(Boolean).join(' · ');
}

function TrackHeader({ track, onTab }: { track: TrackInfo; onTab: (tab: SideTab) => void }) {
  const queue = usePlayer((s) => s.queue);
  const titleDerived = track.metadata.derivedFields.includes('title');
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-3.5">
      <div className="flex flex-col gap-1.5">
        <span className="cap el" style={{ color: 'var(--tx2)' }}>
          {context(track, queue.currentIndex, queue.entries.length)}
        </span>
        <h1
          className="np-title"
          style={titleDerived ? { fontStyle: 'italic' } : undefined}
          title={titleDerived ? 'No title tag: showing the file name' : undefined}
        >
          {track.metadata.title}
        </h1>
        <span className="el text-[18px] font-medium t2">{track.metadata.artist ?? 'Unknown artist'}</span>
      </div>
      <div className="-ml-1.5 flex items-center gap-1">
        <IconButton icon="heart" label="Favorites arrive with the library" disabled />
        <IconButton icon="playlist" label="Playlists arrive with the library" disabled />
        <IconButton icon="lyrics" label="Show lyrics" onClick={() => onTab('lyrics')} />
        <IconButton icon="info" label="File information" onClick={() => onTab('file')} />
      </div>
      <TrackChips track={track} />
      <Visualizer track={track} />
    </div>
  );
}

/** Full-window Now Playing (Now Playing board). */
export function NowPlayingView() {
  const track = usePlayer((s) => s.track);
  const navigate = useNavigation((s) => s.navigate);
  const [tab, setTab] = useState<SideTab>('next');
  // Below 1100 px the side panel is a drawer (Responsive board).
  const [drawerOpen, setDrawerOpen] = useState(false);
  const showTab = (next: SideTab) => {
    setTab(next);
    setDrawerOpen(true);
  };

  if (!track) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <EmptyCard
          icon="nowPlaying"
          title="Nothing is playing"
          className="w-[340px]"
          actions={
            <>
              <button type="button" className="btn pri" onClick={() => void openFolder()}>
                <Icon name="folder" size={15} />
                Open music folder…
              </button>
              <button type="button" className="btn" style={{ fontWeight: 500 }} onClick={() => void openFiles()}>
                Open files…
              </button>
            </>
          }
        >
          Open some music and it will show up here with its artwork, format and signal path.
        </EmptyCard>
      </div>
    );
  }

  return (
    <div className="np">
      <main className="np-main">
        <div className="np-top">
          <Artwork artwork={track.artwork} title={track.metadata.album ?? track.metadata.title} className="np-art" />
          <TrackHeader track={track} onTab={showTab} />
        </div>

        <SeekBar layout="split" />

        <div className="np-controls">
          <VolumeControl width={150} showDb iconSize={18} />
          <Transport scale="large" />
          <div className="flex items-center justify-end gap-1.5">
            <button type="button" className="btn glass" onClick={() => navigate('equalizer')} data-tip="Equalizer">
              <Icon name="eq" size={15} style={{ color: 'var(--tx3)' }} />
              <span className="btn-label">Equalizer off</span>
            </button>
            <OutputButton framed />
          </div>
        </div>
      </main>
      <IconButton
        icon="side"
        label={drawerOpen ? 'Hide queue and details' : 'Show queue and details'}
        className="np-drawer-toggle"
        open={drawerOpen}
        onClick={() => setDrawerOpen(!drawerOpen)}
      />
      <SidePanel tab={tab} onTab={setTab} open={drawerOpen} />
    </div>
  );
}
