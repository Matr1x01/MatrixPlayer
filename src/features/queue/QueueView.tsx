import { memo, useMemo, useState, type KeyboardEvent } from 'react';

import { Icon } from '@/components/icons/Icon';
import { PageHeader } from '@/components/shell/PageHeader';
import { Artwork } from '@/components/ui/Artwork';
import { EmptyCard } from '@/components/ui/EmptyState';
import { EqIndicator } from '@/components/ui/EqIndicator';
import { IconButton } from '@/components/ui/IconButton';
import { Segmented } from '@/components/ui/Segmented';
import { openFiles, openFolder } from '@/features/library/openActions';
import { artistAlbum, formatBadge, formatSpan, formatTime, isHiRes } from '@/features/player/format';
import { toggleShuffle } from '@/features/player/hooks';
import { queueApi } from '@/services/ipc';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';
import type { QueueEntry } from '@/types/player';

type Tab = 'next' | 'history';

/** Rows only re-render when their entry or position changes, not on every
 * position tick, so long queues stay cheap. */
const QueueRow = memo(function QueueRow({ entry, number }: { entry: QueueEntry; number: number }) {
  const { metadata, properties } = entry.track;
  const play = () => void run(queueApi.playEntry(entry.id));
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') play();
    if (e.key === 'Delete') void run(queueApi.removeEntry(entry.id));
  };
  return (
    <div className="qr" role="button" tabIndex={0} onDoubleClick={play} onKeyDown={onKeyDown} aria-label={`Play ${metadata.title}`}>
      <span className="mono t3 text-right text-[11px]">{number}</span>
      <Artwork artwork={entry.track.artwork} size={36} radius={3} title={metadata.album ?? metadata.title} />
      <div className="flex min-w-0 flex-col gap-px">
        <span className="el font-medium" style={{ color: 'var(--tx)' }}>
          {metadata.title}
        </span>
        <span className="el t3 text-[12px]">{metadata.artist ?? 'Unknown artist'}</span>
      </div>
      <span className="el text-[12.5px]">{metadata.album ?? ''}</span>
      <span>
        <span className={isHiRes(properties) ? 'chip hr' : 'chip'}>{formatBadge(properties)}</span>
      </span>
      <span className="mono text-right text-[11.5px]">{formatTime(properties.durationMs || null)}</span>
      <IconButton
        icon="x"
        label="Remove from queue"
        shortcut="Del"
        size={28}
        iconSize={15}
        className="rm"
        onClick={() => void run(queueApi.removeEntry(entry.id))}
      />
    </div>
  );
});

function NowPlayingCard() {
  const track = usePlayer((s) => s.track);
  const status = usePlayer((s) => s.status);
  const positionMs = usePlayer((s) => s.positionMs);
  const durationMs = usePlayer((s) => s.durationMs);
  const navigate = useNavigation((s) => s.navigate);
  if (!track) return null;
  const progress = durationMs ? Math.min(100, (positionMs / durationMs) * 100) : 0;

  return (
    <section className="q-now" aria-label="Now playing">
      <button type="button" onClick={() => navigate('nowPlaying')} aria-label="Open Now Playing">
        <Artwork artwork={track.artwork} size={72} title={track.metadata.album ?? track.metadata.title} />
      </button>
      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex items-center gap-2">
          <EqIndicator paused={status !== 'playing'} />
          <span className="cap" style={{ color: 'var(--ac)' }}>
            {status === 'playing' ? 'Now playing' : status === 'paused' ? 'Paused' : 'Current track'}
          </span>
        </div>
        <div className="el text-[17px] font-bold">{track.metadata.title}</div>
        <div className="t2 el">{artistAlbum(track)}</div>
      </div>
      <div className="flex flex-col gap-[7px]">
        <div className="bar">
          <i style={{ width: `${progress}%` }} />
        </div>
        <div className="mono t3 flex justify-between text-[10.5px]">
          <span>{formatTime(positionMs)}</span>
          <span>{formatTime(durationMs)}</span>
        </div>
      </div>
      <div className="flex items-center gap-1.5">
        <span className={isHiRes(track.properties) ? 'chip hr' : 'chip'}>{formatBadge(track.properties)}</span>
      </div>
    </section>
  );
}

/** "12 tracks · 56 min left · ends ≈ 23:48". Isolated so that position
 * ticks re-render only this readout. */
