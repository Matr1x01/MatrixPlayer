import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from 'react';

interface SliderProps {
  /** Committed value, 0–1, as reported by the core. */
  value: number;
  /** Continuous updates while dragging. */
  onInput?: (value: number) => void;
  /** Final value on release or keyboard change. */
  onCommit: (value: number) => void;
  label: string;
  /** Human-readable value for assistive tech and the hover bubble. */
  valueText?: (value: number) => string;
  disabled?: boolean;
  /**
   * seek: 4 px at rest; on hover grows to 6 px with a knob and a preview
   * bubble. volume: always shows a small knob, neutral fill. setting:
   * accent fill with a full-size knob, for preference sliders.
   */
  variant?: 'seek' | 'volume' | 'setting';
  /** Keyboard step as a fraction of the range. */
  step?: number;
  className?: string;
  style?: CSSProperties;
}

const clamp = (v: number) => Math.min(1, Math.max(0, v));
/** A committed value counts as confirmed once the core reports this close. */
const SETTLED = 0.015;
/** …or after this long, whatever the core says. */
const SETTLE_MS = 1500;

/** Seek and volume slider (Components · B3). */
export function Slider({
  value,
  onInput,
  onCommit,
  label,
  valueText,
  disabled,
  variant = 'seek',
  step = 0.02,
  className,
  style,
}: SliderProps) {
  const track = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  // The last committed value, shown until the core confirms it: otherwise
  // the knob jumps back to the old value for the moment a seek takes.
  const [pending, setPending] = useState<number | null>(null);
  useEffect(() => {
    if (pending != null && Math.abs(value - pending) < SETTLED) setPending(null);
  }, [value, pending]);
  useEffect(() => {
    if (pending == null) return;
    const timer = window.setTimeout(() => setPending(null), SETTLE_MS);
    return () => window.clearTimeout(timer);
  }, [pending]);
  const commit = (v: number) => {
    setPending(v);
    onCommit(v);
  };

  const shown = clamp(drag ?? pending ?? value);
  const fractionAt = (clientX: number) => {
    const rect = track.current!.getBoundingClientRect();
    return rect.width > 0 ? clamp((clientX - rect.left) / rect.width) : 0;
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled || e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    const f = fractionAt(e.clientX);
    setDrag(f);
    onInput?.(f);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled) return;
    const f = fractionAt(e.clientX);
    setHover(f);
    if (drag != null) {
      setDrag(f);
      onInput?.(f);
    }
  };
  const onPointerUp = (e: PointerEvent<HTMLDivElement>) => {
    if (drag == null) return;
    const f = fractionAt(e.clientX);
    setDrag(null);
    commit(f);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    // Modified arrows are global shortcuts (Ctrl+→ next track…).
    if (disabled || e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return;
    // Repeated key presses step from what is shown, not from a value the
    // core has not confirmed yet.
    const from = pending ?? value;
    const moves: Record<string, number> = {
      ArrowRight: from + step,
      ArrowUp: from + step,
      ArrowLeft: from - step,
      ArrowDown: from - step,
      PageUp: from + step * 5,
      PageDown: from - step * 5,
      Home: 0,
      End: 1,
    };
    const next = moves[e.key];
    if (next === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    commit(clamp(next));
  };

  const hot = !disabled && (hover != null || drag != null);
  const bubbleAt = drag ?? hover;
  const classes = ['slider', `slider-${variant}`, hot && 'hot', disabled && 'disabled', className].filter(Boolean).join(' ');

  return (
    <div
      className={classes}
      style={style}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(shown * 100)}
      aria-valuetext={valueText?.(shown)}
      aria-disabled={disabled}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => setDrag(null)}
      onPointerLeave={() => setHover(null)}
      onKeyDown={onKeyDown}
    >
      <div ref={track} className="bar">
        <i style={{ width: `${shown * 100}%` }} />
        <span className="knob" style={{ left: `${shown * 100}%` }} />
        {variant === 'seek' && hot && bubbleAt != null && valueText && (
          <>
            <span className="slider-mark" style={{ left: `${bubbleAt * 100}%` }} />
            <span className="slider-bubble mono" style={{ left: `${bubbleAt * 100}%` }}>
              {valueText(bubbleAt)}
            </span>
          </>
        )}
      </div>
    </div>
  );
}
