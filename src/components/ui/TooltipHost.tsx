import { useEffect, useLayoutEffect, useRef, useState } from 'react';

const DELAY_MS = 500;
const GAP = 8;
const EDGE = 8;

interface Tip {
  text: string;
  shortcut?: string;
  anchor: DOMRect;
}

/**
 * One tooltip for the whole app (Components · B7): any element with
 * `data-tip` (and optionally `data-tip-key`) gets a styled tooltip after a
 * 500 ms hover. Delegated, so controls need no wrapper component.
 */
export function TooltipHost() {
  const [tip, setTip] = useState<Tip | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let timer = 0;
    let current: HTMLElement | null = null;
    const hide = () => {
      window.clearTimeout(timer);
      current = null;
      setTip(null);
    };
    const onOver = (e: PointerEvent) => {
      const el = (e.target as Element | null)?.closest<HTMLElement>('[data-tip]') ?? null;
      if (el === current) return;
      hide();
      current = el;
      if (!el?.dataset.tip) return;
      timer = window.setTimeout(() => {
        setTip({ text: el.dataset.tip!, shortcut: el.dataset.tipKey, anchor: el.getBoundingClientRect() });
      }, DELAY_MS);
    };
    document.addEventListener('pointerover', onOver);
    document.addEventListener('pointerdown', hide, true);
    document.addEventListener('keydown', hide, true);
    document.addEventListener('wheel', hide, true);
    window.addEventListener('blur', hide);
    return () => {
      hide();
      document.removeEventListener('pointerover', onOver);
      document.removeEventListener('pointerdown', hide, true);
      document.removeEventListener('keydown', hide, true);
      document.removeEventListener('wheel', hide, true);
      window.removeEventListener('blur', hide);
    };
  }, []);

  // Measure, then place above the anchor, or below when there is no room.
  useLayoutEffect(() => {
    if (!tip || !ref.current) {
      setPos(null);
      return;
    }
    const { width, height } = ref.current.getBoundingClientRect();
    const { anchor } = tip;
    const above = anchor.top - GAP - height;
    const top = above >= EDGE ? above : anchor.bottom + GAP;
    const centred = anchor.left + anchor.width / 2 - width / 2;
    const left = Math.min(Math.max(EDGE, centred), window.innerWidth - width - EDGE);
    setPos({ left, top });
  }, [tip]);

  if (!tip) return null;
  return (
    <div
      ref={ref}
      className="tooltip"
      role="tooltip"
      style={pos ? { left: pos.left, top: pos.top } : { left: -9999, top: -9999 }}
    >
      {tip.text}
      {tip.shortcut && <span className="kbd">{tip.shortcut}</span>}
    </div>
  );
}
