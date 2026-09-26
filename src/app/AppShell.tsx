import { getCurrentWebview } from '@tauri-apps/api/webview';
import { useEffect, useSyncExternalStore, type CSSProperties } from 'react';

import { Sidebar } from '@/components/shell/Sidebar';
import { SidebarRail } from '@/components/shell/SidebarRail';
import { TitleBar } from '@/components/shell/TitleBar';
import { ToastHost } from '@/components/ui/ToastHost';
import { TooltipHost } from '@/components/ui/TooltipHost';
import { AmbientBackdrop } from '@/features/now-playing/AmbientBackdrop';
import { PlayerBar } from '@/features/player/PlayerBar';
import { useView } from '@/stores/navigation';
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
  const view = useView();
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
  const vars = { '--sidebar-w': sidebarWidth, '--playerbar-h': nowPlaying || settings ? '0px' : '84px' } as CSSProperties;

  return (
    <div className={classes} style={vars}>
      {nowPlaying && <AmbientBackdrop />}
      <TitleBar variant={nowPlaying ? 'nowPlaying' : settings ? 'settings' : 'default'} />
      <div className="shell-body">
        {/* Settings brings its own section list in place of the library sidebar. */}
        {!settings && (rail ? <SidebarRail canExpand={!nowPlaying && !narrow} /> : <Sidebar />)}
        <div className={nowPlaying ? 'shell-main shell-main-clear' : 'shell-main'} key={view}>
          <ViewOutlet view={view} />
        </div>
      </div>
      {/* Now Playing has its own transport; Settings has none (per the design). */}
      {!nowPlaying && !settings && <PlayerBar />}
      <ToastHost />
      <TooltipHost />
    </div>
  );
}
