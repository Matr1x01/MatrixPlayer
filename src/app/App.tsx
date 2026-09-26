import { useEffect } from 'react';

import { useGlobalShortcuts } from '@/hooks/useGlobalShortcuts';
import { onCoreEvent } from '@/services/events';
import { connectLibrary } from '@/stores/library';
import { connectPlayer } from '@/stores/player';
import { loadPreferences, usePreferences } from '@/stores/preferences';
import { errorMessage, showToast } from '@/stores/toasts';

import { AppShell } from './AppShell';

/** Connects to the core, then renders the shell once preferences are known
 * (so the saved theme is applied from the first frame). */
export function App() {
  const loaded = usePreferences((s) => s.loaded);
  useGlobalShortcuts();

  useEffect(() => {
    let cancelled = false;
    const cleanups: (() => void)[] = [];
    const keep = (unlisten: () => void) => (cancelled ? unlisten() : cleanups.push(unlisten));

    void loadPreferences();
    connectPlayer()
      .then(keep)
      .catch((err) => showToast({ tone: 'error', message: `Could not reach the audio engine: ${errorMessage(err)}` }));
    connectLibrary()
      .then(keep)
      .catch((err) => showToast({ tone: 'error', message: `Could not open the library: ${errorMessage(err)}` }));
    void onCoreEvent('audio-error', (err) =>
      showToast({ tone: 'error', message: err.message, detail: err.fileName ?? undefined }),
    ).then(keep);
    void onCoreEvent('tracks-queued', ({ count }) =>
      showToast({ tone: 'info', message: `${count} ${count === 1 ? 'track' : 'tracks'} added to the queue` }),
    ).then(keep);

    return () => {
      cancelled = true;
      cleanups.forEach((unlisten) => unlisten());
    };
  }, []);

  return loaded ? <AppShell /> : null;
}
