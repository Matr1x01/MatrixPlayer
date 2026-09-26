import type { CSSProperties, MouseEventHandler, ReactNode } from 'react';

import { Icon } from '@/components/icons/Icon';
import type { IconName } from '@/components/icons/paths';

interface IconButtonProps {
  icon: IconName;
  /** Accessible name, also shown as the tooltip. */
  label: string;
  /** Keyboard shortcut shown in the tooltip. */
  shortcut?: string;
  onClick?: MouseEventHandler<HTMLButtonElement>;
  /** Accent colour: the control's feature is on (shuffle, repeat…). */
  active?: boolean;
  /** Raised background: the view this button opens is showing. */
  open?: boolean;
  disabled?: boolean;
  filled?: boolean;
  /** Button box size in px (design default 32). */
  size?: number;
  iconSize?: number;
  strokeWidth?: number;
  badge?: ReactNode;
  className?: string;
  style?: CSSProperties;
}

export function IconButton({
  icon,
  label,
  shortcut,
  onClick,
  active,
  open,
  disabled,
  filled,
  size,
  iconSize,
  strokeWidth,
  badge,
  className,
  style,
}: IconButtonProps) {
  const classes = ['ib', active && 'on', open && 'open', className].filter(Boolean).join(' ');
  return (
    <button
      type="button"
      className={classes}
      aria-label={label}
      aria-pressed={active}
      data-tip={label}
      data-tip-key={shortcut}
      onClick={onClick}
      disabled={disabled}
      style={size ? { width: size, height: size, ...style } : style}
    >
      <Icon name={icon} size={iconSize} filled={filled} strokeWidth={strokeWidth} />
      {badge != null && <span className="badge">{badge}</span>}
    </button>
  );
}
