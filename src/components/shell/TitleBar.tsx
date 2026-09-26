import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useState } from 'react';

import { AppMark } from '@/components/icons/AppMark';
import { Icon } from '@/components/icons/Icon';
import { IconButton } from '@/components/ui/IconButton';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';

import { WindowControls } from './WindowControls';

/**
 * Library search is part of the library milestone; until then the field is
 * shown, but inert, so the title bar keeps the design's layout.
 */
interface SearchFieldProps {
  width: number;
  placeholder: string;
  tip: string;
  glass?: boolean;
  shortcut?: string;
}

function SearchField({ width, placeholder, tip, glass, shortcut }: SearchFieldProps) {
  return (
    <div className={glass ? 'fld tb-search glass' : 'fld tb-search'} style={{ width }} aria-disabled="true" data-tip={tip}>
      <Icon name="search" size={16} />
      <span className="flex-1 el">{placeholder}</span>
      {shortcut && <span className="kbd">{shortcut}</span>}
    </div>
  );
}

const LIBRARY_SEARCH_TIP = 'Search becomes available once your library is indexed';

function useFullscreen(): [boolean, () => void] {
  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => {
    const win = getCurrentWindow();
    void win.isFullscreen().then(setFullscreen);
    const unlisten = win.onResized(() => void win.isFullscreen().then(setFullscreen));
    return () => void unlisten.then((u) => u());
  }, []);
  const toggle = () => {
    const win = getCurrentWindow();
    void win.setFullscreen(!fullscreen).then(() => setFullscreen(!fullscreen));
  };
  return [fullscreen, toggle];
}

export function TitleBar({ variant }: { variant: 'default' | 'nowPlaying' | 'settings' }) {
  const canGoBack = useNavigation((s) => s.index > 0);
  const canGoForward = useNavigation((s) => s.index < s.history.length - 1);
  const back = useNavigation((s) => s.back);
  const forward = useNavigation((s) => s.forward);
  const navigate = useNavigation((s) => s.navigate);
  const album = usePlayer((s) => s.track?.metadata.album);
  const [fullscreen, toggleFullscreen] = useFullscreen();

  return (
    <header className="tb" data-tauri-drag-region>
      {variant === 'default' && (
        <>
          <div className="tb-brand" data-tauri-drag-region>
            <AppMark />
            <span data-tauri-drag-region>MatrixPlayer</span>
          </div>
          <IconButton icon="chevL" label="Back" shortcut="Alt ←" onClick={back} disabled={!canGoBack} />
          <IconButton icon="chevR" label="Forward" shortcut="Alt →" onClick={forward} disabled={!canGoForward} />
        </>
      )}
      {variant === 'settings' && (
        <>
          <div className="tb-brand" data-tauri-drag-region>
            <AppMark />
            <span data-tauri-drag-region>MatrixPlayer</span>
          </div>
          <IconButton icon="chevL" label="Back to library" shortcut="Esc" onClick={() => (canGoBack ? back() : navigate('home'))} />
          <span className="ml-1 font-semibold" data-tauri-drag-region>
            Settings
          </span>
        </>
      )}
      {variant === 'nowPlaying' && (
        <>
          <div className="tb-mark" data-tauri-drag-region>
            <AppMark />
          </div>
          <button
            type="button"
            className="btn gh"
            style={{ height: 30, padding: '0 8px', gap: 6, fontWeight: 500 }}
            onClick={() => (canGoBack ? back() : navigate('home'))}
            data-tip="Close Now Playing"
            data-tip-key="Esc"
          >
            <Icon name="chevD" size={16} />
            Now Playing
          </button>
          {album && (
            <span className="flex min-w-0 items-baseline gap-1.5" data-tauri-drag-region>
              <span className="t3 text-[12px]">from</span>
              <span className="el t2 text-[12px] font-semibold">{album}</span>
            </span>
          )}
        </>
      )}

      <div className="tb-center" data-tauri-drag-region>
        {variant === 'default' && (
          <SearchField width={440} placeholder="Search songs, albums, artists, folders…" tip={LIBRARY_SEARCH_TIP} shortcut="Ctrl K" />
        )}
        {variant === 'nowPlaying' && (
          <SearchField width={360} placeholder="Search library…" tip={LIBRARY_SEARCH_TIP} glass shortcut="Ctrl K" />
        )}
        {variant === 'settings' && (
          <SearchField width={360} placeholder="Search settings" tip="Searching settings is not available yet" />
        )}
      </div>

      <div className="flex items-center gap-1.5">
        {variant === 'nowPlaying' && (
          <>
            <IconButton icon="mini" label="Mini player (not available yet)" iconSize={16} disabled />
            <IconButton
              icon={fullscreen ? 'exitFull' : 'full'}
              label={fullscreen ? 'Exit full screen' : 'Full screen'}
              shortcut="F11"
              iconSize={16}
              onClick={toggleFullscreen}
            />
          </>
        )}
        <span className="vsep mx-1.5" />
        <WindowControls />
      </div>
    </header>
  );
}
