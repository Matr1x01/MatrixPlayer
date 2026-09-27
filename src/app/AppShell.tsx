import { getCurrentWebview } from '@tauri-apps/api/webview';
import { useEffect, useSyncExternalStore, type CSSProperties } from 'react';

import { Sidebar } from '@/components/shell/Sidebar';
import { SidebarRail } from '@/components/shell/SidebarRail';
import { TitleBar } from '@/components/shell/TitleBar';
import { ContextMenuHost } from '@/components/ui/ContextMenu';
import { DialogHost } from '@/components/ui/Dialog';
import { DropOverlay } from '@/components/ui/DropOverlay';
import { ToastHost } from '@/components/ui/ToastHost';
import { TooltipHost } from '@/components/ui/TooltipHost';
import { AmbientBackdrop } from '@/features/now-playing/AmbientBackdrop';
import { PlayerBar } from '@/features/player/PlayerBar';
import { SearchDialog } from '@/features/search/SearchDialog';
import { useRoute } from '@/stores/navigation';
import { usePreferences } from '@/stores/preferences';

import { ViewOutlet } from './ViewOutlet';

/** Below this width the sidebar becomes the icon rail (Responsive board). */
const RAIL_QUERY = '(max-width: 1180px)';

function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (notify) => {
      const list = window.matchMedia(query);
      list.addEventListener('change', notify);
      return () => list.removeEventListener('change', notify);
    },
    () => window.matchMedia(query).matches,
  );
}

/**
 * Title bar · sidebar or rail · view · player bar. Now Playing swaps to the
 * rail, drops the player bar (its transport is in the view) and lights the
 * background from the artwork; Settings replaces the sidebar with its own
 * section list.
 */
export function AppShell() {
  const route = useRoute();
  const view = route.view;
  const prefs = usePreferences((s) => s.prefs);
  const narrow = useMediaQuery(RAIL_QUERY);
  const nowPlaying = view === 'nowPlaying';
  const settings = view === 'settings';
  const rail = nowPlaying || narrow || !prefs.sidebarExpanded;

  useEffect(() => {
    void getCurrentWebview().setZoom(prefs.interfaceScale);
  }, [prefs.interfaceScale]);

  const classes = [
    'app',
    `th-${prefs.theme}`,
    prefs.accent && `ac-${prefs.accent}`,
    `density-${prefs.density}`,
    prefs.reduceMotion && 'reduce-motion',
  ]
    .filter(Boolean)
    .join(' ');
  const sidebarWidth = settings ? '240px' : rail ? '60px' : '220px';
  const vars = { '--sidebar-w': sidebarWidth, '--playerbar-h': nowPlaying ? '0px' : '84px' } as CSSProperties;

  return (
    <div className={classes} style={vars}>
      {nowPlaying && <AmbientBackdrop />}
      <TitleBar variant={nowPlaying ? 'nowPlaying' : settings ? 'settings' : 'default'} />
      <div className="shell-body">
        {/* Settings brings its own section list in place of the library sidebar. */}
        {!settings && (rail ? <SidebarRail canExpand={!nowPlaying && !narrow} /> : <Sidebar />)}
        {/* Keyed by route so each page enters with the view transition. */}
        <div
          className={nowPlaying ? 'shell-main shell-main-clear' : 'shell-main'}
          key={view === 'artists' || view === 'folders' ? view : `${view}:${route.id ?? ''}`}
        >
          <ViewOutlet route={route} />
        </div>
      </div>
      {/* Now Playing has its own transport. Settings keeps the player bar,
          so music can be paused while changing the output or playback. */}
      {!nowPlaying && <PlayerBar />}
      <ToastHost />
      <SearchDialog />
      <DialogHost />
      <ContextMenuHost />
      <TooltipHost />
      <DropOverlay />
    </div>
  );
}
