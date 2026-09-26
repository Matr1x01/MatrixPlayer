import type { CSSProperties } from 'react';

import { ICONS, type IconName } from './paths';

interface IconProps {
  name: IconName;
  /** Rendered size in px (design default 18). */
  size?: number;
  /** Filled glyph: transport controls and an active favourite. */
  filled?: boolean;
  strokeWidth?: number;
  className?: string;
  style?: CSSProperties;
}

export function Icon({ name, size, filled, strokeWidth, className, style }: IconProps) {
  const classes = ['ic', filled && 'icf', className].filter(Boolean).join(' ');
  const sizing = size ? { width: size, height: size } : undefined;
  return (
    <svg viewBox="0 0 24 24" className={classes} style={{ ...sizing, strokeWidth, ...style }} aria-hidden="true">
      <path d={ICONS[name]} />
    </svg>
  );
}
