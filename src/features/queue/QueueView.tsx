import { memo, useCallback, useEffect, useMemo, useState, type KeyboardEvent } from 'react';

import { Icon } from '@/components/icons/Icon';
import { PageHeader } from '@/components/shell/PageHeader';
import { Artwork } from '@/components/ui/Artwork';
import { EmptyCard } from '@/components/ui/EmptyState';
import { EqIndicator } from '@/components/ui/EqIndicator';
import { IconButton } from '@/components/ui/IconButton';
import { contextMenu, type MenuItem } from '@/components/ui/ContextMenu';
import { askText } from '@/components/ui/Dialog';
import { Segmented } from '@/components/ui/Segmented';
import { playScope } from '@/features/library/actions';
import { playlistSubmenu } from '@/features/library/trackMenu';
import { openFiles, openFolder } from '@/features/library/openActions';
import { beforeId, useDragReorder } from '@/hooks/useDragReorder';
import { artistAlbum, formatBadge, formatSpan, formatTime, isHiRes } from '@/features/player/format';
import { toggleShuffle } from '@/features/player/hooks';
import { playlistApi, queueApi } from '@/services/ipc';
import { useLibrary } from '@/stores/library';
import { navigate, useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { run, showToast } from '@/stores/toasts';
import type { QueueEntry } from '@/types/player';

type Tab = 'next' | 'history';

/** Rows only re-render when their entry or position changes, not on every
 * position tick, so long queues stay cheap. */
interface QueueRowProps {
  entry: QueueEntry;
  number: number;
  /** Pointer handlers for the drag grip; omitted where rows cannot move. */
  grip?: ReturnType<ReturnType<typeof useDragReorder>['grip']>;
  ghost?: boolean;
  /** Moves the row one step with Alt+↑ / Alt+↓. */
  onNudge?: (delta: number) => void;
  index: number;
}

const QueueRow = memo(function QueueRow({ entry, number, grip, ghost, onNudge, index }: QueueRowProps) {
  const { metadata, properties } = entry.track;
  const play = () => void run(queueApi.playEntry(entry.id));
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') play();
    if (e.key === 'Delete') void run(queueApi.removeEntry(entry.id));
    if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown') && onNudge) {
      e.preventDefault();
      onNudge(e.key === 'ArrowUp' ? -1 : 1);
    }
  };
  const menu = contextMenu(() => queueMenu(entry), { label: 'Queue entry actions' });
  return (
    <div
      className={['qr', ghost && 'ghost', entry.failed && 'failed'].filter(Boolean).join(' ')}
      role="button"
      tabIndex={0}
      data-reorder
      style={{ ['--i' as string]: Math.min(index, 16) }}
      onDoubleClick={play}
      onKeyDown={onKeyDown}
      onContextMenu={menu}
      aria-label={`Play ${metadata.title}`}
    >
      {grip ? (
        <span className="gp" aria-label="Drag to reorder" data-tip="Drag to reorder · Alt ↑↓" {...grip}>
          <Icon name="grip" size={16} strokeWidth={2.6} />
        </span>
      ) : (
        <span />
      )}
      <span className="mono t3 text-right text-[11px]">{number}</span>
      <Artwork artwork={entry.track.artwork} size={36} radius={3} title={metadata.album ?? metadata.title} />
      <div className="flex min-w-0 flex-col gap-px">
        <span className="flex min-w-0 items-center gap-1.5">
          {entry.failed && (
            <span className="flex-none" style={{ color: 'var(--warn)' }} data-tip="Could not be played the last time it was tried">
              <Icon name="warn" size={13} />
            </span>
          )}
          <span className="el font-medium" style={{ color: 'var(--tx)' }}>
            {metadata.title}
          </span>
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

function queueMenu(entry: QueueEntry): MenuItem[] {
  const lib = entry.track.library;
  return [
    { label: 'Play', icon: 'play', filledIcon: true, shortcut: 'Enter', onSelect: () => void run(queueApi.playEntry(entry.id)) },
    {
      label: 'Play next',
      icon: 'playNext',
      onSelect: () => {
        const { entries, currentIndex } = usePlayer.getState().queue;
        const after = currentIndex != null ? entries[currentIndex + 1] : undefined;
        if (after && after.id !== entry.id) void run(queueApi.move([entry.id], after.id));
      },
    },
    { label: 'Move to end', icon: 'queue', onSelect: () => void run(queueApi.move([entry.id], null)) },
    'separator',
    { label: 'Add to playlist', icon: 'playlist', disabled: !lib, submenu: lib ? playlistSubmenu([lib.trackId]) : [] },
    'separator',
    { label: 'Open album', icon: 'album', disabled: lib?.albumId == null, onSelect: () => lib?.albumId != null && navigate('album', lib.albumId) },
    { label: 'Browse folder', icon: 'folder', disabled: !lib, onSelect: () => lib && navigate('folders', lib.folderId) },
    'separator',
    { label: 'Remove from queue', icon: 'x', shortcut: 'Del', onSelect: () => void run(queueApi.removeEntry(entry.id)) },
  ];
}

/** Saves the queue, in play order, as a new playlist (library tracks only). */
async function saveQueueAsPlaylist(entries: QueueEntry[]) {
  const ids = entries.map((e) => e.track.library?.trackId).filter((id): id is number => id != null);
  if (ids.length === 0) {
    showToast({ tone: 'error', message: 'Only tracks in your library can go into a playlist' });
    return;
  }
  const first = entries[0]?.track.metadata;
  const name = await askText({
    title: 'Save queue as playlist',
    label: 'Name',
    initial: first?.album ? `${first.album} and more` : 'Queue',
    confirm: 'Save',
  });
  if (!name) return;
  const id = await run(playlistApi.create(name, ids));
  if (id == null) return;
  const skipped = entries.length - ids.length;
  showToast({
    tone: 'info',
    message: `Saved ${ids.length} tracks as “${name}”`,
    detail: skipped ? `${skipped} files outside the library were left out` : undefined,
  });
}

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

/** "Continuing album · X" when the started part of the queue is one album. */
function contextTitle(entries: QueueEntry[]): string {
  const albums = new Set(entries.map((e) => e.track.metadata.album ?? ''));
  const album = entries[0]?.track.metadata.album;
  return albums.size === 1 && album ? `Continuing album · ${album}` : 'Up next';
}

function EmptyQueue() {
  const [saved, setSaved] = useState<{ tracks: number } | null>(null);
  const hasLibrary = useLibrary((s) => (s.stats?.tracks ?? 0) > 0);
  useEffect(() => {
    void queueApi
      .savedSession()
      .then(setSaved)
      .catch(() => setSaved(null));
  }, []);
  return (
    <div className="view-body flex items-start justify-center pt-16">
      <EmptyCard
        icon="queue"
        title="Queue is empty"
        className="w-[340px]"
        actions={
          <>
            {hasLibrary ? (
              <button type="button" className="btn pri" onClick={() => void playScope({ kind: 'all' }, 'replace', true)}>
                <Icon name="shuffle" size={15} />
                Shuffle library
              </button>
            ) : (
              <button type="button" className="btn pri" onClick={() => void openFolder()}>
                <Icon name="folder" size={15} />
                Open music folder…
              </button>
            )}
            {saved ? (
              <button type="button" className="btn" style={{ fontWeight: 500 }} onClick={() => void run(queueApi.restoreSession())}>
                Restore last session · {saved.tracks} {saved.tracks === 1 ? 'track' : 'tracks'}
              </button>
            ) : (
              <button type="button" className="btn" style={{ fontWeight: 500 }} onClick={() => void openFiles()}>
                Open files…
              </button>
            )}
          </>
        }
      >
        Play an album, or drag files here from anywhere on your computer.
      </EmptyCard>
    </div>
  );
}

export function QueueView() {
  const queue = usePlayer((s) => s.queue);
  const [tab, setTab] = useState<Tab>('next');
  const current = queue.currentIndex;
  const upcoming = useMemo(() => (current == null ? queue.entries : queue.entries.slice(current + 1)), [queue, current]);
  const history = useMemo(() => (current == null ? [] : queue.entries.slice(0, current).reverse()), [queue, current]);
  const total = queue.entries.length;

  // Optimistic order while a move is on its way to the core.
  const [pending, setPending] = useState<QueueEntry[] | null>(null);
  useEffect(() => setPending(null), [queue]);
  const list = tab === 'next' ? (pending ?? upcoming) : history;

  const move = useCallback(
    (from: number, gap: number) => {
      const entry = upcoming[from];
      if (!entry) return;
      const before = beforeId(upcoming, gap);
      const next = [...upcoming];
      next.splice(from, 1);
      next.splice(gap > from ? gap - 1 : gap, 0, entry);
      setPending(next);
      void run(queueApi.move([entry.id], before));
    },
    [upcoming],
  );
  const { container, drag, grip } = useDragReorder(move);
  const nudge = (index: number, delta: number) => {
    const gap = delta < 0 ? index - 1 : index + 2;
    if (gap >= 0 && gap <= upcoming.length) move(index, gap);
  };

  // Upcoming entries split as on the Queue board: what was started, then
  // what was added by hand.
  const groups = useMemo(() => {
    if (tab !== 'next') return [{ title: 'Played in this session', start: 0, entries: list }];
    const firstAdded = list.findIndex((e) => e.added);
    if (firstAdded <= 0 || list.slice(firstAdded).some((e) => !e.added)) {
      return [{ title: firstAdded === 0 ? 'Added by you' : contextTitle(list), start: 0, entries: list }];
    }
    return [
      { title: contextTitle(list.slice(0, firstAdded)), start: 0, entries: list.slice(0, firstAdded) },
      { title: 'Added by you', start: firstAdded, entries: list.slice(firstAdded) },
    ];
  }, [list, tab]);

  const firstNumber = tab === 'next' ? (current ?? -1) + 2 : 1;
  const dragged = drag ? list[drag.from] : undefined;

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
            <button type="button" className="btn" disabled={total === 0} onClick={() => void saveQueueAsPlaylist(queue.entries)}>
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
        <EmptyQueue />
      ) : (
        <div className="view-body flex flex-col gap-[18px] px-7 pb-6">
          <NowPlayingCard />
          <div ref={container} className="relative flex flex-col gap-[18px]">
            {groups.map((group) => (
              <div key={group.title} className="flex flex-col gap-0.5">
                <div className="flex items-center justify-between px-2.5 pb-1.5">
                  <span className="cap el">{group.title}</span>
                  <span className="mono t3 flex-none text-[10.5px]">
                    {group.entries.length} {group.entries.length === 1 ? 'track' : 'tracks'} ·{' '}
                    {formatTime(group.entries.reduce((sum, e) => sum + e.track.properties.durationMs, 0))}
                  </span>
                </div>
                {group.entries.map((entry, i) => {
                  const index = group.start + i;
                  return (
                    <QueueRow
                      key={entry.id}
                      entry={entry}
                      index={index}
                      number={tab === 'next' ? firstNumber + index : current! - index}
                      grip={tab === 'next' ? grip(index) : undefined}
                      ghost={drag?.from === index}
                      onNudge={tab === 'next' ? (delta) => nudge(index, delta) : undefined}
                    />
                  );
                })}
              </div>
            ))}
            {list.length === 0 && (
              <div className="t3 px-2.5 py-3 text-[12.5px]">
                {tab === 'next'
                  ? queue.repeat === 'all'
                    ? 'Repeat all is on: the queue starts over after this track.'
                    : 'Nothing after this track.'
                  : 'Nothing has played before this track.'}
              </div>
            )}
            {drag && dragged && (
              <>
                {drag.gap !== drag.from && drag.gap !== drag.from + 1 && <div className="drop-line" style={{ top: drag.lineY }} />}
                <div className="qr drag" style={{ position: 'absolute', left: 0, right: 0, top: drag.top + drag.dy }} aria-hidden="true">
                  <span className="gp">
                    <Icon name="grip" size={16} strokeWidth={2.6} />
                  </span>
                  <span />
                  <Artwork artwork={dragged.track.artwork} size={36} radius={3} title={dragged.track.metadata.album ?? dragged.track.metadata.title} />
                  <div className="flex min-w-0 flex-col gap-px">
                    <span className="el font-medium" style={{ color: 'var(--tx)' }}>
                      {dragged.track.metadata.title}
                    </span>
                    <span className="el t3 text-[12px]">{dragged.track.metadata.artist ?? 'Unknown artist'}</span>
                  </div>
                  <span className="el text-[12.5px]">{dragged.track.metadata.album ?? ''}</span>
                  <span>
                    <span className={isHiRes(dragged.track.properties) ? 'chip hr' : 'chip'}>{formatBadge(dragged.track.properties)}</span>
                  </span>
                  <span className="mono text-right text-[11.5px]">{formatTime(dragged.track.properties.durationMs || null)}</span>
                  <span />
                </div>
              </>
            )}
          </div>
          {tab === 'next' && upcoming.length > 1 && (
            <p className="t3 px-2.5 text-[11.5px]">Double-click to play · drag the grip or press Alt ↑↓ to reorder · Delete removes.</p>
          )}
        </div>
      )}
    </div>
  );
}
