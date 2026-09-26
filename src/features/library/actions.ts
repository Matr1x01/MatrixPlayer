// Playing and queueing library items. Everything is resolved to tracks in
// Rust; the UI only names what to play.

import { libraryApi } from '@/services/ipc';
import { run, showToast } from '@/stores/toasts';
import type { PlaySource, TrackQuery, TrackScope } from '@/types/library';
import type { InsertMode } from '@/types/player';

function report(count: number | undefined, mode: InsertMode) {
  if (!count || mode === 'replace') return;
  const noun = count === 1 ? 'track' : 'tracks';
  showToast({ tone: 'info', message: mode === 'next' ? `${count} ${noun} will play next` : `${count} ${noun} added to the queue` });
}

export async function playSource(source: PlaySource, mode: InsertMode = 'replace', start?: number, shuffle?: boolean) {
  report(await run(libraryApi.play(source, mode, start, shuffle)), mode);
}

export function playTracks(ids: number[], mode: InsertMode = 'replace', start?: number) {
  return playSource({ kind: 'tracks', ids }, mode, start);
}

export function playQuery(query: TrackQuery, mode: InsertMode = 'replace', shuffle?: boolean) {
  return playSource({ kind: 'query', query }, mode, undefined, shuffle);
}

export function playScope(scope: TrackScope, mode: InsertMode = 'replace', shuffle?: boolean) {
  return playQuery({ scope }, mode, shuffle);
}
