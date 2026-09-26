import { useCallback, useEffect, useRef, useState } from 'react';

import { libraryApi } from '@/services/ipc';
import { useLibrary } from '@/stores/library';
import type { TrackRow } from '@/types/library';

const PAGE = 120;

/**
 * Loads rows for a (possibly huge) ordered list of track ids a page at a
 * time, as the list scrolls. Rows already shown stay visible while a
 * library change refreshes them, so the list never flashes empty.
 */
export function useTrackRows(ids: number[]) {
  const version = useLibrary((s) => `${s.versions.tracks}.${s.versions.history}.${s.versions.favorites}`);
  const [rows, setRows] = useState<ReadonlyMap<number, TrackRow>>(new Map());
  const requested = useRef(new Set<number>());
  const idsRef = useRef(ids);
  idsRef.current = ids;

  // New ids or new data: pages must be fetched again (old rows stay meanwhile).
  useEffect(() => {
    requested.current = new Set();
  }, [ids, version]);

  const ensure = useCallback(
    (start: number, end: number) => {
      const list = idsRef.current;
      for (let page = Math.floor(start / PAGE); page * PAGE < Math.min(end, list.length); page++) {
        if (requested.current.has(page)) continue;
        requested.current.add(page);
        const slice = list.slice(page * PAGE, (page + 1) * PAGE);
        void libraryApi
          .tracks(slice)
          .then((fetched) =>
            setRows((prev) => {
              const next = new Map(prev);
              for (const row of fetched) next.set(row.id, row);
              return next;
            }),
          )
          .catch(() => requested.current.delete(page));
      }
    },
    // `version` resets what counts as requested; `ensure` itself reads refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [ids, version],
  );

  return { rows, ensure };
}
