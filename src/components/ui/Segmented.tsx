import type { ReactNode } from 'react';

export interface SegmentOption<T extends string> {
  value: T;
  label: ReactNode;
  /** Accessible name when the label is an icon. */
  ariaLabel?: string;
  disabled?: boolean;
  tip?: string;
}

interface SegmentedProps<T extends string> {
  options: SegmentOption<T>[];
  value: T;
  onChange: (value: T) => void;
  label: string;
  size?: 'sm' | 'lg';
  className?: string;
}

/** Mutually exclusive modes, 2–5 options (Components · B4). */
export function Segmented<T extends string>({ options, value, onChange, label, size, className }: SegmentedProps<T>) {
  const classes = ['seg', size, className].filter(Boolean).join(' ');
  return (
    <div className={classes} role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          aria-label={option.ariaLabel}
          data-tip={option.tip}
          className={option.value === value ? 'on' : undefined}
          disabled={option.disabled}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
