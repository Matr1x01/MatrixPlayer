import { useMemo } from 'react';

import { Icon } from '@/components/icons/Icon';
import { PageHeader } from '@/components/shell/PageHeader';
import { Artwork } from '@/components/ui/Artwork';
import { askConfirm, askText } from '@/components/ui/Dialog';
import { contextMenu, type MenuItem } from '@/components/ui/ContextMenu';
import { EmptyCard } from '@/components/ui/EmptyState';
import { EqIndicator } from '@/components/ui/EqIndicator';
import { IconButton } from '@/components/ui/IconButton';
import { formatTime } from '@/features/player/format';
import { useLibraryQuery } from '@/hooks/useLibraryQuery';
import { libraryApi, playlistApi } from '@/services/ipc';
import { useLibrary } from '@/stores/library';
import { navigate } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { run, showToast } from '@/stores/toasts';
import type { HistoryEntry, PlaylistRow } from '@/types/library';

import { playScope, playTracks } from './actions';
import { AlbumTile, usePlayingAlbum } from './albums';
import { badgeClass, count, formatDate, formatLength, plural, trackBadge } from './format';
import { LibraryWelcome } from './LibraryWelcome';
import { ScanBanner } from './ScanBanner';
import { addToNewPlaylist, trackMenu } from './trackMenu';
import { LIST_COLUMNS, SONG_COLUMNS, TrackTable } from './TrackTable';

// ── Favorites ──────────────────────────────────────────────────────────────

export function FavoritesView() {
  const { data } = useLibraryQuery('favorites', () => libraryApi.trackList({ scope: { kind: 'favorites' } }), ['tracks', 'favorites']);
  const n = data?.ids.length ?? 0;
  return (
    <div className="view">
      <PageHeader
        section="Collections"
        title="Favorites"
        meta={data && n > 0 && `${plural(n, 'track')} · ${formatLength(data.durationMs)}`}
        actions={
          n > 0 && (
            <>
              <button type="button" className="btn pri" onClick={() => void playScope({ kind: 'favorites' })}>
                <Icon name="play" size={15} filled />
                Play
              </button>
              <button type="button" className="btn" onClick={() => void playScope({ kind: 'favorites' }, 'replace', true)}>
                <Icon name="shuffle" size={15} />
                Shuffle
              </button>
            </>
          )
        }
      />
      <div className="flex min-h-0 flex-1 flex-col px-[18px]">
        <TrackTable
          ids={data?.ids ?? []}
          columns={SONG_COLUMNS.filter((c) => c !== 'genre' && c !== 'bits')}
          footer="Newest favorites first"
          empty={
            data && (
              <EmptyCard icon="heart" title="No favorites yet" className="w-[320px]">
                Press <span className="kbd">L</span> while a track plays, or use the heart in the player bar.
              </EmptyCard>
            )
          }
        />
      </div>
    </div>
  );
}

// ── Recently played ────────────────────────────────────────────────────────

function dayLabel(seconds: number) {
  return formatDate(seconds);
}

function HistoryRow({ entry, index, all, playing, paused }: { entry: HistoryEntry; index: number; all: number[]; playing: boolean; paused: boolean }) {
  const t = entry.track;
  const time = new Date(entry.playedAt * 1000).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  return (
    <div
      className={playing ? 'hist-row now' : 'hist-row'}
      role="button"
      tabIndex={0}
      style={{ ['--i' as string]: Math.min(index, 20) }}
      onDoubleClick={() => void playTracks(all, 'replace', index)}
      onKeyDown={(e) => e.key === 'Enter' && void playTracks(all, 'replace', index)}
      onContextMenu={contextMenu(() => trackMenu([t.id], [t]).items, { label: 'Track actions' })}
    >
      <span className="mono t3 text-[11px]">{playing ? <EqIndicator paused={paused} /> : time}</span>
      <Artwork artworkId={t.artworkId} size={32} radius={3} title={t.album ?? t.title} />
      <span className="flex min-w-0 flex-col">
        <span className="tt el">{t.title}</span>
        <span className="el t3 text-[11.5px]">{t.artist ?? 'Unknown artist'}</span>
      </span>
      <span className="el t2 text-[12.5px]">{t.album}</span>
      <span>
        <span className={badgeClass(t)}>{trackBadge(t)}</span>
      </span>
      <span className="mono text-right text-[11.5px]">{formatTime(t.durationMs)}</span>
    </div>
  );
}

