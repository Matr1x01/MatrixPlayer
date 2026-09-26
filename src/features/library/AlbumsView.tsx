import { useMemo, useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import { PageHeader } from '@/components/shell/PageHeader';
import { EmptyCard } from '@/components/ui/EmptyState';
import { Segmented } from '@/components/ui/Segmented';
import { useLibraryQuery } from '@/hooks/useLibraryQuery';
import { libraryApi } from '@/services/ipc';
import { isScanning, useLibrary } from '@/stores/library';
import { navigate } from '@/stores/navigation';
import { setViewPrefs, useViewPrefs, type AlbumFilter, type AlbumSort } from '@/stores/viewPrefs';
import type { AlbumRow } from '@/types/library';

import { AlbumGrid } from './albums';
import { FilterField, Pill, SortButton, useDebounced } from './controls';
import { count, formatSize, isHiResAlbum } from './format';
import { LibraryWelcome } from './LibraryWelcome';
import { ScanBanner } from './ScanBanner';

/** Lowercase without accents, to match the core's folding. */
export function fold(text: string): string {
  return text.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase();
}

const SORTS: { value: AlbumSort; label: string }[] = [
  { value: 'artist', label: 'Artist, then year' },
  { value: 'title', label: 'Title' },
  { value: 'year', label: 'Year, newest first' },
  { value: 'added', label: 'Recently added' },
  { value: 'played', label: 'Most played' },
];

const RECENT_DAYS = 30;

function sortAlbums(albums: AlbumRow[], sort: AlbumSort): AlbumRow[] {
  const sorted = [...albums];
  const byArtist = (a: AlbumRow, b: AlbumRow) =>
    a.sortKey.localeCompare(b.sortKey) || (a.year ?? 0) - (b.year ?? 0) || a.titleKey.localeCompare(b.titleKey);
  switch (sort) {
    case 'artist':
      return sorted.sort(byArtist);
    case 'title':
      return sorted.sort((a, b) => a.titleKey.localeCompare(b.titleKey) || byArtist(a, b));
    case 'year':
      return sorted.sort((a, b) => (b.year ?? 0) - (a.year ?? 0) || byArtist(a, b));
    case 'added':
      return sorted.sort((a, b) => b.addedAt - a.addedAt || byArtist(a, b));
    case 'played':
      return sorted.sort((a, b) => b.playCount - a.playCount || (b.lastPlayedAt ?? 0) - (a.lastPlayedAt ?? 0) || byArtist(a, b));
  }
}

const FILTERS: Record<AlbumFilter, (a: AlbumRow, now: number) => boolean> = {
  all: () => true,
  hires: isHiResAlbum,
  lossless: (a) => a.lossless,
  lossy: (a) => !a.lossless,
  recent: (a, now) => now - a.addedAt < RECENT_DAYS * 86400,
  unplayed: (a) => a.playCount === 0,
};

export function AlbumsView() {
  const { albumSort, albumFilter, tileSize } = useViewPrefs();
  const [filterText, setFilterText] = useState('');
  const text = fold(useDebounced(filterText).trim());
  const stats = useLibrary((s) => s.stats);
  const scan = useLibrary((s) => s.scan);
  const hasRoots = useLibrary((s) => s.roots.length > 0);
  const { data: albums, loading } = useLibraryQuery('albums', libraryApi.albums);

  const now = Date.now() / 1000;
  const counts = useMemo(() => {
    const c: Record<AlbumFilter, number> = { all: 0, hires: 0, lossless: 0, lossy: 0, recent: 0, unplayed: 0 };
    for (const a of albums ?? []) for (const f of Object.keys(c) as AlbumFilter[]) if (FILTERS[f](a, now)) c[f]++;
    return c;
    // `now` only matters at day granularity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [albums]);
  const shown = useMemo(() => {
    const filtered = (albums ?? []).filter(
      (a) => FILTERS[albumFilter](a, now) && (!text || fold(`${a.title} ${a.artistName} ${a.year ?? ''}`).includes(text)),
    );
    return sortAlbums(filtered, albumSort);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [albums, albumFilter, albumSort, text]);

  if (!hasRoots && useLibrary.getState().connected && !albums?.length) {
    return (
      <div className="view">
        <LibraryWelcome />
      </div>
    );
  }

  const indexBy = albumSort === 'artist' ? (a: AlbumRow) => a.sortKey : albumSort === 'title' ? (a: AlbumRow) => a.titleKey : undefined;
  return (
    <div className="view">
      <PageHeader
        section="Library"
        title="Albums"
        meta={stats && `${count(stats.albums)} albums · ${count(stats.tracks)} tracks · ${formatSize(stats.totalSize)}`}
        actions={
          <>
            <FilterField value={filterText} onChange={setFilterText} placeholder="Filter albums" />
            <SortButton value={albumSort} options={SORTS} onChange={(v) => setViewPrefs({ albumSort: v })} />
            <Segmented
              label="Layout"
              value="grid"
              onChange={(v) => v === 'list' && navigate('songs')}
              options={[
                { value: 'grid', label: <Icon name="grid" size={15} />, ariaLabel: 'Grid view' },
                { value: 'list', label: <Icon name="list" size={15} />, ariaLabel: 'List view', tip: 'Songs' },
              ]}
            />
          </>
        }
      />
      <div className="pills-row">
        <Pill on={albumFilter === 'all'} onClick={() => setViewPrefs({ albumFilter: 'all' })} n={counts.all}>
          All
        </Pill>
        <Pill on={albumFilter === 'hires'} onClick={() => setViewPrefs({ albumFilter: 'hires' })} n={counts.hires}>
          Hi-Res
        </Pill>
        <Pill on={albumFilter === 'lossless'} onClick={() => setViewPrefs({ albumFilter: 'lossless' })} n={counts.lossless}>
          Lossless
        </Pill>
        <Pill on={albumFilter === 'lossy'} onClick={() => setViewPrefs({ albumFilter: 'lossy' })} n={counts.lossy}>
          Lossy
        </Pill>
        <Pill on={albumFilter === 'recent'} onClick={() => setViewPrefs({ albumFilter: 'recent' })}>
          Recently added
        </Pill>
        <Pill on={albumFilter === 'unplayed'} onClick={() => setViewPrefs({ albumFilter: 'unplayed' })}>
          Unplayed
        </Pill>
        <label className="ml-auto flex items-center gap-2.5">
          <span className="cap" style={{ letterSpacing: '.06em' }}>
            Size
          </span>
          <input
            type="range"
            className="size-range"
            min={120}
            max={260}
            step={4}
            value={tileSize}
            aria-label="Cover size"
            onChange={(e) => setViewPrefs({ tileSize: Number(e.target.value) })}
            style={{ ['--p' as string]: `${((tileSize - 120) / 140) * 100}%` }}
          />
        </label>
      </div>
      <ScanBanner />
      {loading || (isScanning(scan) && !albums?.length) ? (
        <SkeletonGrid />
      ) : (
        <AlbumGrid
          albums={shown}
          tileSize={tileSize}
          indexBy={indexBy}
          empty={
            <EmptyCard icon="album" title={text || albumFilter !== 'all' ? 'No albums match' : 'No albums yet'} className="w-[340px]">
              {text || albumFilter !== 'all'
                ? 'Try another filter, or clear it to see every album.'
                : 'Albums appear here as your folders are indexed.'}
            </EmptyCard>
          }
        />
      )}
    </div>
  );
}

/** Placeholder tiles while the first albums are being indexed. */
export function SkeletonGrid({ count: n = 12 }: { count?: number }) {
  return (
    <div className="skeleton-grid" aria-hidden="true">
      {Array.from({ length: n }, (_, i) => (
        <div key={i} className="flex flex-col gap-[7px]" style={{ ['--i' as string]: i }}>
          <span className="sk" style={{ aspectRatio: '1 / 1', width: '100%' }} />
          <span className="sk" style={{ height: 10, width: '80%' }} />
          <span className="sk" style={{ height: 8, width: '55%' }} />
        </div>
      ))}
    </div>
  );
}
