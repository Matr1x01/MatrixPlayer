import { Icon } from '@/components/icons/Icon';
import { askConfirm } from '@/components/ui/Dialog';
import { IconButton } from '@/components/ui/IconButton';
import { count, formatAgo, formatSize } from '@/features/library/format';
import { addLibraryFolder } from '@/features/library/LibraryWelcome';
import { libraryApi } from '@/services/ipc';
import { isScanning, useLibrary } from '@/stores/library';
import { run } from '@/stores/toasts';
import type { RootInfo } from '@/types/library';

import { SettingRow } from './AppearanceSettings';

async function removeRoot(root: RootInfo) {
  const ok = await askConfirm({
    title: `Remove ${root.display} from the library?`,
    body: `${count(root.trackCount)} tracks, their play counts and favorites leave the library. Your files are not touched.`,
    confirm: 'Remove folder',
    danger: true,
  });
  if (ok) await run(libraryApi.removeRoot(root.id));
}

/** Settings › Library & folders. */
export function LibrarySettings() {
  const roots = useLibrary((s) => s.roots);
  const scan = useLibrary((s) => s.scan);
  const stats = useLibrary((s) => s.stats);
  const scanning = isScanning(scan);
  return (
    <>
      <div className="flex flex-col gap-2 border-t border-ln py-3.5">
        <div className="flex items-center justify-between">
          <span className="flex flex-col gap-0.5">
            <span className="font-semibold">Music folders</span>
            <span className="t3 text-[12px]">Indexed recursively and watched for changes</span>
          </span>
          <button type="button" className="btn" onClick={() => void addLibraryFolder()}>
            <Icon name="plus" size={15} />
            Add folder…
          </button>
        </div>
        <div className="flex flex-col gap-1.5 pt-1">
          {roots.length === 0 && <span className="t3 text-[12.5px]">No folders yet. Add the folders where you keep your music.</span>}
          {roots.map((root) => (
            <div key={root.id} className="root-row">
              <span className="dot" style={{ background: root.online ? 'var(--ok)' : 'var(--warn)' }} />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="mono el text-[12px]">{root.display}</span>
                <span className="mono t3 text-[10.5px]">
                  {root.online
                    ? `${count(root.trackCount)} tracks · ${formatSize(root.totalSize)} · scanned ${formatAgo(root.lastScanAt)}`
                    : `Offline — ${count(root.trackCount)} tracks kept until the drive is back`}
                </span>
              </span>
              <IconButton icon="refresh" label="Rescan this folder" size={28} iconSize={15} disabled={scanning} onClick={() => void run(libraryApi.rescan(root.id))} />
              <IconButton icon="trash" label="Remove from library" size={28} iconSize={15} onClick={() => void removeRoot(root)} />
            </div>
          ))}
        </div>
      </div>
      <SettingRow title="Rescan" hint={scanning ? 'A scan is running' : 'Look for new, changed and removed files everywhere'}>
        <button type="button" className="btn" disabled={scanning || roots.length === 0} onClick={() => void run(libraryApi.rescan())}>
          <Icon name="refresh" size={15} />
          Rescan library
        </button>
      </SettingRow>
      <SettingRow title="Re-read all tags" hint="After editing tags in another program without changing the files' dates">
        <button type="button" className="btn" disabled={scanning || roots.length === 0} onClick={() => void run(libraryApi.rescan(undefined, true))}>
          Re-read tags
        </button>
      </SettingRow>
      <SettingRow title="Watching" hint="Changes on disk are picked up automatically">
        <span className="chip ok">On</span>
      </SettingRow>
      <SettingRow title="Moved and renamed files" hint="Favorites, play counts and playlists follow the file">
        <span className="t2">Matched by size and modification time</span>
      </SettingRow>
      {stats && (
        <SettingRow title="Library" hint="Everything indexed">
          <span className="mono t2">
            {count(stats.tracks)} tracks · {count(stats.albums)} albums · {formatSize(stats.totalSize)}
            {stats.unavailable > 0 && ` · ${count(stats.unavailable)} offline`}
          </span>
        </SettingRow>
      )}
    </>
  );
}
