import { useCallback, useEffect, useMemo, useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import type { IconName } from '@/components/icons/paths';
import { Artwork } from '@/components/ui/Artwork';
import { contextMenu } from '@/components/ui/ContextMenu';
import { EmptyCard } from '@/components/ui/EmptyState';
import { EqIndicator } from '@/components/ui/EqIndicator';
import { Toggle } from '@/components/ui/Toggle';
import { formatTime } from '@/features/player/format';
import { useLibraryQuery } from '@/hooks/useLibraryQuery';
import { libraryApi } from '@/services/ipc';
import { useLibrary } from '@/stores/library';
import { navigate, useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { run, showToast } from '@/stores/toasts';
import { setViewPrefs, useViewPrefs } from '@/stores/viewPrefs';
import type { FolderDetail, FolderNode, OtherFile, RootInfo, TrackRow } from '@/types/library';

import { playScope, playTracks } from './actions';
import { badgeClass, count, formatAgo, formatLength, formatSize, trackBadge } from './format';
import { LibraryWelcome } from './LibraryWelcome';
import { ScanBanner } from './ScanBanner';
import { trackMenu } from './trackMenu';

const home = (path: string) => path.replace(/^\/home\/[^/]+/, '~');

function useChildren() {
  const [children, setChildren] = useState<ReadonlyMap<number, FolderNode[]>>(new Map());
  const version = useLibrary((s) => s.versions.tracks);
  useEffect(() => setChildren(new Map()), [version]);
  const load = useCallback(async (id: number) => {
    const nodes = await libraryApi.folderChildren(id);
    setChildren((prev) => new Map(prev).set(id, nodes));
  }, []);
  return { children, load };
}

function TreeNode({
  node,
  depth,
  selected,
  expanded,
  childrenOf,
  toggle,
}: {
  node: FolderNode;
  depth: number;
  selected: number | null;
  expanded: ReadonlySet<number>;
  childrenOf: ReadonlyMap<number, FolderNode[]>;
  toggle: (id: number) => void;
}) {
  const open = expanded.has(node.id);
  const kids = childrenOf.get(node.id);
  const on = node.id === selected;
  return (
    <>
      <div className={on ? 'tr-node on' : 'tr-node'} style={{ paddingLeft: 6 + depth * 14 }}>
        <button
          type="button"
          className="tr-chev"
          aria-label={open ? 'Collapse' : 'Expand'}
          style={{ opacity: node.hasChildren ? 1 : 0 }}
          disabled={!node.hasChildren}
          onClick={() => toggle(node.id)}
        >
          <Icon name="chevR" size={12} style={{ transform: open ? 'rotate(90deg)' : undefined }} />
        </button>
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
          onClick={() => useNavigation.getState().replace({ view: 'folders', id: node.id })}
          onDoubleClick={() => node.hasChildren && toggle(node.id)}
        >
          {node.artworkId && !node.hasChildren ? (
            <Artwork artworkId={node.artworkId} size={16} radius={2} title={node.name} />
          ) : (
            <Icon name="folder" size={15} style={{ color: on ? 'var(--ac)' : 'var(--tx3)' }} />
          )}
          <span className="el">{node.name}</span>
          <span className="n">{count(node.trackCount)}</span>
        </button>
      </div>
      {open &&
        kids?.map((child) => (
          <TreeNode key={child.id} node={child} depth={depth + 1} selected={selected} expanded={expanded} childrenOf={childrenOf} toggle={toggle} />
        ))}
    </>
  );
}

function FolderTree({ roots, selected, crumbs }: { roots: RootInfo[]; selected: number | null; crumbs: number[] }) {
  const { children, load } = useChildren();
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(new Set());

  // Open the path to the selected folder, and the roots themselves.
  useEffect(() => {
    const open = [...roots.map((r) => r.folderId).filter((id): id is number => id != null), ...crumbs];
    setExpanded((prev) => new Set([...prev, ...open]));
  }, [roots, crumbs]);
  useEffect(() => {
    for (const id of expanded) if (!children.has(id)) void load(id).catch(() => undefined);
  }, [expanded, children, load]);

  const toggle = (id: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <section className="folder-tree" aria-label="Folder tree">
      {roots.map((root) => (
        <div key={root.id} className="flex flex-col gap-px">
          <div className="flex items-center justify-between px-2 pb-1.5">
            <span className="cap flex items-center gap-1.5">
              <span className="dot" style={{ background: root.online ? 'var(--ok)' : 'var(--warn)' }} />
              {root.display}
            </span>
            <span className="mono t3 text-[9.5px]">{root.online ? formatSize(root.totalSize) : 'offline'}</span>
          </div>
          {root.folderId != null &&
            children
              .get(root.folderId)
              ?.map((node) => (
                <TreeNode key={node.id} node={node} depth={0} selected={selected} expanded={expanded} childrenOf={children} toggle={toggle} />
              ))}
          {root.folderId == null && <span className="t3 px-3 text-[12px]">Not indexed yet</span>}
        </div>
      ))}
    </section>
  );
}

const FILE_ICONS: Record<OtherFile['kind'], IconName> = {
  image: 'image',
  cue: 'file',
  log: 'file',
  playlist: 'playlist',
  text: 'file',
  file: 'file',
};
const FILE_LABELS: Record<OtherFile['kind'], string> = {
  image: 'Image',
  cue: 'Cue sheet',
  log: 'Rip log',
  playlist: 'Playlist',
  text: 'Text',
  file: 'File',
};

function date(seconds: number | null) {
  return seconds ? new Date(seconds * 1000).toLocaleDateString('en-CA') : '';
}

function Contents({ folder, showFiles }: { folder: FolderDetail; showFiles: boolean }) {
  const playing = usePlayer((s) => s.track?.library?.trackId ?? null);
  const paused = usePlayer((s) => s.status !== 'playing');
  const ids = folder.tracks.map((t) => t.id);
  const menu = (t: TrackRow) => contextMenu(() => trackMenu([t.id], [t]).items, { label: 'Track actions' });
  const empty = folder.subfolders.length === 0 && folder.tracks.length === 0 && (!showFiles || folder.otherFiles.length === 0);
  return (
    <section className="folder-contents" aria-label="Folder contents">
      <div className="fr hd">
        <span />
        <span>Name</span>
        <span>Format</span>
        <span className="text-right">Time</span>
        <span className="text-right">Size</span>
        <span className="text-right">Modified</span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {folder.subfolders.map((f, i) => (
          <div key={`f${f.id}`} className="fr" role="button" tabIndex={0} style={{ ['--i' as string]: Math.min(i, 20) }} onDoubleClick={() => navigate('folders', f.id)} onKeyDown={(e) => e.key === 'Enter' && navigate('folders', f.id)}>
            <span className="flex justify-center">
              {f.artworkId ? <Artwork artworkId={f.artworkId} size={20} radius={2} title={f.name} /> : <Icon name="folder" size={16} className="t3" />}
            </span>
            <span className="fn el mono text-[11.5px]">{f.name}/</span>
            <span>
              <span className="chip">Folder · {count(f.trackCount)}</span>
            </span>
            <span />
            <span className="m text-right">{formatSize(f.totalSize)}</span>
            <span />
          </div>
        ))}
        {folder.tracks.map((t, i) => (
          <div
            key={`t${t.id}`}
            className={['fr', t.id === playing && 'now', !t.available && 'gone'].filter(Boolean).join(' ')}
            role="button"
            tabIndex={0}
            style={{ ['--i' as string]: Math.min(folder.subfolders.length + i, 20) }}
            onDoubleClick={() => void playTracks(ids, 'replace', i)}
            onKeyDown={(e) => e.key === 'Enter' && void playTracks(ids, 'replace', i)}
            onContextMenu={menu(t)}
          >
            <span className="flex justify-center">
              {t.id === playing ? <EqIndicator paused={paused} /> : <Icon name={t.unreadable ? 'warn' : 'note'} size={16} className="t3" />}
            </span>
            <span className="fn el mono text-[11.5px]">{t.fileName}</span>
            <span>
              <span className={badgeClass(t)}>{trackBadge(t)}</span>
            </span>
            <span className="m text-right">{formatTime(t.durationMs)}</span>
            <span className="m text-right">{formatSize(t.fileSize)}</span>
            <span className="m t3 text-right">{date(t.addedAt)}</span>
          </div>
        ))}
        {showFiles &&
          folder.otherFiles.map((f) => (
            <div key={`o${f.name}`} className="fr other">
              <span className="flex justify-center">
                <Icon name={FILE_ICONS[f.kind]} size={16} className="t3" />
              </span>
              <span className="fn el mono text-[11.5px]">{f.name}</span>
              <span>
                <span className="chip">{FILE_LABELS[f.kind]}</span>
              </span>
              <span />
              <span className="m text-right">{formatSize(f.size)}</span>
              <span className="m t3 text-right">{date(f.modified)}</span>
            </div>
          ))}
        {empty && <p className="t3 px-3 py-8 text-center text-[12px]">This folder has no audio files.</p>}
      </div>
    </section>
  );
}

function Details({ folder }: { folder: FolderDetail }) {
  const duration = folder.tracks.reduce((t, r) => t + r.durationMs, 0);
  const size = folder.tracks.reduce((t, r) => t + r.fileSize, 0) + folder.otherFiles.reduce((t, f) => t + f.size, 0);
  const formats = useMemo(() => {
    const byBadge = new Map<string, number>();
    for (const t of folder.tracks) byBadge.set(trackBadge(t), (byBadge.get(trackBadge(t)) ?? 0) + 1);
    return [...byBadge].map(([badge, n]) => `${n} × ${badge}`).join(', ');
  }, [folder.tracks]);
  const others = useMemo(() => {
    const byExt = new Map<string, number>();
    for (const f of folder.otherFiles) {
      const ext = f.name.includes('.') ? f.name.split('.').pop()!.toLowerCase() : f.kind;
      byExt.set(ext, (byExt.get(ext) ?? 0) + 1);
    }
    return [...byExt].map(([ext, n]) => `${n} ${ext}`).join(' · ');
  }, [folder.otherFiles]);
  const cover = folder.otherFiles.find((f) => f.kind === 'image' && /^(cover|folder|front)\./i.test(f.name));
  const n = folder.tracks.length;
  const consistent = n > 0 && folder.tagged === n;

  return (
    <aside className="folder-aside" aria-label="Folder details">
      <Artwork artwork={folder.artwork} full size={240} radius={5} title={folder.name}>
        {folder.artwork && (
          <span className="chip art-chip">
            {cover ? cover.name : folder.artwork.source === 'embedded' ? 'embedded' : 'cover'} · {folder.artwork.width}
            {folder.artwork.width === folder.artwork.height ? '²' : `×${folder.artwork.height}`}
          </span>
        )}
      </Artwork>
      <div className="flex flex-col gap-[3px]">
        <span className="el text-[14px] font-bold">{folder.name}</span>
        <span className="t3 text-[12px]">
          {folder.albumCount === 1 ? 'Album folder · matched to 1 album' : folder.albumCount > 1 ? `Holds ${folder.albumCount} albums` : 'Folder'}
        </span>
      </div>
      <div className="grid grid-cols-3 gap-1.5">
        <Stat value={count(n)} label="tracks" />
        <Stat value={count(n + folder.otherFiles.length)} label="files" />
        <Stat value={size >= 1e9 ? (size / 1e9).toFixed(2) : (size / 1e6).toFixed(0)} label={size >= 1e9 ? 'GB' : 'MB'} />
      </div>
      <div className="mono grid grid-cols-[auto_1fr] gap-x-3 gap-y-[5px] text-[10.5px]">
        {formats && (
          <>
            <span className="t3">Audio</span>
            <span className="el">{formats}</span>
          </>
        )}
        {others && (
          <>
            <span className="t3">Other</span>
            <span className="el">{others}</span>
          </>
        )}
        {n > 0 && (
          <>
            <span className="t3">Duration</span>
            <span>{formatLength(duration)}</span>
            <span className="t3">Tags</span>
            <span style={{ color: consistent ? 'var(--ok)' : 'var(--warn)' }}>
              {consistent ? 'Consistent' : 'Incomplete'} · {folder.tagged}/{n}
            </span>
          </>
        )}
        <span className="t3">Scanned</span>
        <span>{folder.rootOnline ? formatAgo(folder.lastScanAt) : 'drive offline'}</span>
      </div>
      <div className="mt-auto flex flex-col gap-1.5">
        <button type="button" className="btn pri justify-center" disabled={n === 0 && folder.subfolders.length === 0} onClick={() => void playScope({ kind: 'folder', id: folder.id, recursive: true })}>
          <Icon name="play" size={15} filled />
          Play folder
        </button>
        <div className="flex gap-1.5">
          <button type="button" className="btn flex-1 justify-center text-[12px]" style={{ fontWeight: 500 }} onClick={() => void playScope({ kind: 'folder', id: folder.id, recursive: true }, 'append')}>
            Add to queue
          </button>
          <button
            type="button"
            className="btn flex-1 justify-center text-[12px]"
            style={{ fontWeight: 500 }}
            onClick={() => {
              void run(libraryApi.rescan(folder.rootId));
              showToast({ tone: 'info', message: 'Rescanning this library folder' });
            }}
          >
            Rescan
          </button>
        </div>
        <button type="button" className="btn gh justify-center text-[12px]" style={{ fontWeight: 500 }} onClick={() => void run(libraryApi.revealFolder(folder.id))}>
          <Icon name="open" size={14} />
          Open in file manager
        </button>
      </div>
    </aside>
  );
}

function Stat({ value, label }: { value: string; label: string }) {
  return (
    <div className="stat">
      <span className="mono text-[16px] font-semibold">{value}</span>
      <span className="t3 text-[10.5px]">{label}</span>
    </div>
  );
}

export function FoldersView({ id }: { id?: number }) {
  const roots = useLibrary((s) => s.roots);
  const connected = useLibrary((s) => s.connected);
  const showFiles = useViewPrefs((s) => s.folderShowFiles);
  const selected = id ?? roots.find((r) => r.folderId != null)?.folderId ?? null;
  const { data: folder, error } = useLibraryQuery(selected != null ? `folder:${selected}` : null, () => libraryApi.folder(selected!));
  const crumbIds = useMemo(() => (folder ? folder.crumbs.map((c) => c.id) : []), [folder]);

  if (connected && roots.length === 0) {
    return (
      <div className="view">
        <LibraryWelcome />
      </div>
    );
  }

  const copy = () => {
    if (!folder) return;
    void navigator.clipboard
      .writeText(folder.displayPath)
      .then(() => showToast({ tone: 'info', message: 'Path copied' }))
      .catch(() => showToast({ tone: 'error', message: 'Could not copy the path' }));
  };

  return (
    <div className="view">
      <div className="folder-bar">
        <button type="button" className="ib" style={{ width: 28, height: 28 }} aria-label="Parent folder" disabled={!folder?.parentId} onClick={() => folder?.parentId && navigate('folders', folder.parentId)}>
          <Icon name="up" size={16} />
        </button>
        <div className="crumbs mono">
          <Icon name="home" size={15} className="t3 mx-1 flex-none" />
          {folder &&
            [...folder.crumbs, { id: folder.id, name: folder.name }].map((c, i, all) => (
              <span key={c.id} className="flex min-w-0 items-center">
                {i > 0 && <span className="t3">/</span>}
                <button type="button" className={i === all.length - 1 ? 'crumb here' : 'crumb'} onClick={() => navigate('folders', c.id)}>
                  {i === 0 ? home(c.name) : c.name}
                </button>
              </span>
            ))}
          <button type="button" className="ib ml-auto" style={{ width: 26, height: 26 }} aria-label="Copy path" data-tip="Copy path" onClick={copy}>
            <Icon name="copy" size={14} />
          </button>
        </div>
        <label className="t2 ml-1.5 flex items-center gap-2 text-[12px]">
          <Toggle small on={showFiles} onChange={(on) => setViewPrefs({ folderShowFiles: on })} label="Show non-audio files" />
          All files
        </label>
      </div>
      <ScanBanner />
      <div className="flex min-h-0 flex-1">
        <FolderTree roots={roots} selected={selected} crumbs={crumbIds} />
        {error ? (
          <div className="flex flex-1 items-center justify-center">
            <EmptyCard icon="folder" title="Folder not found">It is no longer part of your library.</EmptyCard>
          </div>
        ) : folder ? (
          <>
            <Contents folder={folder} showFiles={showFiles} />
            <Details folder={folder} />
          </>
        ) : (
          <div className="flex-1" />
        )}
      </div>
    </div>
  );
}
