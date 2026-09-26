interface ToggleProps {
  on: boolean;
  onChange: (on: boolean) => void;
  label: string;
  disabled?: boolean;
  small?: boolean;
}

/** On/off switch (Components · B4). */
export function Toggle({ on, onChange, label, disabled, small }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      className={on ? 'tg on' : 'tg'}
      disabled={disabled}
      onClick={() => onChange(!on)}
      style={small ? { transform: 'scale(0.8)' } : undefined}
    />
  );
}
