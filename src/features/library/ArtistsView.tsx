import { memo, useEffect, useMemo, useState, type CSSProperties } from 'react';

import { Icon } from '@/components/icons/Icon';
import { Artwork, fallbackColors, monogram } from '@/components/ui/Artwork';
import { contextMenu, type MenuItem } from '@/components/ui/ContextMenu';
import { EmptyCard } from '@/components/ui/EmptyState';
import { IconButton } from '@/components/ui/IconButton';
import { formatTime } from '@/features/player/format';
import { useLibraryQuery } from '@/hooks/useLibraryQuery';
import { useVariableList } from '@/hooks/useVirtual';
import { libraryApi } from '@/services/ipc';
import { setFavorite, useLibrary } from '@/stores/library';
import { navigate, useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { setViewPrefs, useViewPrefs } from '@/stores/viewPrefs';
import type { AlbumRow, ArtistDetail, ArtistKind, ArtistRow } from '@/types/library';
import type { Rgb } from '@/types/player';

import { playScope, playTracks } from './actions';
import { AlbumTile, scopeTrackIds, usePlayingAlbum } from './albums';
import { fold } from './AlbumsView';
import { FilterField, useDebounced } from './controls';
import { count, formatDate, formatLength, formatSize, indexLetter, plural } from './format';
import { LibraryWelcome } from './LibraryWelcome';
import { playlistSubmenu } from './trackMenu';
import { SONG_COLUMNS, TrackTable } from './TrackTable';

/** A round artist picture: their newest cover, or a monogram. */
export function ArtistAvatar({ name, artworkId, size = 32, style }: { name: string; artworkId: string | null; size?: number; style?: CSSProperties }) {
  if (artworkId) return <Artwork artworkId={artworkId} size={size} radius={size} title={name} className="av" style={style} />;
  const [from, to] = fallbackColors(name);
  return (
    <span className="av" style={{ width: size, height: size, background: `linear-gradient(160deg, ${from}, ${to})`, fontSize: Math.round(size * 0.34), ...style }}>
      {monogram(name)}
    </span>
  );
}

function artistMenu(artist: { id: number; name: string }): MenuItem[] {
  const scope = { kind: 'artist' as const, id: artist.id };
  return [
    { label: 'Play all', icon: 'play', filledIcon: true, onSelect: () => void playScope(scope) },
    { label: 'Shuffle', icon: 'shuffle', onSelect: () => void playScope(scope, 'replace', true) },
    { label: 'Play next', icon: 'playNext', onSelect: () => void playScope(scope, 'next', undefined, artist.name) },
    { label: 'Add to queue', icon: 'queue', onSelect: () => void playScope(scope, 'append', undefined, artist.name) },
    'separator',
    { label: 'Add to playlist', icon: 'playlist', submenu: playlistSubmenu(() => scopeTrackIds(scope)) },
    { label: 'Add to favorites', icon: 'heart', onSelect: () => void scopeTrackIds(scope).then((ids) => setFavorite(ids, true)) },
  ];
}

const KINDS: { value: ArtistKind; label: string }[] = [
  { value: 'albumArtists', label: 'Album artists' },
  { value: 'all', label: 'All artists' },
  { value: 'composers', label: 'Composers' },
];

type Item = { kind: 'head'; letter: string } | { kind: 'artist'; artist: ArtistRow };

const ArtistRowItem = memo(function ArtistRowItem({ artist, on, playing }: { artist: ArtistRow; on: boolean; playing: boolean }) {
  const detail =
    artist.albumCount > 0 ? `${plural(artist.albumCount, 'album')} · ${count(artist.trackCount)} trk` : plural(artist.trackCount, 'track');
  return (
    <button
      type="button"
      className={on ? 'ar on' : 'ar'}
      aria-current={on ? 'true' : undefined}
      onClick={() => useNavigation.getState().replace({ view: 'artists', id: artist.id })}
      onContextMenu={contextMenu(() => artistMenu(artist), { label: `${artist.name} actions` })}
    >
      <ArtistAvatar name={artist.name} artworkId={artist.artworkId} />
      <span className="flex min-w-0 flex-col text-left">
        <span className="el font-semibold" style={playing ? { color: 'var(--ac)' } : undefined}>
          {artist.name}
        </span>
        <span className="el mono t3 text-[10.5px]">{detail}</span>
      </span>
    </button>
  );
});

function ArtistList({ artists, selected, kind }: { artists: ArtistRow[]; selected: number | null; kind: ArtistKind }) {
  const [filter, setFilter] = useState('');
  const text = fold(useDebounced(filter).trim());
  const playingArtist = usePlayer((s) => s.track?.library?.artistId ?? null);
  const items = useMemo(() => {
    const out: Item[] = [];
    let letter = '';
    for (const artist of artists) {
      if (text && !fold(artist.name).includes(text)) continue;
      const l = indexLetter(artist.sortKey);
      if (l !== letter) {
        letter = l;
        out.push({ kind: 'head', letter });
      }
      out.push({ kind: 'artist', artist });
    }
    return out;
  }, [artists, text]);
  const heights = useMemo(() => items.map((i) => (i.kind === 'head' ? 28 : 46)), [items]);
  const list = useVariableList<HTMLDivElement>(heights);

  // Keep the selected artist in view when it changes from elsewhere.
  useEffect(() => {
    const at = items.findIndex((i) => i.kind === 'artist' && i.artist.id === selected);
    if (at >= 0) list.scrollToIndex(at);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, items.length]);

  const rendered = [];
  for (let i = list.start; i < list.end; i++) {
    const item = items[i]!;
    rendered.push(
      <div key={item.kind === 'head' ? `h${item.letter}` : item.artist.id} style={{ position: 'absolute', top: list.offsets[i], left: 0, right: 0 }}>
        {item.kind === 'head' ? (
          <div className="cap mono ar-head">{item.letter}</div>
        ) : (
          <ArtistRowItem artist={item.artist} on={item.artist.id === selected} playing={item.artist.id === playingArtist} />
        )}
      </div>,
    );
  }

  return (
    <section className="ar-col" aria-label="Artists">
      <div className="flex flex-col gap-2.5 px-3.5 pb-2.5 pt-4">
        <div className="flex items-baseline justify-between">
          <span className="text-[17px] font-bold">Artists</span>
          <span className="mono t3 text-[10.5px]">{count(artists.length)} · A–Z</span>
        </div>
        <FilterField value={filter} onChange={setFilter} placeholder="Filter artists" width={248} />
        <div className="flex gap-1.5">
          {KINDS.map((k) => (
            <button key={k.value} type="button" className={kind === k.value ? 'chip chip-on' : 'chip'} style={{ height: 22 }} onClick={() => setViewPrefs({ artistKind: k.value })}>
              {k.label}
            </button>
          ))}
        </div>
      </div>
      <div ref={list.ref} className="ar-scroll">
        <div style={{ height: list.totalHeight, position: 'relative' }}>{rendered}</div>
        {items.length === 0 && <p className="t3 px-4 py-6 text-center text-[12px]">No artists match “{filter}”.</p>}
      </div>
    </section>
  );
}

type Tab = 'overview' | 'albums' | 'singles' | 'tracks' | 'appears' | 'folders';

function isSingle(a: AlbumRow) {
  return a.trackCount <= 6 && a.durationMs < 30 * 60_000;
}

const rgba = ([r, g, b]: Rgb, a: number) => `rgba(${r}, ${g}, ${b}, ${a})`;

function ArtistPanel({ id }: { id: number }) {
  const { data, error } = useLibraryQuery<ArtistDetail>(`artist:${id}`, () => libraryApi.artist(id), ['tracks', 'history']);
  const tab = (useNavigation((s) => s.history[s.index]?.tab) ?? 'overview') as Tab;
  const setTab = (t: Tab) => useNavigation.getState().replace({ view: 'artists', id, tab: t });
  const { albumId, paused } = usePlayingAlbum();
  const tracksQuery = useLibraryQuery(tab === 'tracks' ? `artist-tracks:${id}` : null, () => libraryApi.trackList({ scope: { kind: 'artist', id } }));

  if (error) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <EmptyCard icon="artist" title="Artist not found">This artist is no longer in your library.</EmptyCard>
      </div>
    );
  }
  if (!data) return <div className="flex-1"><div className="artist-hero sk-hero" /></div>;

  const albums = data.albums.filter((a) => !isSingle(a));
  const singles = data.albums.filter(isSingle);
  const palette = data.artwork?.palette;
  const hero: CSSProperties = palette
    ? {
        background: `linear-gradient(0deg, var(--bg1) 0%, transparent 70%), radial-gradient(60% 120% at 80% 20%, ${rgba(palette[0], 0.32)}, transparent 60%), linear-gradient(120deg, ${rgba(palette[1], 0.35)}, ${rgba(palette[2], 0.3)})`,
      }
    : {};
  const topMax = Math.max(1, ...data.topTracks.map((t) => t.playCount));
  const formatsTotal = data.formats.reduce((t, f) => t + f.tracks, 0) || 1;
  const releases = data.albums.length;
  const tabs: { id: Tab; label: string; n?: number }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'albums', label: 'Albums', n: albums.length },
    { id: 'singles', label: 'Singles & EPs', n: singles.length },
    { id: 'tracks', label: 'All tracks', n: data.trackCount },
    { id: 'appears', label: 'Appears on', n: data.appearsOn.length },
    { id: 'folders', label: 'Folders', n: data.folders.length },
  ];
  const grid = (list: AlbumRow[]) => (
    <div className="disc-grid">
      {list.map((a, i) => (
        <AlbumTile key={a.id} album={a} playing={a.id === albumId} paused={paused} subtitle="year" index={i} />
      ))}
    </div>
  );

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <section className="artist-hero" style={hero}>
        <ArtistAvatar name={data.name} artworkId={data.artwork?.id ?? null} size={132} style={{ boxShadow: '0 16px 40px rgba(0,0,0,.5), 0 0 0 3px var(--bg1)' }} />
        <div className="flex min-w-0 flex-1 flex-col gap-2 pb-1">
          <span className="cap el" style={{ color: 'var(--tx2)' }}>
            {['Artist', data.genres.slice(0, 3).join(', ')].filter(Boolean).join(' · ')}
          </span>
          <h1 className="hero-title el" style={{ fontSize: 42 }}>
            {data.name}
          </h1>
          <span className="mono t2 el text-[11.5px]">
            {[
              releases && plural(releases, 'release'),
              plural(data.trackCount, 'track'),
              formatLength(data.durationMs),
              formatSize(data.totalSize),
              data.playCount > 0 && `played ${data.playCount}×`,
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
        </div>
        <div className="flex gap-2 pb-1">
          <button type="button" className="btn pri h36" onClick={() => void playScope({ kind: 'artist', id })}>
            <Icon name="play" size={16} filled />
            Play all
          </button>
          <button type="button" className="btn h36" onClick={() => void playScope({ kind: 'artist', id }, 'replace', true)}>
            <Icon name="shuffle" size={16} />
            Shuffle
          </button>
          <IconButton icon="more" label="More artist actions" size={36} strokeWidth={2.8} onClick={(e) => contextMenu(() => artistMenu(data), { label: 'Artist actions' })(e)} />
        </div>
      </section>
      <div className="tabs px-7" role="tablist">
        {tabs.map((t) => (
          <button key={t.id} type="button" role="tab" aria-selected={tab === t.id} className={tab === t.id ? 'on' : undefined} disabled={t.n === 0} onClick={() => setTab(t.id)}>
            {t.label}
            {t.n != null && <span className="n">{t.n}</span>}
          </button>
        ))}
      </div>

      {tab === 'tracks' ? (
        <div className="flex min-h-0 flex-1 flex-col px-[18px] pt-2">
          <TrackTable ids={tracksQuery.data?.ids ?? []} columns={SONG_COLUMNS.filter((c) => c !== 'artist')} revealPlaying />
        </div>
      ) : (
        <div className="view-body" key={tab}>
          {tab === 'overview' && (
            <div className="artist-overview">
              <div className="flex min-w-0 flex-col gap-[18px]">
                {data.topTracks.length > 0 && (
                  <div className="flex flex-col gap-0.5">
                    <div className="flex justify-between px-2.5 pb-1.5">
                      <span className="cap">{data.playCount > 0 ? 'Most played in your library' : 'Tracks'}</span>
                      <span className="cap">Plays</span>
                    </div>
                    {data.topTracks.map((t, i) => (
                      <div
                        key={t.id}
                        className="top-row"
                        role="button"
                        tabIndex={0}
                        onDoubleClick={() => void playTracks(data.topTracks.map((x) => x.id), 'replace', i)}
                        onKeyDown={(e) => e.key === 'Enter' && void playTracks(data.topTracks.map((x) => x.id), 'replace', i)}
                        style={{ ['--i' as string]: i }}
                      >
                        <span className="mono t3 text-right text-[11px]">{i + 1}</span>
                        <Artwork artworkId={t.artworkId} size={36} radius={3} title={t.album ?? t.title} />
                        <span className="flex min-w-0 flex-col">
                          <span className="el font-semibold">{t.title}</span>
                          <span className="el t3 text-[12px]">{t.album}</span>
                        </span>
                        <span className="flex items-center gap-2">
                          <span className="bar flex-1" style={{ height: 3 }}>
                            <i style={{ width: `${(t.playCount / topMax) * 100}%`, background: 'var(--tx3)' }} />
                          </span>
                          <span className="mono w-[26px] text-right text-[11px]">{t.playCount}</span>
                        </span>
                        <span className="mono t2 text-right text-[11.5px]">{formatTime(t.durationMs)}</span>
                      </div>
                    ))}
                  </div>
                )}
                {data.albums.length > 0 && (
                  <div className="flex flex-col gap-2.5">
                    <span className="cap px-2.5">Discography</span>
                    {grid(data.albums)}
                  </div>
                )}
                {data.albums.length === 0 && data.appearsOn.length > 0 && (
                  <div className="flex flex-col gap-2.5">
                    <span className="cap px-2.5">Appears on</span>
                    {grid(data.appearsOn)}
                  </div>
                )}
              </div>
              <aside className="flex flex-col gap-4">
                {data.formats.length > 0 && (
                  <div className="side-card">
                    <span className="cap">Library formats</span>
                    <div className="fmt-bar">
                      {data.formats.slice(0, 4).map((f, i) => (
                        <span key={f.label} style={{ width: `${(f.tracks / formatsTotal) * 100}%`, background: ['var(--ac)', 'var(--tx2)', 'var(--bg4)', 'var(--ln2)'][i] }} />
                      ))}
                    </div>
                    <div className="mono grid grid-cols-[10px_1fr_auto] items-center gap-x-2 gap-y-1.5 text-[10.5px]">
                      {data.formats.slice(0, 4).map((f, i) => (
                        <FormatLegend key={f.label} label={f.label} tracks={f.tracks} color={['var(--ac)', 'var(--tx2)', 'var(--bg4)', 'var(--ln2)'][i]!} />
                      ))}
                    </div>
                  </div>
                )}
                <div className="side-card">
                  <span className="cap">Details</span>
                  <div className="grid grid-cols-[76px_1fr] gap-x-2.5 gap-y-1.5 text-[12px]">
                    {data.firstYear != null && (
                      <>
                        <span className="t3">Active</span>
                        <span>{data.firstYear === data.lastYear ? data.firstYear : `${data.firstYear} – ${data.lastYear}`}</span>
                      </>
                    )}
                    {data.genres.length > 0 && (
                      <>
                        <span className="t3">Genres</span>
                        <span>{data.genres.join(', ')}</span>
                      </>
                    )}
                    <span className="t3">First added</span>
                    <span>{formatDate(data.firstAdded)}</span>
                    <span className="t3">Last played</span>
                    <span>{data.lastPlayed ? formatDate(data.lastPlayed, true) : 'Never'}</span>
                    {data.composedCount > 0 && (
                      <>
                        <span className="t3">Composed</span>
                        <span>{plural(data.composedCount, 'track')}</span>
                      </>
                    )}
                  </div>
                </div>
                {data.homeFolder && (
                  <div className="side-card">
                    <span className="cap">On disk</span>
                    <span className="mono t2 break-all text-[10.5px]">{data.homeFolder.displayPath.replace(/^\/home\/[^/]+/, '~')}/</span>
                    <button type="button" className="link-ac" onClick={() => navigate('folders', data.homeFolder!.id)}>
                      Browse folder
                      <Icon name="chevR" size={13} />
                    </button>
                  </div>
                )}
              </aside>
            </div>
          )}
          {tab === 'albums' && <div className="px-7 py-5">{grid(albums)}</div>}
          {tab === 'singles' && <div className="px-7 py-5">{grid(singles)}</div>}
          {tab === 'appears' && <div className="px-7 py-5">{grid(data.appearsOn)}</div>}
          {tab === 'folders' && (
            <div className="flex flex-col gap-1 px-7 py-4">
              {data.folders.map((f, i) => (
                <button key={f.id} type="button" className="folder-line" style={{ ['--i' as string]: i }} onClick={() => navigate('folders', f.id)}>
                  <Icon name="folder" size={16} className="t3" />
                  <span className="mono el flex-1 text-left text-[11.5px]">{f.displayPath.replace(/^\/home\/[^/]+/, '~')}</span>
                  <span className="mono t3 text-[10.5px]">{plural(f.trackCount, 'track')}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </main>
  );
}

function FormatLegend({ label, tracks, color }: { label: string; tracks: number; color: string }) {
  return (
    <>
      <span style={{ width: 8, height: 8, background: color, borderRadius: 1, boxShadow: 'inset 0 0 0 1px var(--ln2)' }} />
      <span>{label}</span>
      <span className="t3">{tracks} trk</span>
    </>
  );
}

export function ArtistsView({ id }: { id?: number }) {
  const kind = useViewPrefs((s) => s.artistKind);
  const { data: artists, loading } = useLibraryQuery(`artists:${kind}`, () => libraryApi.artists(kind));
  const hasRoots = useLibrary((s) => s.roots.length > 0);
  const connected = useLibrary((s) => s.connected);
  const selected = id ?? artists?.[0]?.id ?? null;

  if (connected && !hasRoots && !artists?.length) {
    return (
      <div className="view">
        <LibraryWelcome />
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1">
      <ArtistList artists={artists ?? []} selected={selected} kind={kind} />
      {selected != null ? (
        <ArtistPanel key={selected} id={selected} />
      ) : (
        <div className="flex flex-1 items-center justify-center">
          {!loading && <EmptyCard icon="artist" title="No artists yet">Artists appear as your folders are indexed.</EmptyCard>}
        </div>
      )}
    </div>
  );
}
