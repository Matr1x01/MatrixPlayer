import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type ReactNode } from 'react';
import { create } from 'zustand';

import { Icon } from '@/components/icons/Icon';
import type { IconName } from '@/components/icons/paths';

import { Artwork } from './Artwork';

export type MenuItem =
  | {
      label: string;
      icon?: IconName;
      filledIcon?: boolean;
      /** Artwork shown in place of an icon (playlists in "Add to playlist"). */
      artworkId?: string | null;
      shortcut?: string;
      danger?: boolean;
      disabled?: boolean;
      onSelect?: () => void;
      submenu?: MenuItem[];
    }
  | 'separator';

export interface MenuHeader {
  title: string;
  meta?: string;
  artworkId?: string | null;
}

interface OpenMenu {
  x: number;
  y: number;
  items: MenuItem[];
  header?: MenuHeader;
  label: string;
}

const useMenu = create<{ menu: OpenMenu | null }>()(() => ({ menu: null }));

/** Opens a menu at the pointer (or under an element). */
export function openMenu(at: { x: number; y: number } | HTMLElement, items: MenuItem[], options: { header?: MenuHeader; label?: string } = {}) {
  const point =
    at instanceof HTMLElement
      ? (() => {
          const r = at.getBoundingClientRect();
          return { x: r.left, y: r.bottom + 4 };
        })()
      : at;
  useMenu.setState({ menu: { ...point, items, header: options.header, label: options.label ?? 'Actions' } });
}

export function closeMenu() {
  useMenu.setState({ menu: null });
}

/** `onContextMenu` handler that opens `items`. */
export function contextMenu(items: () => MenuItem[], options?: { header?: MenuHeader; label?: string }) {
  return (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    openMenu({ x: e.clientX, y: e.clientY }, items(), options);
  };
}

const MARGIN = 8;

