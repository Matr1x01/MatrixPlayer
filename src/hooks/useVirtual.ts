import { useCallback, useLayoutEffect, useMemo, useRef, useState } from 'react';

/** Tracks a scroll container's scroll position and size. */
function useViewport<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [viewport, setViewport] = useState({ top: 0, height: 0, width: 0 });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    let frame = 0;
    const measure = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() =>
        setViewport({ top: el.scrollTop, height: el.clientHeight, width: el.clientWidth }),
      );
    };
    setViewport({ top: el.scrollTop, height: el.clientHeight, width: el.clientWidth });
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    el.addEventListener('scroll', measure, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      el.removeEventListener('scroll', measure);
    };
  }, []);

  return { ref, viewport };
}

export interface VirtualList<T extends HTMLElement> {
  ref: React.RefObject<T | null>;
  start: number;
  end: number;
  /** Space above the first rendered row. */
  offset: number;
  totalHeight: number;
  scrollToIndex: (index: number, align?: 'nearest' | 'start') => void;
}

/**
 * Renders only the rows of a long list that are near the viewport. Rows
 * have a fixed height, which keeps scrolling through 100,000 tracks as cheap
 * as through ten.
 */
export function useVirtualList<T extends HTMLElement = HTMLDivElement>(
  count: number,
  rowHeight: number,
  overscan = 10,
  /** Height of content above the rows inside the scroll container. */
  headerHeight = 0,
): VirtualList<T> {
  const { ref, viewport } = useViewport<T>();
  const top = Math.max(0, viewport.top - headerHeight);
  const height = viewport.height || 900;
  const start = Math.max(0, Math.floor(top / rowHeight) - overscan);
  const end = Math.min(count, Math.ceil((top + height) / rowHeight) + overscan);

  const scrollToIndex = useCallback(
    (index: number, align: 'nearest' | 'start' = 'nearest') => {
      const el = ref.current;
      if (!el) return;
      const rowTop = headerHeight + index * rowHeight;
      if (align === 'start') el.scrollTop = rowTop;
      else if (rowTop < el.scrollTop + headerHeight) el.scrollTop = rowTop - headerHeight;
      else if (rowTop + rowHeight > el.scrollTop + el.clientHeight) el.scrollTop = rowTop + rowHeight - el.clientHeight;
    },
    [ref, rowHeight, headerHeight],
  );

  return { ref, start, end, offset: start * rowHeight, totalHeight: count * rowHeight, scrollToIndex };
}

export interface VirtualGrid<T extends HTMLElement> {
  ref: React.RefObject<T | null>;
  columns: number;
  tileWidth: number;
  rowHeight: number;
  /** Item indexes to render. */
  start: number;
  end: number;
  offset: number;
  totalHeight: number;
  scrollToIndex: (index: number) => void;
}

/**
 * A responsive grid of square tiles with a caption below, rendering only the
 * rows near the viewport. Columns follow the container width and the chosen
 * tile size, as the Albums board's size slider does.
 */
export function useVirtualGrid<T extends HTMLElement = HTMLDivElement>(
  count: number,
  options: { minTile: number; gap: number; caption: number; rowGap: number; paddingX: number; paddingTop: number },
): VirtualGrid<T> {
  const { ref, viewport } = useViewport<T>();
  const inner = Math.max(0, viewport.width - options.paddingX);
  const columns = Math.max(1, Math.floor((inner + options.gap) / (options.minTile + options.gap)));
  const tileWidth = columns > 0 ? (inner - options.gap * (columns - 1)) / columns : options.minTile;
  const rowHeight = tileWidth + options.caption + options.rowGap;
  const rows = Math.ceil(count / columns);
  const top = Math.max(0, viewport.top - options.paddingTop);
  const height = viewport.height || 900;
  const firstRow = Math.max(0, Math.floor(top / rowHeight) - 2);
  const lastRow = Math.min(rows, Math.ceil((top + height) / rowHeight) + 2);

  const scrollToIndex = useCallback(
    (index: number) => {
      const el = ref.current;
      if (el) el.scrollTop = options.paddingTop + Math.floor(index / columns) * rowHeight - 8;
    },
    [ref, columns, rowHeight, options.paddingTop],
  );

  return {
    ref,
    columns,
    tileWidth,
    rowHeight,
    start: firstRow * columns,
    end: Math.min(count, lastRow * columns),
    offset: firstRow * rowHeight,
    totalHeight: rows * rowHeight,
    scrollToIndex,
  };
}

/** Like [`useVirtualList`], for rows of differing (known) heights. */
export function useVariableList<T extends HTMLElement = HTMLDivElement>(heights: number[], overscan = 6) {
  const { ref, viewport } = useViewport<T>();
  const offsets = useMemo(() => {
    const out = new Array<number>(heights.length + 1);
    out[0] = 0;
    for (let i = 0; i < heights.length; i++) out[i + 1] = out[i]! + heights[i]!;
    return out;
  }, [heights]);
  const find = (y: number) => {
    let lo = 0;
    let hi = heights.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (offsets[mid + 1]! <= y) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  };
  const height = viewport.height || 900;
  const start = Math.max(0, find(viewport.top) - overscan);
  const end = Math.min(heights.length, find(viewport.top + height) + overscan + 1);
  const scrollToIndex = useCallback(
    (index: number) => {
      const el = ref.current;
      if (!el) return;
      const top = offsets[index] ?? 0;
      const bottom = offsets[index + 1] ?? top;
      if (top < el.scrollTop) el.scrollTop = top;
      else if (bottom > el.scrollTop + el.clientHeight) el.scrollTop = bottom - el.clientHeight;
    },
    [ref, offsets],
  );
  return { ref, start, end, offsets, totalHeight: offsets[heights.length] ?? 0, scrollToIndex };
}
