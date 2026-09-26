import { useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import { PageHeader } from '@/components/shell/PageHeader';
import { EmptyCard } from '@/components/ui/EmptyState';
import { Segmented } from '@/components/ui/Segmented';
import { useLibraryQuery } from '@/hooks/useLibraryQuery';
import { libraryApi } from '@/services/ipc';
import { useLibrary } from '@/stores/library';
import { navigate } from '@/stores/navigation';
import { setViewPrefs, useViewPrefs } from '@/stores/viewPrefs';
import type { TrackQuery, TrackSort } from '@/types/library';

import { playQuery } from './actions';
import { FilterField, useDebounced } from './controls';
import { count, formatLength, formatSize } from './format';
import { LibraryWelcome } from './LibraryWelcome';
import { ScanBanner } from './ScanBanner';
import { SONG_COLUMNS, TrackTable } from './TrackTable';

const SORT_LABELS: Record<TrackSort, string> = {
  default: 'artist ▲ then album, disc, track',
  artist: 'artist ▲ then album, disc, track',
  title: 'title',
  album: 'album, then disc and track',
  year: 'year, then album',
  genre: 'genre, then artist',
  format: 'format',
  sampleRate: 'sample rate',
  bitDepth: 'bit depth',
  size: 'size',
  duration: 'duration',
  added: 'date added',
  plays: 'play count',
  lastPlayed: 'last played',
};

/** Every track in the library (Songs table board). */
export function SongsView() {
  const { songSort, songDescending, songHiRes } = useViewPrefs();
  const [filter, setFilter] = useState('');
  const text = useDebounced(filter.trim(), 200);
  const stats = useLibrary((s) => s.stats);
  const connected = useLibrary((s) => s.connected);
  const hasRoots = useLibrary((s) => s.roots.length > 0);
  const query: TrackQuery = { scope: { kind: 'all' }, filter: text, sort: songSort, descending: songDescending, hiResOnly: songHiRes };
  const { data } = useLibraryQuery(`songs:${JSON.stringify(query)}`, () => libraryApi.trackList(query));

  if (connected && !hasRoots && !stats?.tracks) {
    return (
      <div className="view">
        <LibraryWelcome />
      </div>
    );
  }

  const onSort = (key: TrackSort) =>
    setViewPrefs(key === songSort ? { songDescending: !songDescending } : { songSort: key, songDescending: false });
  const sorted = `Sorted by ${SORT_LABELS[songSort].replace('▲', songDescending ? '▼' : '▲')}`;
  const filtered = text || songHiRes;
  return (
    <div className="view">
      <PageHeader
        section="Library"
        title="Songs"
        meta={
          stats &&
          (filtered && data
            ? `${count(data.ids.length)} of ${count(stats.tracks)} songs · ${formatLength(data.durationMs)} · ${formatSize(data.totalSize)}`
            : `${count(stats.tracks)} songs · ${formatLength(stats.durationMs)} · ${formatSize(stats.totalSize)}`)
        }
        actions={
          <>
            <FilterField value={filter} onChange={setFilter} placeholder="Filter songs" width={240} />
            <button type="button" className="btn" style={{ fontWeight: 500 }} aria-pressed={songHiRes} onClick={() => setViewPrefs({ songHiRes: !songHiRes })}>
              Hi-Res only
              <span className="mono text-[10px]" style={{ color: songHiRes ? 'var(--ac)' : 'var(--tx3)' }}>
                {songHiRes ? 'on' : 'off'}
              </span>
            </button>
            <button type="button" className="btn" style={{ fontWeight: 500 }} onClick={() => data && void playQuery(query, 'replace', true)} disabled={!data?.ids.length}>
              <Icon name="shuffle" size={15} className="t3" />
              Shuffle
            </button>
            <Segmented
              label="Layout"
              value="list"
              onChange={(v) => v === 'grid' && navigate('albums')}
              options={[
                { value: 'grid', label: <Icon name="grid" size={15} />, ariaLabel: 'Grid view', tip: 'Albums' },
                { value: 'list', label: <Icon name="list" size={15} />, ariaLabel: 'List view' },
              ]}
            />
          </>
        }
      />
      <ScanBanner />
      <div className="flex min-h-0 flex-1 flex-col px-[18px]">
        <TrackTable
          ids={data?.ids ?? []}
          columns={SONG_COLUMNS}
          sort={{ key: songSort === 'default' ? 'artist' : songSort, descending: songDescending }}
          onSort={onSort}
          footer={`${sorted}${filtered ? ' · filtered' : ''}`}
          revealPlaying
          empty={
            data && (
              <EmptyCard icon="search" title={text ? `No matches for “${text}”` : 'No songs'} className="w-[340px]" actions={
                filtered ? (
                  <button type="button" className="btn" onClick={() => { setFilter(''); setViewPrefs({ songHiRes: false }); }}>
                    Clear filters
                  </button>
                ) : undefined
              }>
                {text ? 'Filters understand format:, rate:, bits: and year:, e.g. format:flac rate:>48k.' : 'Songs appear here as your folders are indexed.'}
              </EmptyCard>
            )
          }
        />
      </div>
    </div>
  );
}