function MenuPanel({
  items,
  header,
  x,
  y,
  label,
  depth,
  onClose,
  onDone,
}: {
  items: MenuItem[];
  header?: MenuHeader;
  x: number;
  y: number;
  label: string;
  depth: number;
  /** Closes this level (Escape, or ← in a submenu). */
  onClose: () => void;
  /** Closes the whole menu, once an item has been chosen. */
  onDone: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y, ready: false });
  const [active, setActive] = useState(-1);
  const [sub, setSub] = useState<{ index: number; x: number; y: number } | null>(null);
  const hoverTimer = useRef<number | undefined>(undefined);

  // Keep the panel on screen: flip left/up when it would overflow.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    let left = x;
    let top = y;
    if (left + width > window.innerWidth - MARGIN) left = depth > 0 ? x - width * 2 - 4 : window.innerWidth - width - MARGIN;
    if (top + height > window.innerHeight - MARGIN) top = Math.max(MARGIN, window.innerHeight - height - MARGIN);
    setPos({ left: Math.max(MARGIN, left), top, ready: true });
  }, [x, y, depth]);

  useEffect(() => {
    if (depth === 0) ref.current?.focus();
  }, [depth]);

  const selectable = items.map((item, i) => (item !== 'separator' && !item.disabled ? i : -1)).filter((i) => i >= 0);

  const openSub = (index: number) => {
    const el = ref.current?.querySelector<HTMLElement>(`[data-index="${index}"]`);
    if (!el) return;
    const r = el.getBoundingClientRect();
    setSub({ index, x: r.right + 2, y: r.top - 5 });
  };

  const activate = (index: number) => {
    const item = items[index];
    if (!item || item === 'separator' || item.disabled) return;
    if (item.submenu) {
      openSub(index);
      return;
    }
    // Choosing an item ends the whole menu, not just this submenu.
    onDone();
    item.onSelect?.();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const at = selectable.indexOf(active);
    switch (e.key) {
      case 'ArrowDown':
        setActive(selectable[(at + 1) % selectable.length] ?? -1);
        break;
      case 'ArrowUp':
        setActive(selectable[(at - 1 + selectable.length) % selectable.length] ?? -1);
        break;
      case 'ArrowRight':
        if (active >= 0) openSub(active);
        break;
      case 'ArrowLeft':
        if (depth > 0) onClose();
        break;
      case 'Enter':
      case ' ':
        activate(active);
        break;
      case 'Escape':
        onClose();
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  const subItem = sub ? items[sub.index] : undefined;
  return (
    <>
      <div
        ref={ref}
        className={pos.ready ? 'menu menu-open' : 'menu'}
        style={{ left: pos.left, top: pos.top, visibility: pos.ready ? 'visible' : 'hidden' }}
        role="menu"
        aria-label={label}
        tabIndex={-1}
        onKeyDown={onKeyDown}
        onContextMenu={(e) => e.preventDefault()}
      >
        {header && (
          <div className="menu-head">
            {header.artworkId !== undefined && <Artwork artworkId={header.artworkId} size={22} radius={2} title={header.title} />}
            <span className="flex min-w-0 flex-col">
              <span className="el text-[12px] font-semibold">{header.title}</span>
              {header.meta && <span className="mono t3 text-[9.5px]">{header.meta}</span>}
            </span>
          </div>
        )}
        {items.map((item, i) =>
          item === 'separator' ? (
            <div key={`sep-${i}`} className="msep" />
          ) : (
            <button
              key={`${item.label}-${i}`}
              type="button"
              role="menuitem"
              data-index={i}
              aria-haspopup={item.submenu ? 'true' : undefined}
              aria-expanded={item.submenu ? sub?.index === i : undefined}
              disabled={item.disabled}
              className={['mi', item.danger && 'dg', (active === i || sub?.index === i) && 'hov'].filter(Boolean).join(' ')}
              onMouseEnter={() => {
                setActive(i);
                window.clearTimeout(hoverTimer.current);
                hoverTimer.current = window.setTimeout(() => (item.submenu ? openSub(i) : setSub(null)), 120);
              }}
              onClick={() => activate(i)}
            >
              {item.artworkId !== undefined ? (
                <Artwork artworkId={item.artworkId} size={16} radius={2} title={item.label} />
              ) : item.icon ? (
                <Icon name={item.icon} filled={item.filledIcon} />
              ) : (
                <span className="w-[15px] flex-none" />
              )}
              <span className="el">{item.label}</span>
              {item.submenu ? (
                <Icon name="chevR" size={13} style={{ marginLeft: 'auto' }} />
              ) : (
                item.shortcut && <span className="sc">{item.shortcut}</span>
              )}
            </button>
          ),
        )}
      </div>
      {sub && subItem && subItem !== 'separator' && subItem.submenu && (
        <MenuPanel
          items={subItem.submenu}
          x={sub.x}
          y={sub.y}
          label={subItem.label}
          depth={depth + 1}
          onClose={() => {
            setSub(null);
            ref.current?.focus();
          }}
          onDone={onDone}
        />
      )}
    </>
  );
}

/** Renders the open menu, if any, and closes it on outside interaction. */
export function ContextMenuHost(): ReactNode {
  const menu = useMenu((s) => s.menu);
  useEffect(() => {
    if (!menu) return;
    const onDown = (e: PointerEvent) => {
      if (!(e.target as HTMLElement).closest('.menu')) closeMenu();
    };
    const onBlur = () => closeMenu();
    window.addEventListener('pointerdown', onDown, true);
    window.addEventListener('blur', onBlur);
    window.addEventListener('resize', onBlur);
    return () => {
      window.removeEventListener('pointerdown', onDown, true);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('resize', onBlur);
    };
  }, [menu]);
  if (!menu) return null;
  return <MenuPanel key={`${menu.x}:${menu.y}`} {...menu} depth={0} onClose={closeMenu} onDone={closeMenu} />;
}