function QueueMeta({ total, upcoming }: { total: number; upcoming: QueueEntry[] }) {
  const positionMs = usePlayer((s) => s.positionMs);
  const durationMs = usePlayer((s) => s.durationMs);
  const status = usePlayer((s) => s.status);
  if (total === 0) return <>Empty</>;
  const rest = upcoming.reduce((sum, e) => sum + e.track.properties.durationMs, 0);
  const remaining = Math.max(0, (durationMs ?? 0) - positionMs) + rest;
  const endsAt = status === 'playing' ? new Date(Date.now() + remaining) : null;
  const parts = [
    `${total} ${total === 1 ? 'track' : 'tracks'}`,
    `${formatSpan(remaining)} left`,
    endsAt && `ends ≈ ${endsAt.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })}`,
  ];
  return <>{parts.filter(Boolean).join(' · ')}</>;
}

export function QueueView() {
  const queue = usePlayer((s) => s.queue);
  const [tab, setTab] = useState<Tab>('next');
  const current = queue.currentIndex;
  const upcoming = useMemo(() => (current == null ? queue.entries : queue.entries.slice(current + 1)), [queue, current]);
  const history = useMemo(() => (current == null ? [] : queue.entries.slice(0, current).reverse()), [queue, current]);
  const total = queue.entries.length;

  const rows = tab === 'next' ? upcoming : history;
  const firstNumber = tab === 'next' ? (current ?? -1) + 2 : 1;

  return (
    <div className="view">
      <PageHeader
        section="Playback"
        title="Queue"
        meta={<QueueMeta total={total} upcoming={upcoming} />}
        actions={
          <>
            <Segmented<Tab>
              label="Queue section"
              value={tab}
              onChange={setTab}
              options={[
                { value: 'next', label: 'Up next' },
                {
                  value: 'history',
                  label: (
                    <>
                      History <span className="mono t3 text-[10px]">{history.length}</span>
                    </>
                  ),
                },
              ]}
            />
            <span className="vsep mx-1" style={{ height: 20 }} />
            <button type="button" className="btn" onClick={toggleShuffle} aria-pressed={queue.shuffle}>
              <Icon name="shuffle" size={15} style={{ color: queue.shuffle ? 'var(--ac)' : undefined }} />
              {queue.shuffle ? 'Shuffle on' : 'Shuffle'}
            </button>
            <button type="button" className="btn" disabled data-tip="Playlists arrive with the library">
              <Icon name="playlist" size={15} />
              Save as playlist
            </button>
            <button type="button" className="btn gh" disabled={total === 0} onClick={() => void run(queueApi.clear())}>
              <Icon name="trash" size={15} />
              Clear
            </button>
          </>
        }
      />

      {total === 0 ? (
        <div className="view-body flex items-start justify-center pt-16">
          <EmptyCard
            icon="queue"
            title="Queue is empty"
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
            Open a folder or some files, or drop them anywhere on the window.
          </EmptyCard>
        </div>
      ) : (
        <div className="view-body flex flex-col gap-[18px] px-7 pb-6">
          <NowPlayingCard />
          <div className="flex flex-col gap-0.5">
            <div className="flex items-center justify-between px-2.5 pb-1.5">
              <span className="cap">{tab === 'next' ? 'Up next' : 'Played in this session'}</span>
              <span className="mono t3 text-[10.5px]">
                {rows.length} {rows.length === 1 ? 'track' : 'tracks'} ·{' '}
                {formatTime(rows.reduce((sum, e) => sum + e.track.properties.durationMs, 0))}
              </span>
            </div>
            {rows.length === 0 ? (
              <div className="t3 px-2.5 py-3 text-[12.5px]">
                {tab === 'next'
                  ? queue.repeat === 'all'
                    ? 'Repeat all is on: the queue starts over after this track.'
                    : 'Nothing after this track.'
                  : 'Nothing has played before this track.'}
              </div>
            ) : (
              rows.map((entry, i) => (
                <QueueRow key={entry.id} entry={entry} number={tab === 'next' ? firstNumber + i : current! - i} />
              ))
            )}
          </div>
          {tab === 'next' && upcoming.length > 0 && (
            <p className="t3 px-2.5 text-[11.5px]">
              Double-click a track to play it. Drag-and-drop reordering arrives with playlists.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

