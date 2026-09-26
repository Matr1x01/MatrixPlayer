import { useEffect, useRef, useState, type ReactNode } from 'react';

import { Icon } from '@/components/icons/Icon';
import { openMenu } from '@/components/ui/ContextMenu';

interface FilterFieldProps {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  width?: number;
  /** Focus with "/" (the design's hint). */
  slashFocus?: boolean;
}

/** The design's filter field (`.fld`) as a real input, debounced by the caller. */
export function FilterField({ value, onChange, placeholder, width = 210, slashFocus = true }: FilterFieldProps) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!slashFocus) return;
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (e.key === '/' && !target.closest('input, textarea') && !document.querySelector('.dlg-scrim, .search-scrim')) {
        e.preventDefault();
        ref.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [slashFocus]);
  return (
    <label className="fld fld-live" style={{ width }}>
      <Icon name="filter" size={15} />
      <input
        ref={ref}
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            if (value) onChange('');
            else ref.current?.blur();
            e.stopPropagation();
          }
        }}
      />
      {value ? (
        <button type="button" className="fld-clear" aria-label="Clear filter" onClick={() => onChange('')}>
          <Icon name="x" size={13} />
        </button>
      ) : (
        slashFocus && <span className="kbd">/</span>
      )}
    </label>
  );
}

/** A value that follows `value` after it stops changing for `ms`. */
export function useDebounced<T>(value: T, ms = 160): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(value), ms);
    return () => window.clearTimeout(timer);
  }, [value, ms]);
  return debounced;
}

interface SortButtonProps<T extends string> {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}

/** "Artist, then year ▾": opens a menu of sort orders. */
export function SortButton<T extends string>({ value, options, onChange }: SortButtonProps<T>) {
  const current = options.find((o) => o.value === value) ?? options[0]!;
  return (
    <button
      type="button"
      className="btn"
      style={{ fontWeight: 500 }}
      aria-haspopup="menu"
      onClick={(e) =>
        openMenu(
          e.currentTarget,
          options.map((o) => ({ label: o.label, icon: o.value === value ? 'check' : undefined, onSelect: () => onChange(o.value) })),
          { label: 'Sort by' },
        )
      }
    >
      <Icon name="sort" size={15} className="t3" />
      {current.label}
      <Icon name="chevD" size={14} className="t3" />
    </button>
  );
}

interface PillProps {
  on: boolean;
  onClick: () => void;
  children: ReactNode;
  n?: number;
}

export function Pill({ on, onClick, children, n }: PillProps) {
  return (
    <button type="button" className={on ? 'pill on' : 'pill'} aria-pressed={on} onClick={onClick}>
      {children}
      {n != null && <span className="n">{n.toLocaleString('en-US')}</span>}
    </button>
  );
}
