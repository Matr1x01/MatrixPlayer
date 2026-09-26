import { useEffect, useRef, useState } from 'react';

import { useLibrary } from '@/stores/library';
import { errorMessage } from '@/stores/toasts';
import type { LibraryChange } from '@/types/library';

interface Entry {
  data: unknown;
  version: string;
}

/** Results by key, so going back to a view shows it instantly. */
const cache = new Map<string, Entry>();
const CACHE_LIMIT = 60;

export interface QueryResult<T> {
  data: T | undefined;
  error: string | null;
  /** True until the first result for this key arrives. */
  loading: boolean;
  /** True while a newer result is being fetched behind shown data. */
  refreshing: boolean;
}

/**
 * Fetches library data for `key` and refetches when any of the `depends`
 * change kinds is bumped. Cached data for the key is shown immediately while
 * a fresh copy loads (stale-while-revalidate).
 */
export function useLibraryQuery<T>(
  key: string | null,
  fetch: () => Promise<T>,
  depends: LibraryChange[] = ['tracks'],
): QueryResult<T> {
  const version = useLibrary((s) => depends.map((d) => s.versions[d]).join('.'));
  const cached = key ? cache.get(key) : undefined;
  const [state, setState] = useState<{ key: string | null; data: T | undefined; error: string | null }>({
    key,
    data: cached?.data as T | undefined,
    error: null,
  });
  const [refreshing, setRefreshing] = useState(false);
  const fetchRef = useRef(fetch);
  fetchRef.current = fetch;

  useEffect(() => {
    if (!key) return;
    const hit = cache.get(key);
    if (hit) setState({ key, data: hit.data as T, error: null });
    if (hit?.version === version) return;
    let cancelled = false;
    setRefreshing(true);
    fetchRef
      .current()
      .then((data) => {
        cache.set(key, { data, version });
        if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value!);
        if (!cancelled) setState({ key, data, error: null });
      })
      .catch((err) => {
        if (!cancelled) setState((s) => ({ ...s, key, error: errorMessage(err) }));
      })
      .finally(() => {
        if (!cancelled) setRefreshing(false);
      });
    return () => {
      cancelled = true;
    };
  }, [key, version]);

  const current = state.key === key ? state : { data: cached?.data as T | undefined, error: null };
  return {
    data: current.data,
    error: current.error,
    loading: current.data === undefined && !current.error,
    refreshing,
  };
}

/** Forgets cached results, e.g. after the library was removed. */
export function clearLibraryCache() {
  cache.clear();
}
