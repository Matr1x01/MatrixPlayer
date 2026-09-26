import { useCallback, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';

export interface DragState {
  /** Index of the row being dragged. */
  from: number;
  /** Gap it would drop into: 0 = before the first row, n = after the last. */
  gap: number;
  /** Pointer travel since the drag started, in px. */
  dy: number;
  /** The dragged row's box, relative to the list container. */
  top: number;
  height: number;
  /** Where the drop line goes, relative to the list container. */
  lineY: number;
}

/**
 * Pointer-driven reordering for a vertical list (Queue board: the dragged
 * row lifts and follows the pointer, a dashed ghost keeps its place, an
 * accent line marks where it will land). Rows are measured when the drag
 * starts, so rows of any height and headers between them are fine.
 *
 * `rowSelector` finds the rows inside the container, in list order.
 */
export function useDragReorder(onDrop: (from: number, gap: number) => void, rowSelector = '[data-reorder]') {
  const container = useRef<HTMLDivElement>(null);
  const [drag, setDragState] = useState<DragState | null>(null);
  // Mirrors `drag` for the pointer-up handler, which must act exactly once
  // (state updaters can run twice in development).
  const current = useRef<DragState | null>(null);
  const setDrag = useCallback((next: DragState | null | ((d: DragState | null) => DragState | null)) => {
    current.current = typeof next === 'function' ? next(current.current) : next;
    setDragState(current.current);
  }, []);
  const boxes = useRef<{ top: number; height: number }[]>([]);
  const start = useRef(0);

  const onGripDown = useCallback(
    (index: number, e: ReactPointerEvent) => {
      if (e.button !== 0 || !container.current) return;
      e.preventDefault();
      const base = container.current.getBoundingClientRect().top;
      boxes.current = [...container.current.querySelectorAll<HTMLElement>(rowSelector)].map((el) => {
        const r = el.getBoundingClientRect();
        return { top: r.top - base, height: r.height };
      });
      const box = boxes.current[index];
      if (!box) return;
      start.current = e.clientY;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
      setDrag({ from: index, gap: index, dy: 0, top: box.top, height: box.height, lineY: box.top });
    },
    [rowSelector, setDrag],
  );

  const onGripMove = useCallback((e: ReactPointerEvent) => {
    setDrag((d) => {
      if (!d) return d;
      const dy = e.clientY - start.current;
      const center = d.top + d.height / 2 + dy;
      const list = boxes.current;
      let gap = 0;
      while (gap < list.length && list[gap]!.top + list[gap]!.height / 2 < center) gap++;
      const lineY = gap < list.length ? list[gap]!.top - 1 : (list.at(-1)?.top ?? 0) + (list.at(-1)?.height ?? 0) + 1;
      return { ...d, dy, gap, lineY };
    });
  }, [setDrag]);

  const onGripUp = useCallback(
    (e: ReactPointerEvent) => {
      (e.target as HTMLElement).releasePointerCapture?.(e.pointerId);
      const d = current.current;
      setDrag(null);
      if (d && d.gap !== d.from && d.gap !== d.from + 1) onDrop(d.from, d.gap);
    },
    [onDrop, setDrag],
  );

  const cancel = useCallback(() => setDrag(null), [setDrag]);

  return {
    container,
    drag,
    /** Spread onto the grip of row `index`. */
    grip: (index: number) => ({
      onPointerDown: (e: ReactPointerEvent) => onGripDown(index, e),
      onPointerMove: onGripMove,
      onPointerUp: onGripUp,
      onPointerCancel: cancel,
    }),
  };
}

/** Turns a drop gap into the id to insert before (null = at the end). */
export function beforeId<T extends { id: number }>(list: T[], gap: number, after?: T | null): number | null {
  return list[gap]?.id ?? after?.id ?? null;
}
