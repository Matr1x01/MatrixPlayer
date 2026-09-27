// The track context menu (Songs table board) and the actions behind it,
// shared by every list of library tracks.

import { askChoice, askText } from '@/components/ui/Dialog';
import type { MenuHeader, MenuItem } from '@/components/ui/ContextMenu';
import { libraryApi, playlistApi } from '@/services/ipc';
import { setFavorite, useLibrary } from '@/stores/library';
import { navigate } from '@/stores/navigation';
import { run, showToast } from '@/stores/toasts';
import type { TrackRow } from '@/types/library';

import { playTracks } from './actions';
import { formatLength, formatSize, plural } from './format';
import { showTrackInfo } from './TrackInfoDialog';

export async function addToNewPlaylist(ids: number[]) {
  const name = await askText({ title: 'New playlist', label: 'Name', placeholder: 'Late-night lossless', confirm: 'Create' });
  if (!name) return;
  const id = await run(playlistApi.create(name, ids));
  if (id != null) showToast({ tone: 'info', message: ids.length ? `Created “${name}” with ${plural(ids.length, 'track')}` : `Created “${name}”` });
}

export async function addToPlaylist(playlistId: number, name: string, ids: number[]) {
  // Adding what is already there is usually a slip: ask before doubling up.
  const existing = await run(libraryApi.trackList({ scope: { kind: 'playlist', id: playlistId } }));
  if (!existing) return;
  const present = new Set(existing.ids);
  const fresh = ids.filter((id) => !present.has(id));
  let chosen = ids;
  if (fresh.length < ids.length) {
    const already = ids.length - fresh.length;
    const choice = await askChoice({
      title: fresh.length === 0 ? `Already in “${name}”` : `Some are already in “${name}”`,
      body:
        fresh.length === 0
          ? ids.length === 1
            ? 'This track is already in the playlist. Add it again?'
            : `All ${ids.length} tracks are already in the playlist. Add them again?`
          : `${already} of these ${ids.length} tracks ${already === 1 ? 'is' : 'are'} already in the playlist.`,
      choices:
        fresh.length === 0
          ? [{ label: 'Add again', value: 'all' }]
          : [
              { label: 'Add all', value: 'all' },
              { label: `Add ${plural(fresh.length, 'new track')}`, value: 'new' },
            ],
    });
    if (choice == null) return;
    chosen = choice === 'all' ? ids : fresh;
  }
  const added = await run(playlistApi.add(playlistId, chosen));
  if (added) showToast({ tone: 'info', message: `Added ${plural(added, 'track')} to “${name}”` });
}

/** "Add to playlist" entries; `tracks` may be resolved only when chosen. */
export function playlistSubmenu(tracks: number[] | (() => Promise<number[]>)): MenuItem[] {
  const playlists = useLibrary.getState().playlists;
  const resolve = typeof tracks === 'function' ? tracks : () => Promise.resolve(tracks);
  return [
    { label: 'New playlist…', icon: 'plus', shortcut: 'Ctrl+N', onSelect: () => void resolve().then(addToNewPlaylist) },
    ...(playlists.length ? (['separator'] as MenuItem[]) : []),
    ...playlists.map(
      (p): MenuItem => ({
        label: p.name,
        artworkId: p.artworkId,
        shortcut: String(p.trackCount),
        onSelect: () => void resolve().then((ids) => addToPlaylist(p.id, p.name, ids)),
      }),
    ),
  ];
}

interface TrackMenuOptions {
  /** Extra items placed after the queue actions (e.g. "Remove from playlist"). */
  extra?: MenuItem[];
  /** What "Play" does, when it should play more than the chosen tracks
   * (a list plays from the chosen track on, as Enter does). */
  play?: () => void;
}

/** Menu for a selection of tracks; `rows` are the selected rows that are loaded. */
export function trackMenu(ids: number[], rows: TrackRow[], options: TrackMenuOptions = {}): { items: MenuItem[]; header?: MenuHeader } {
  const favorites = useLibrary.getState().favorites;
  const allFavorite = ids.every((id) => favorites.has(id));
  const single = rows.length === 1 ? rows[0] : undefined;
  const albums = new Set(rows.map((r) => r.albumId));
  const firstAlbum = rows[0]?.album;
  const header: MenuHeader | undefined =
    ids.length > 1
      ? {
          title: albums.size === 1 && firstAlbum ? `${plural(ids.length, 'track')} · ${firstAlbum}` : plural(ids.length, 'track'),
          meta: `${formatLength(rows.reduce((t, r) => t + r.durationMs, 0))} · ${formatSize(rows.reduce((t, r) => t + r.fileSize, 0))}`,
          artworkId: rows.find((r) => r.artworkId)?.artworkId ?? null,
        }
      : undefined;

  const items: MenuItem[] = [
    { label: 'Play', icon: 'play', filledIcon: true, shortcut: 'Enter', onSelect: () => (options.play ? options.play() : void playTracks(ids)) },
    { label: 'Play next', icon: 'playNext', shortcut: 'Shift+Enter', onSelect: () => void playTracks(ids, 'next', undefined, single?.title) },
    { label: 'Add to queue', icon: 'queue', shortcut: 'Q', onSelect: () => void playTracks(ids, 'append', undefined, single?.title) },
    ...(options.extra ?? []),
    'separator',
    { label: 'Add to playlist', icon: 'playlist', submenu: playlistSubmenu(ids) },
    {
      label: allFavorite ? 'Remove from favorites' : 'Add to favorites',
      icon: 'heart',
      filledIcon: allFavorite,
      shortcut: 'L',
      onSelect: () => void setFavorite(ids, !allFavorite),
    },
    'separator',
    {
      label: 'Open album',
      icon: 'album',
      shortcut: 'Alt+A',
      disabled: single?.albumId == null && !(albums.size === 1 && rows[0]?.albumId != null),
      onSelect: () => rows[0]?.albumId != null && navigate('album', rows[0].albumId),
    },
    {
      label: 'Open artist',
      icon: 'artist',
      shortcut: 'Alt+R',
      disabled: rows[0]?.artistId == null,
      onSelect: () => rows[0]?.artistId != null && navigate('artists', rows[0].artistId),
    },
    'separator',
    {
      label: 'File information',
      icon: 'info',
      shortcut: 'Ctrl+I',
      disabled: !single,
      onSelect: () => single && void showTrackInfo(single),
    },
    {
      label: 'Browse folder',
      icon: 'folder',
      shortcut: 'Ctrl+Shift+O',
      disabled: !rows[0],
      onSelect: () => rows[0] && navigate('folders', rows[0].folderId),
    },
  ];
  return { items, header };
}
