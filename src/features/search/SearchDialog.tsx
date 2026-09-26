import { Fragment, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { create } from 'zustand';

import { Icon } from '@/components/icons/Icon';
import { Artwork } from '@/components/ui/Artwork';
import { playScope, playTracks } from '@/features/library/actions';
import { ArtistAvatar } from '@/features/library/ArtistsView';
import { useDebounced } from '@/features/library/controls';
import { albumBadge, badgeClass, count, formatSize, plural, trackBadge } from '@/features/library/format';
import { formatTime } from '@/features/player/format';
import { libraryApi } from '@/services/ipc';
import { navigate } from '@/stores/navigation';
import type { SearchResults, TrackScope } from '@/types/library';
import type { InsertMode } from '@/types/player';

export const useSearch = create<{ open: boolean; text: string }>()(() => ({ open: false, text: '' }));

export function openSearch(text?: string) {
  useSearch.setState((s) => ({ open: true, text: text ?? s.text }));
}
export function closeSearch() {
  useSearch.setState({ open: false });
}

type Scope = 'all' | 'songs' | 'albums' | 'artists' | 'genres' | 'folders' | 'playlists';
const SCOPES: { id: Scope; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'songs', label: 'Songs' },
  { id: 'albums', label: 'Albums' },
  { id: 'artists', label: 'Artists' },
  { id: 'genres', label: 'Genres' },
  { id: 'folders', label: 'Folders' },
  { id: 'playlists', label: 'Playlists' },
];

/** One selectable result, with what Enter / Shift+Enter / Ctrl+Enter do. */
interface Hit {
  key: string;
  play: (mode: InsertMode) => void;
  open: () => void;
}

function fold1(c: string) {
  return c.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase();
}

/** Wraps the parts of `text` that match any query word in <mark>. */
export function Highlight({ text, query }: { text: string; query: string }) {
  const words = query
    .split(/\s+/)
    .map((w) => fold1(w))
    .filter((w) => w && !w.includes(':'));
  if (!words.length) return <>{text}</>;
  // Fold char by char so positions in the folded text map back to `text`.
  const chars = [...text];
  let folded = '';
  const origin: number[] = [];
  chars.forEach((c, i) => {
    for (const f of fold1(c)) {
      folded += f;
      origin.push(i);
    }
  });
  const marked = new Array<boolean>(chars.length).fill(false);
  for (const word of words) {
    let at = folded.indexOf(word);
    while (at >= 0) {
      // Only matches at the start of a word, like the search itself.
      if (at === 0 || !/\p{L}|\p{N}/u.test(folded[at - 1]!)) {
        for (let k = at; k < at + word.length; k++) marked[origin[k]!] = true;
      }
      at = folded.indexOf(word, at + 1);
    }
  }
  const parts: ReactNode[] = [];
  let run = '';
  let inMark = false;
  chars.forEach((c, i) => {
    if (marked[i] !== inMark) {
      if (run) parts.push(inMark ? <mark key={i}>{run}</mark> : <Fragment key={i}>{run}</Fragment>);
      run = '';
      inMark = marked[i]!;
    }
    run += c;
  });
  if (run) parts.push(inMark ? <mark key="end">{run}</mark> : <Fragment key="end">{run}</Fragment>);
  return <>{parts}</>;
}

function useResults(text: string, scope: Scope) {
  const [results, setResults] = useState<SearchResults | null>(null);
  const query = useDebounced(text.trim(), 90);
  useEffect(() => {
    if (!query) {
      setResults(null);
      return;
    }
    let cancelled = false;
    void libraryApi
      .search(query, scope === 'all' ? 4 : 40)
      .then((r) => !cancelled && setResults(r))
      .catch(() => !cancelled && setResults(null));
    return () => {
      cancelled = true;
    };
  }, [query, scope]);
  return results;
}

