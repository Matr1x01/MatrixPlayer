import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useState } from 'react';

import { AppMark } from '@/components/icons/AppMark';
import { Icon } from '@/components/icons/Icon';
import { IconButton } from '@/components/ui/IconButton';
import { openSearch } from '@/features/search/SearchDialog';
import { libraryApi } from '@/services/ipc';
import { isScanning, useLibrary } from '@/stores/library';
import { useNavigation } from '@/stores/navigation';
import { run } from '@/stores/toasts';
import { usePlayer } from '@/stores/player';

import { WindowControls } from './WindowControls';

interface SearchFieldProps {
  width: number;
  placeholder: string;
  glass?: boolean;
  shortcut?: string;
  onOpen?: () => void;
  tip?: string;
}

/** Looks like a field; opens the search dialog (the design's title-bar search). */
function SearchField({ width, placeholder, glass, shortcut, onOpen, tip }: SearchFieldProps) {
  return (
    <button
      type="button"
      className={glass ? 'fld tb-search glass' : 'fld tb-search'}
      style={{ width }}
      onClick={onOpen}
      disabled={!onOpen}
      data-tip={tip}
      aria-label={placeholder}
    >
      <Icon name="search" size={16} />
      <span className="el flex-1 text-left">{placeholder}</span>
      {shortcut && <span className="kbd">{shortcut}</span>}
    </button>
  );
}

/** Rescans the library; spins while a scan runs. */
function RescanButton() {
  const scan = useLibrary((s) => s.scan);
  const hasRoots = useLibrary((s) => s.roots.length > 0);
  const scanning = isScanning(scan);
  if (!hasRoots) return null;
  const label = scanning
    ? scan.phase === 'reading' && scan.total
      ? `Indexing ${scan.processed.toLocaleString('en-US')} of ${scan.total.toLocaleString('en-US')}`
      : 'Scanning your folders'
    : scan.finishedAt
      ? 'Library is up to date — rescan'
      : 'Rescan library';
  return (
    <IconButton
      icon="refresh"
      label={label}
      iconSize={16}
      className={scanning ? 'spin' : undefined}
      active={scanning}
      onClick={() => !scanning && void run(libraryApi.rescan())}
    />
  );
}

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
          <SearchField width={440} placeholder="Search songs, albums, artists, folders…" shortcut="Ctrl K" onOpen={() => openSearch()} />
        )}
        {variant === 'nowPlaying' && (
          <SearchField width={360} placeholder="Search library…" glass shortcut="Ctrl K" onOpen={() => openSearch()} />
        )}
        {variant === 'settings' && (
          <SearchField width={360} placeholder="Search settings" tip="Searching settings is not available yet" />
        )}
      </div>

      <div className="flex items-center gap-1.5">
        {variant === 'default' && <RescanButton />}
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
