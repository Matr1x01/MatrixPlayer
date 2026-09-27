// Playing and queueing library items. Everything is resolved to tracks in
// Rust; the UI only names what to play.

import { libraryApi, queueApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import { run, showToast } from '@/stores/toasts';
import type { PlaySource, TrackQuery, TrackScope } from '@/types/library';
import type { InsertMode } from '@/types/player';

/** Confirms a queue change; `label` names what was queued (a title, an album). */
function report(count: number | undefined, mode: InsertMode, label?: string) {
  if (!count || mode === 'replace') return;
  const noun = count === 1 ? 'track' : 'tracks';
  const what = label ? (count === 1 ? `“${label}”` : `“${label}” · ${count} ${noun}`) : `${count} ${noun}`;
  showToast({ tone: 'info', message: `${what} ${mode === 'next' ? 'will play next' : 'added to the queue'}` });
}

/** Tracks the user queued by hand that have not played yet: replacing the
 * queue would throw these away, so that is offered as undoable. */
function queuedByHand(): number {
  const { entries, currentIndex } = usePlayer.getState().queue;
  return entries.slice((currentIndex ?? -1) + 1).filter((e) => e.added).length;
}

export async function playSource(source: PlaySource, mode: InsertMode = 'replace', start?: number, shuffle?: boolean, label?: string) {
  const discarded = mode === 'replace' ? queuedByHand() : 0;
  const count = await run(libraryApi.play(source, mode, start, shuffle));
  if (count && discarded > 0) {
    showToast({
      tone: 'info',
      message: `Queue replaced · ${discarded} ${discarded === 1 ? 'track' : 'tracks'} you added removed`,
      action: { label: 'Undo', run: () => void run(queueApi.undo()) },
    });
  }
  report(count, mode, label);
}

export function playTracks(ids: number[], mode: InsertMode = 'replace', start?: number, label?: string) {
  return playSource({ kind: 'tracks', ids }, mode, start, undefined, label);
}

export function playQuery(query: TrackQuery, mode: InsertMode = 'replace', shuffle?: boolean, label?: string) {
  return playSource({ kind: 'query', query }, mode, undefined, shuffle, label);
}

export function playScope(scope: TrackScope, mode: InsertMode = 'replace', shuffle?: boolean, label?: string) {
  return playQuery({ scope }, mode, shuffle, label);
}

/** Empties the queue, offering to put it back. */
export async function clearQueue() {
  const count = usePlayer.getState().queue.entries.length;
  if (count === 0) return;
  await run(queueApi.clear());
  showToast({
    tone: 'info',
    message: `Queue cleared · ${count} ${count === 1 ? 'track' : 'tracks'}`,
    action: { label: 'Undo', run: () => void run(queueApi.undo()) },
  });
}