/** Global search (Global search board): Ctrl K from anywhere. */
export function SearchDialog() {
  const { open, text } = useSearch();
  const [scope, setScope] = useState<Scope>('all');
  const [active, setActive] = useState(0);
  const [closing, setClosing] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const results = useResults(open ? text : '', scope);

  useEffect(() => {
    if (open) {
      setClosing(false);
      requestAnimationFrame(() => input.current?.select());
    }
  }, [open]);
  useEffect(() => setActive(0), [results, scope]);

  const close = () => {
    setClosing(true);
    window.setTimeout(closeSearch, 120);
  };

  const sections = useMemo(() => buildSections(results, scope, text), [results, scope, text]);
  const hits = sections.flatMap((s) => s.hits);

  if (!open) return null;
  const counts: Record<Scope, number> = {
    all: results ? results.trackCount + results.albums.length + results.artists.length + results.genres.length + results.folders.length + results.playlists.length : 0,
    songs: results?.trackCount ?? 0,
    albums: results?.albums.length ?? 0,
    artists: results?.artists.length ?? 0,
    genres: results?.genres.length ?? 0,
    folders: results?.folders.length ?? 0,
    playlists: results?.playlists.length ?? 0,
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const hit = hits[active];
    switch (e.key) {
      case 'ArrowDown':
        setActive((a) => Math.min(hits.length - 1, a + 1));
        break;
      case 'ArrowUp':
        setActive((a) => Math.max(0, a - 1));
        break;
      case 'Tab': {
        const at = SCOPES.findIndex((s) => s.id === scope);
        const step = e.shiftKey ? -1 : 1;
        setScope(SCOPES[(at + step + SCOPES.length) % SCOPES.length]!.id);
        break;
      }
      case 'Enter':
        if (!hit) return;
        if (e.altKey) hit.open();
        else hit.play(e.shiftKey ? 'next' : e.ctrlKey ? 'append' : 'replace');
        close();
        break;
      case 'Escape':
        if (text) useSearch.setState({ text: '' });
        else close();
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  let index = -1;
  return (
    <div className={closing ? 'search-scrim closing' : 'search-scrim'} onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <section className="search-dlg" role="dialog" aria-modal="true" aria-label="Search" onKeyDown={onKeyDown}>
        <label className="search-input">
          <Icon name="search" size={20} style={{ color: 'var(--ac)' }} />
          <input
            ref={input}
            value={text}
            placeholder="Search songs, albums, artists, folders…"
            spellCheck={false}
            onChange={(e) => useSearch.setState({ text: e.target.value })}
            aria-controls="search-results"
          />
          {results && (
            <span className="mono t3 flex-none text-[11px]">
              {plural(counts.all, 'result')} · {results.tookMs < 1 ? '<1' : Math.round(results.tookMs)} ms
            </span>
          )}
          {text && (
            <button type="button" className="ib" style={{ width: 28, height: 28 }} aria-label="Clear search" onClick={() => useSearch.setState({ text: '' })}>
              <Icon name="x" size={15} />
            </button>
          )}
          <span className="kbd">Esc</span>
        </label>
        <div className="search-scopes" role="tablist">
          {SCOPES.map((s) => (
            <button
              key={s.id}
              type="button"
              role="tab"
              aria-selected={scope === s.id}
              className={[scope === s.id && 'on', results && counts[s.id] === 0 && 'dim'].filter(Boolean).join(' ')}
              onClick={() => setScope(s.id)}
            >
              {s.label}
              {results && <span className="n">{count(counts[s.id])}</span>}
            </button>
          ))}
          <span className="t3 ml-auto flex items-center gap-1.5">
            <span className="text-[11px]">Filters</span>
            {['format:', 'rate:>48k', 'year:'].map((f) => (
              <button key={f} type="button" className="kbd" onClick={() => useSearch.setState({ text: `${text.trim()} ${f}`.trimStart() })}>
                {f}
              </button>
            ))}
          </span>
        </div>

        <div className="search-body" id="search-results">
          {!text.trim() && <p className="t3 px-3 py-6 text-center text-[12.5px]">Type to search your library. Words match the start of titles, artists, albums and file names.</p>}
          {text.trim() && results && hits.length === 0 && (
            <div className="flex flex-col items-center gap-2 px-3 py-8 text-center">
              <Icon name="search" size={26} className="t3" />
              <span className="text-[15px] font-bold">No matches for “{text.trim()}”</span>
              <span className="t2 text-[12.5px]">Check the spelling, or search for fewer words.</span>
            </div>
          )}
          {sections.map((section) => (
            <div key={section.title} className={section.half ? 'search-half' : undefined}>
              {section.title && (
                <div className="search-hdr">
                  <span className="cap">{section.title}</span>
                  {section.more && (
                    <button type="button" onClick={() => setScope(section.more!)}>
                      Show all
                    </button>
                  )}
                </div>
              )}
              {section.render.map((node, i) => {
                index += 1;
                const at = index;
                const hit = section.hits[i]!;
                return (
                  <div
                    key={hit.key}
                    className={at === active ? 'res-wrap foc' : 'res-wrap'}
                    onMouseEnter={() => setActive(at)}
                    onClick={() => {
                      hit.open();
                      close();
                    }}
                    onDoubleClick={() => {
                      hit.play('replace');
                      close();
                    }}
                  >
                    {node}
                  </div>
                );
              })}
            </div>
          ))}
        </div>

        <footer className="search-foot t3">
          <span><span className="kbd">↑</span><span className="kbd">↓</span>Navigate</span>
          <span><span className="kbd">Enter</span>Play</span>
          <span><span className="kbd">Shift+Enter</span>Play next</span>
          <span><span className="kbd">Ctrl+Enter</span>Add to queue</span>
          <span><span className="kbd">Tab</span>Scope</span>
          <span className="ml-auto"><span className="kbd">Alt+Enter</span>Show in library</span>
        </footer>
      </section>
    </div>
  );
}

interface Section {
  title: string;
  hits: Hit[];
  render: ReactNode[];
  more?: Scope;
  half?: boolean;
}

function buildSections(results: SearchResults | null, scope: Scope, text: string): Section[] {
  if (!results) return [];
  const q = text;
  const sections: Section[] = [];
  const all = scope === 'all';
  const trackHit = (ids: number[], i: number, albumId: number | null, folderId: number): Hit => ({
    key: `t${ids[i]}`,
    play: (mode) => void playTracks(mode === 'replace' ? ids : [ids[i]!], mode, mode === 'replace' ? i : undefined),
    open: () => (albumId != null ? navigate('album', albumId) : navigate('folders', folderId)),
  });
  const scopeHit = (key: string, scopeOf: TrackScope, open: () => void): Hit => ({
    key,
    play: (mode) => void playScope(scopeOf, mode),
    open,
  });

  if (all && results.top) {
    const top = results.top;
    const hit: Hit =
      top.kind === 'album'
        ? scopeHit(`top-a${top.album.id}`, { kind: 'album', id: top.album.id }, () => navigate('album', top.album.id))
        : top.kind === 'artist'
          ? scopeHit(`top-r${top.artist.id}`, { kind: 'artist', id: top.artist.id }, () => navigate('artists', top.artist.id))
          : trackHit([top.track.id], 0, top.track.albumId, top.track.folderId);
    const title = top.kind === 'album' ? top.album.title : top.kind === 'artist' ? top.artist.name : top.track.title;
    const sub =
      top.kind === 'album'
        ? `Album · ${top.album.artistName}${top.album.year ? ` · ${top.album.year}` : ''} · ${plural(top.album.trackCount, 'track')}`
        : top.kind === 'artist'
          ? `Artist · ${plural(top.artist.albumCount, 'album')} · ${plural(top.artist.trackCount, 'track')}`
          : `Song · ${top.track.artist ?? 'Unknown artist'}${top.track.album ? ` · ${top.track.album}` : ''}`;
    const badge = top.kind === 'album' ? albumBadge(top.album) : top.kind === 'track' ? trackBadge(top.track) : null;
    sections.push({
      title: 'Top result',
      hits: [hit],
      render: [
        <div className="top-res" key="top">
          {top.kind === 'artist' ? (
            <ArtistAvatar name={top.artist.name} artworkId={top.artist.artworkId} size={60} />
          ) : (
            <Artwork artworkId={top.kind === 'album' ? top.album.artworkId : top.track.artworkId} size={60} title={title} />
          )}
          <div className="flex min-w-0 flex-col gap-[3px]">
            <span className="el text-[16px] font-bold">
              <Highlight text={title} query={q} />
            </span>
            <span className="t2 el text-[12px]">
              <Highlight text={sub} query={q} />
            </span>
            {badge && (
              <span className="flex gap-[5px]">
                <span className={top.kind === 'album' ? badgeClass(top.album) : top.kind === 'track' ? badgeClass(top.track) : 'chip'}>{badge}</span>
              </span>
            )}
          </div>
          <div className="flex gap-1.5">
            <button
              type="button"
              className="btn pri"
              onClick={(e) => {
                e.stopPropagation();
                hit.play('replace');
                closeSearch();
              }}
            >
              <Icon name="play" size={14} filled />
              Play
            </button>
            <button
              type="button"
              className="btn"
              onClick={(e) => {
                e.stopPropagation();
                hit.play('append');
              }}
            >
              Queue
            </button>
          </div>
        </div>,
      ],
    });
  }

  if ((all || scope === 'songs') && results.tracks.length) {
    const ids = results.tracks.map((t) => t.id);
    const tracks = all ? results.tracks.slice(0, 4) : results.tracks;
    sections.push({
      title: `Songs · ${count(results.trackCount)}`,
      more: all && results.trackCount > 4 ? 'songs' : undefined,
      hits: tracks.map((t, i) => trackHit(ids, i, t.albumId, t.folderId)),
      render: tracks.map((t) => (
        <div className="res" key={t.id}>
          <Artwork artworkId={t.artworkId} size={32} radius={3} title={t.album ?? t.title} />
          <span className="flex min-w-0 flex-col">
            <span className="el font-semibold">
              <Highlight text={t.title} query={q} />
            </span>
            <span className="el t3 text-[11.5px]">
              <Highlight text={[t.artist ?? 'Unknown artist', t.album].filter(Boolean).join(' · ')} query={q} />
            </span>
          </span>
          <span className="flex items-center gap-2.5">
            <span className={badgeClass(t)}>{trackBadge(t)}</span>
            <span className="mono t2 w-8 text-right text-[11px]">{formatTime(t.durationMs)}</span>
          </span>
        </div>
      )),
    });
  }

  const half = all;
  if ((all || scope === 'albums') && results.albums.length) {
    sections.push({
      title: `Albums · ${results.albums.length}`,
      half,
      hits: results.albums.map((a) => scopeHit(`a${a.id}`, { kind: 'album', id: a.id }, () => navigate('album', a.id))),
      render: results.albums.map((a) => (
        <div className="res" key={a.id}>
          <Artwork artworkId={a.artworkId} size={32} radius={3} title={a.title} />
          <span className="flex min-w-0 flex-col">
            <span className="el font-semibold">
              <Highlight text={a.title} query={q} />
            </span>
            <span className="el t3 text-[11.5px]">{[a.artistName, a.year].filter(Boolean).join(' · ')}</span>
          </span>
          <span />
        </div>
      )),
    });
  }
  if ((all || scope === 'folders') && results.folders.length) {
    sections.push({
      title: `Folders · ${results.folders.length}`,
      half,
      hits: results.folders.map((f) => scopeHit(`f${f.id}`, { kind: 'folder', id: f.id, recursive: true }, () => navigate('folders', f.id))),
      render: results.folders.map((f) => (
        <div className="res" key={f.id}>
          <span className="res-icon">
            <Icon name="folder" size={16} />
          </span>
          <span className="flex min-w-0 flex-col">
            <span className="el mono text-[11.5px] font-medium">
              {f.displayPath.replace(/^\/home\/[^/]+/, '~').slice(0, -f.name.length)}
              <Highlight text={f.name} query={q} />
            </span>
            <span className="el t3 text-[11.5px]">
              {plural(f.trackCount, 'file')} · {formatSize(f.totalSize)}
            </span>
          </span>
          <span />
        </div>
      )),
    });
  }
  if ((all || scope === 'artists') && results.artists.length) {
    sections.push({
      title: `Artists · ${results.artists.length}`,
      half,
      hits: results.artists.map((a) => scopeHit(`r${a.id}`, { kind: 'artist', id: a.id }, () => navigate('artists', a.id))),
      render: results.artists.map((a) => (
        <div className="res" key={a.id}>
          <ArtistAvatar name={a.name} artworkId={a.artworkId} size={32} />
          <span className="flex min-w-0 flex-col">
            <span className="el font-semibold">
              <Highlight text={a.name} query={q} />
            </span>
            <span className="el t3 text-[11.5px]">
              {a.albumCount > 0 ? `${plural(a.albumCount, 'album')} · ` : ''}
              {plural(a.trackCount, 'track')}
            </span>
          </span>
          <span />
        </div>
      )),
    });
  }
  if ((all || scope === 'playlists') && results.playlists.length) {
    sections.push({
      title: `Playlists · ${results.playlists.length}`,
      half,
      hits: results.playlists.map((p) => scopeHit(`p${p.id}`, { kind: 'playlist', id: p.id }, () => navigate('playlist', p.id))),
      render: results.playlists.map((p) => (
        <div className="res" key={p.id}>
          <Artwork artworkId={p.artworkId} size={32} radius={3} title={p.name} />
          <span className="flex min-w-0 flex-col">
            <span className="el font-semibold">
              <Highlight text={p.name} query={q} />
            </span>
            <span className="el t3 text-[11.5px]">Playlist · {plural(p.trackCount, 'track')}</span>
          </span>
          <span />
        </div>
      )),
    });
  }
  if ((all || scope === 'genres') && results.genres.length) {
    sections.push({
      title: `Genres · ${results.genres.length}`,
      half,
      hits: results.genres.map((g) => scopeHit(`g${g.id}`, { kind: 'genre', id: g.id }, () => navigate('genre', g.id))),
      render: results.genres.map((g) => (
        <div className="res" key={g.id}>
          <Artwork artworkId={g.artworkIds[0] ?? null} size={32} radius={3} title={g.name} />
          <span className="flex min-w-0 flex-col">
            <span className="el font-semibold">
              <Highlight text={g.name} query={q} />
            </span>
            <span className="el t3 text-[11.5px]">
              Genre · {plural(g.albumCount, 'album')} · {plural(g.trackCount, 'track')}
            </span>
          </span>
          <span />
        </div>
      )),
    });
  }
  return sections;
}