export function RecentView() {
  const { data: history } = useLibraryQuery('history', () => libraryApi.history(500), ['tracks', 'history']);
  const playing = usePlayer((s) => s.track?.library?.trackId ?? null);
  const status = usePlayer((s) => s.status);
  const paused = status !== 'playing';
  const active = status === 'playing' || status === 'paused' || status === 'loading';
  const all = useMemo(() => (history ?? []).map((h) => h.track.id), [history]);
  const groups = useMemo(() => {
    const out: { label: string; entries: { entry: HistoryEntry; index: number }[] }[] = [];
    (history ?? []).forEach((entry, index) => {
      const label = dayLabel(entry.playedAt);
      if (out.at(-1)?.label !== label) out.push({ label, entries: [] });
      out.at(-1)!.entries.push({ entry, index });
    });
    return out;
  }, [history]);

  const clear = async () => {
    const ok = await askConfirm({
      title: 'Clear listening history?',
      body: 'Play counts and the Recently Played list are reset. Favorites and playlists are kept.',
      confirm: 'Clear history',
      danger: true,
    });
    if (ok) await run(libraryApi.clearHistory());
  };

  return (
    <div className="view">
      <PageHeader
        section="Collections"
        title="Recently Played"
        meta={history && history.length > 0 && `${plural(history.length, 'play')} · a play counts after half a track`}
        actions={
          history &&
          history.length > 0 && (
            <>
              <button type="button" className="btn" onClick={() => void playTracks([...new Set(all)])}>
                <Icon name="play" size={15} filled />
                Play again
              </button>
              <button type="button" className="btn gh" onClick={() => void clear()}>
                Clear history
              </button>
            </>
          )
        }
      />
      <div className="view-body border-t border-ln px-[18px] pb-6">
        {history && history.length === 0 && (
          <div className="flex justify-center pt-16">
            <EmptyCard icon="recent" title="Nothing played yet" className="w-[320px]">
              Tracks from your library appear here once you have listened to at least half of them.
            </EmptyCard>
          </div>
        )}
        {groups.map((g) => (
          <div key={g.label} className="flex flex-col">
            <div className="cap px-2.5 pb-1.5 pt-4">{g.label}</div>
            {g.entries.map(({ entry, index }) => (
              <HistoryRow key={`${entry.playedAt}:${index}`} entry={entry} index={index} all={all} playing={active && index === 0 && entry.track.id === playing} paused={paused} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

// ── Playlists ──────────────────────────────────────────────────────────────

function playlistMenu(p: PlaylistRow): MenuItem[] {
  return [
    { label: 'Play', icon: 'play', filledIcon: true, disabled: !p.trackCount, onSelect: () => void playScope({ kind: 'playlist', id: p.id }) },
    { label: 'Shuffle', icon: 'shuffle', disabled: !p.trackCount, onSelect: () => void playScope({ kind: 'playlist', id: p.id }, 'replace', true) },
    { label: 'Add to queue', icon: 'queue', disabled: !p.trackCount, onSelect: () => void playScope({ kind: 'playlist', id: p.id }, 'append', undefined, p.name) },
    'separator',
    { label: p.pinned ? 'Unpin from sidebar' : 'Pin to sidebar', icon: 'side', onSelect: () => void run(playlistApi.setPinned(p.id, !p.pinned)) },
    { label: 'Rename…', icon: 'edit', onSelect: () => void renamePlaylist(p) },
    'separator',
    { label: 'Delete playlist…', icon: 'trash', danger: true, onSelect: () => void deletePlaylist(p) },
  ];
}

async function renamePlaylist(p: PlaylistRow) {
  const name = await askText({ title: 'Rename playlist', label: 'Name', initial: p.name, confirm: 'Rename' });
  if (name && name !== p.name) await run(playlistApi.rename(p.id, name));
}

async function deletePlaylist(p: PlaylistRow, after?: () => void) {
  const ok = await askConfirm({
    title: `Delete “${p.name}”?`,
    body: 'The playlist is removed. The tracks in it stay in your library.',
    confirm: 'Delete playlist',
    danger: true,
  });
  if (!ok) return;
  if ((await run(playlistApi.remove(p.id).then(() => true))) === true) {
    showToast({ tone: 'info', message: `Deleted “${p.name}”` });
    after?.();
  }
}

export function PlaylistsView() {
  const playlists = useLibrary((s) => s.playlists);
  return (
    <div className="view">
      <PageHeader
        section="Collections"
        title="Playlists"
        meta={playlists.length > 0 && plural(playlists.length, 'playlist')}
        actions={
          <button type="button" className="btn pri" onClick={() => void addToNewPlaylist([])}>
            <Icon name="plus" size={15} />
            New playlist
          </button>
        }
      />
      <div className="view-body border-t border-ln">
        {playlists.length === 0 ? (
          <div className="flex justify-center pt-16">
            <EmptyCard
              icon="playlist"
              title="No playlists yet"
              className="w-[340px]"
              actions={
                <button type="button" className="btn pri" onClick={() => void addToNewPlaylist([])}>
                  New playlist…
                </button>
              }
            >
              Create one here, or right-click any tracks and choose Add to playlist.
            </EmptyCard>
          </div>
        ) : (
          <div className="genre-grid">
            {playlists.map((p, i) => (
              <button
                key={p.id}
                type="button"
                className="genre-tile"
                style={{ ['--i' as string]: Math.min(i, 24) }}
                onClick={() => navigate('playlist', p.id)}
                onContextMenu={contextMenu(() => playlistMenu(p), { label: `${p.name} actions` })}
              >
                <span className="genre-art">
                  <Artwork artworkId={p.artworkId} title={p.name} />
                  {p.pinned && (
                    <span className="pin-badge" aria-label="Pinned">
                      <Icon name="side" size={12} />
                    </span>
                  )}
                </span>
                <span className="el text-left font-semibold">{p.name}</span>
                <span className="mono t3 text-left text-[10.5px]">
                  {plural(p.trackCount, 'track')}
                  {p.durationMs > 0 && ` · ${formatLength(p.durationMs)}`}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export function PlaylistView({ id }: { id: number }) {
  const playlist = useLibrary((s) => s.playlists.find((p) => p.id === id));
  const { data } = useLibraryQuery(`playlist:${id}`, () => libraryApi.trackList({ scope: { kind: 'playlist', id } }), ['tracks', 'playlists']);
  if (!playlist) {
    return (
      <div className="view items-center justify-center">
        <EmptyCard icon="playlist" title="Playlist not found" actions={<button type="button" className="btn" onClick={() => navigate('playlists')}>All playlists</button>}>
          It may have been deleted.
        </EmptyCard>
      </div>
    );
  }
  const ids = data?.ids ?? [];
  const remove = (positions: number[]) => void run(playlistApi.removeEntries(id, positions));
  return (
    <div className="view">
      <section className="genre-hero">
        <Artwork artworkId={playlist.artworkId} size={132} radius={5} title={playlist.name} />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <span className="cap">Playlist{playlist.pinned ? ' · pinned' : ''}</span>
          <h1 className="hero-title el">{playlist.name}</h1>
          <span className="mono t3 text-[11.5px]">
            {plural(playlist.trackCount, 'track')} · {formatLength(playlist.durationMs)} · updated {formatDate(playlist.updatedAt)}
          </span>
          <div className="mt-1 flex items-center gap-2">
            <button type="button" className="btn pri h36" disabled={!ids.length} onClick={() => void playScope({ kind: 'playlist', id })}>
              <Icon name="play" size={16} filled />
              Play
            </button>
            <button type="button" className="btn h36" disabled={!ids.length} onClick={() => void playScope({ kind: 'playlist', id }, 'replace', true)}>
              <Icon name="shuffle" size={16} />
              Shuffle
            </button>
            <IconButton icon="more" label="Playlist actions" size={36} strokeWidth={2.8} onClick={(e) => contextMenu(() => playlistMenu(playlist), { label: 'Playlist actions' })(e)} />
          </div>
        </div>
      </section>
      <div className="flex min-h-0 flex-1 flex-col px-[18px]">
        <TrackTable
          ids={ids}
          columns={LIST_COLUMNS}
          rowHeight={44}
          menuExtra={(positions) => [{ label: 'Remove from playlist', icon: 'x', shortcut: 'Del', onSelect: () => remove(positions) }]}
          onDelete={remove}
          onMove={(positions, to) => void run(playlistApi.move(id, positions, to))}
          footer="Playlist order · Alt ↑↓ moves · Delete removes"
          empty={
            data && (
              <EmptyCard icon="playlist" title="This playlist is empty" className="w-[320px]">
                Right-click tracks anywhere in your library and choose Add to playlist.
              </EmptyCard>
            )
          }
        />
      </div>
    </div>
  );
}

// ── Home ───────────────────────────────────────────────────────────────────

export function HomeView() {
  const connected = useLibrary((s) => s.connected);
  const stats = useLibrary((s) => s.stats);
  const hasRoots = useLibrary((s) => s.roots.length > 0);
  const { data: albums } = useLibraryQuery('albums', libraryApi.albums);
  const { data: history } = useLibraryQuery('history:home', () => libraryApi.history(300), ['tracks', 'history']);
  const { data: top } = useLibraryQuery('top-tracks', () => libraryApi.trackList({ scope: { kind: 'all' }, sort: 'plays', descending: true }), ['tracks', 'history']);
  const { albumId, paused } = usePlayingAlbum();

  const byId = useMemo(() => new Map((albums ?? []).map((a) => [a.id, a])), [albums]);
  const recentAlbums = useMemo(() => {
    const seen = new Set<number>();
    const out = [];
    for (const h of history ?? []) {
      const album = h.track.albumId != null ? byId.get(h.track.albumId) : undefined;
      if (album && !seen.has(album.id)) {
        seen.add(album.id);
        out.push(album);
      }
      if (out.length === 6) break;
    }
    return out;
  }, [history, byId]);
  const added = useMemo(() => [...(albums ?? [])].sort((a, b) => b.addedAt - a.addedAt).slice(0, 12), [albums]);
  const mostPlayed = useLibraryQuery(
    top ? `top-rows:${top.ids.slice(0, 8).join(',')}` : null,
    () => libraryApi.tracks(top!.ids.slice(0, 8)),
    ['tracks', 'history'],
  ).data?.filter((t) => t.playCount > 0);

  if (connected && !hasRoots && !albums?.length) {
    return (
      <div className="view">
        <LibraryWelcome />
      </div>
    );
  }

  const hour = new Date().getHours();
  const greeting = hour < 5 ? 'Late night listening' : hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening';
  const shelf = (list: typeof added) => (
    <div className="shelf">
      {list.map((a, i) => (
        <AlbumTile key={a.id} album={a} playing={a.id === albumId} paused={paused} index={i} />
      ))}
    </div>
  );
  return (
    <div className="view">
      <PageHeader
        section="Home"
        title={greeting}
        meta={stats && `${count(stats.tracks)} tracks · ${count(stats.albums)} albums · ${formatLength(stats.durationMs)}`}
        actions={
          stats && stats.tracks > 0 ? (
            <button type="button" className="btn pri" onClick={() => void playScope({ kind: 'all' }, 'replace', true)}>
              <Icon name="shuffle" size={15} />
              Shuffle library
            </button>
          ) : undefined
        }
      />
      <ScanBanner />
      <div className="view-body flex flex-col gap-7 border-t border-ln px-7 pb-8 pt-5">
        {recentAlbums.length > 0 && (
          <section className="flex flex-col gap-3">
            <div className="flex items-baseline justify-between">
              <span className="cap">Continue listening</span>
              <button type="button" className="link-ac text-[12px]" onClick={() => navigate('recent')}>
                History
              </button>
            </div>
            {shelf(recentAlbums)}
          </section>
        )}
        {added.length > 0 && (
          <section className="flex flex-col gap-3">
            <div className="flex items-baseline justify-between">
              <span className="cap">Recently added</span>
              <button type="button" className="link-ac text-[12px]" onClick={() => navigate('albums')}>
                All albums
              </button>
            </div>
            {shelf(added)}
          </section>
        )}
        {mostPlayed && mostPlayed.length > 0 && (
          <section className="flex flex-col gap-1">
            <span className="cap pb-1">Most played</span>
            {mostPlayed.map((t, i) => (
              <div
                key={t.id}
                className="top-row"
                role="button"
                tabIndex={0}
                style={{ ['--i' as string]: i }}
                onDoubleClick={() => void playTracks(mostPlayed.map((x) => x.id), 'replace', i)}
                onKeyDown={(e) => e.key === 'Enter' && void playTracks(mostPlayed.map((x) => x.id), 'replace', i)}
                onContextMenu={contextMenu(() => trackMenu([t.id], [t], { play: () => void playTracks(mostPlayed.map((x) => x.id), 'replace', i) }).items, { label: 'Track actions' })}
              >
                <span className="mono t3 text-right text-[11px]">{i + 1}</span>
                <Artwork artworkId={t.artworkId} size={36} radius={3} title={t.album ?? t.title} />
                <span className="flex min-w-0 flex-col">
                  <span className="el font-semibold">{t.title}</span>
                  <span className="el t3 text-[12px]">{[t.artist, t.album].filter(Boolean).join(' · ')}</span>
                </span>
                <span className="mono t2 text-[11px]">{plural(t.playCount, 'play')}</span>
                <span className="mono t2 text-right text-[11.5px]">{formatTime(t.durationMs)}</span>
              </div>
            ))}
          </section>
        )}
        {added.length === 0 && (
          <div className="flex justify-center pt-10">
            <EmptyCard icon="album" title="Indexing your music" className="w-[340px]">
              Albums appear here as soon as the first folders are read.
            </EmptyCard>
          </div>
        )}
      </div>
    </div>
  );
}
