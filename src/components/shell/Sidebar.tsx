import { useMemo } from 'react';

import { Icon } from '@/components/icons/Icon';
import { Artwork } from '@/components/ui/Artwork';
import { IconButton } from '@/components/ui/IconButton';
import { count, formatSizeShort } from '@/features/library/format';
import { isScanning, useLibrary } from '@/stores/library';
import { PARENT_VIEW, useNavigation, useRoute, type ViewId } from '@/stores/navigation';
import { useDsp } from '@/stores/dsp';
import { usePlayer } from '@/stores/player';
import { updatePreferences } from '@/stores/preferences';

import { NAV_GROUPS } from './navItems';

/** Counts shown beside nav items, from the library and the queue. */
function useCounts(): Partial<Record<ViewId, string>> {
  const queueLength = usePlayer((s) => s.queue.entries.length);
  const stats = useLibrary((s) => s.stats);
  const eqOn = useDsp((s) => !!s.settings?.eqEnabled && !s.settings.bypass);
  const counts: Partial<Record<ViewId, string>> = {};
  if (eqOn) counts.equalizer = 'ON';
  if (queueLength > 0) counts.queue = count(queueLength);
  if (stats && stats.tracks > 0) {
    counts.artists = count(stats.albumArtists);
    counts.albums = count(stats.albums);
    counts.songs = count(stats.tracks);
    counts.genres = count(stats.genres);
  }
  if (stats?.playlists) counts.playlists = count(stats.playlists);
  if (stats?.favorites) counts.favorites = count(stats.favorites);
  return counts;
}

/** The dot and summary at the foot of the sidebar. */
export function LibraryStatus() {
  const scan = useLibrary((s) => s.scan);
  const stats = useLibrary((s) => s.stats);
  const roots = useLibrary((s) => s.roots);
  const navigate = useNavigation((s) => s.navigate);
  const scanning = isScanning(scan);
  const offline = roots.some((r) => !r.online);
  const text = scanning
    ? scan.phase === 'reading' && scan.total > 0
      ? `Indexing ${Math.round((scan.processed / scan.total) * 100)}% · ${count(scan.processed)} / ${count(scan.total)}`
      : 'Scanning folders…'
    : stats && stats.tracks > 0
      ? `${count(stats.tracks)} tracks · ${formatSizeShort(stats.totalSize)}`
      : roots.length
        ? 'Library is empty'
        : 'No music folders yet';
  return (
    <button
      type="button"
      className="flex min-w-0 flex-1 items-center gap-2 text-left"
      data-tip={offline ? 'A library folder is offline' : 'Library & folders'}
      onClick={() => navigate('settings', undefined, 'library')}
    >
      <span className={scanning ? 'dot pulse' : 'dot'} style={{ background: scanning ? 'var(--ac)' : offline ? 'var(--warn)' : roots.length ? 'var(--ok)' : 'var(--tx3)' }} />
      <span className="mono t3 el flex-1 text-[10px]">{text}</span>
    </button>
  );
}

/** The full 220 px sidebar (Library — Albums board). */
export function Sidebar() {
  const route = useRoute();
  const view = PARENT_VIEW[route.view] ?? route.view;
  const navigate = useNavigation((s) => s.navigate);
  const counts = useCounts();
  const playlists = useLibrary((s) => s.playlists);
  const pinned = useMemo(() => playlists.filter((p) => p.pinned), [playlists]);

  return (
    <nav className="sb" aria-label="Library navigation">
      {NAV_GROUPS.map((group) => (
        <div key={group.label} className="sb-group">
          <div className="cap sb-cap">{group.label}</div>
          {group.items.map((item) => (
            <button
              key={item.view}
              type="button"
              className={view === item.view ? 'nv act' : 'nv'}
              aria-current={view === item.view ? 'page' : undefined}
              onClick={() => navigate(item.view)}
            >
              <Icon name={item.icon} />
              <span className="el">{item.label}</span>
              {counts[item.view] && <span className="ct mono">{counts[item.view]}</span>}
            </button>
          ))}
        </div>
      ))}
      {pinned.length > 0 && (
        <div className="sb-group sb-pins">
          <div className="cap sb-cap">Pinned</div>
          {pinned.map((p) => (
            <button
              key={p.id}
              type="button"
              className={route.view === 'playlist' && route.id === p.id ? 'nv act pin' : 'nv pin'}
              onClick={() => navigate('playlist', p.id)}
            >
              <Artwork artworkId={p.artworkId} size={18} radius={3} title={p.name} />
              <span className="el">{p.name}</span>
              <span className="ct mono">{p.trackCount}</span>
            </button>
          ))}
        </div>
      )}

      <div className="sb-foot">
        <button
          type="button"
          className={view === 'settings' ? 'nv act' : 'nv'}
          aria-current={view === 'settings' ? 'page' : undefined}
          onClick={() => navigate('settings')}
        >
          <Icon name="gear" />
          <span>Settings</span>
        </button>
        <div className="sb-status">
          <LibraryStatus />
          <IconButton
            icon="side"
            label="Collapse sidebar to icon rail"
            size={26}
            iconSize={16}
            onClick={() => void updatePreferences({ sidebarExpanded: false })}
          />
        </div>
      </div>
    </nav>
  );
}
