import { queueApi } from '@/services/ipc';
import { run, showToast } from '@/stores/toasts';
import type { InsertMode } from '@/types/player';

function report(count: number | undefined, mode: InsertMode) {
  if (!count) return;
  const noun = count === 1 ? 'track' : 'tracks';
  showToast({ tone: 'info', message: mode === 'replace' ? `Playing ${count} ${noun}` : `${count} ${noun} added to the queue` });
}

/** Native file picker → queue. Files never pass through the frontend. */
export async function openFiles(mode: InsertMode = 'replace') {
  report(await run(queueApi.openFiles(mode)), mode);
}

export async function openFolder(mode: InsertMode = 'replace') {
  report(await run(queueApi.openFolder(mode)), mode);
}
