// Controls for continuous sound parameters: rotary knobs, bipolar sliders,
// vertical faders and typed values. All of them report changes as they
// happen (the store coalesces what reaches the core), support the wheel
// and arrow keys, and reset to their default on double-click.

import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type WheelEvent } from 'react';

import { Segmented } from '@/components/ui/Segmented';
import { patchDsp } from '@/stores/dsp';
import type { CrossfeedPreset } from '@/types/dsp';

import { CROSSFEED_PRESETS } from './chain';

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));
const snap = (v: number, step: number) => Math.round(v / step) * step;

interface RangeProps {
  value: number;
  min: number;
  max: number;
  /** Keyboard and wheel step. */
  step: number;
  /** Value for double-click reset. */
  reset: number;
  onChange: (value: number) => void;
  label: string;
  valueText: string;
  disabled?: boolean;
}

/** Arrow keys, Page Up/Down and Home/End for a range control. */
function keyStep(e: KeyboardEvent, { value, min, max, step, reset, onChange, disabled }: RangeProps) {
  if (disabled || e.ctrlKey || e.altKey || e.metaKey) return;
  const fine = e.shiftKey ? 0.1 : 1;
  const moves: Record<string, number> = {
    ArrowUp: value + step * fine,
    ArrowRight: value + step * fine,
    ArrowDown: value - step * fine,
    ArrowLeft: value - step * fine,
    PageUp: value + step * 5,
    PageDown: value - step * 5,
    Home: min,
    End: max,
    Delete: reset,
  };
  const next = moves[e.key];
  if (next === undefined) return;
  e.preventDefault();
  e.stopPropagation();
  onChange(clamp(snap(next, step * fine), min, max));
}

function wheelStep(e: WheelEvent, { value, min, max, step, onChange, disabled }: RangeProps) {
  if (disabled || e.deltaY === 0) return;
  const fine = e.shiftKey ? 0.1 : 1;
  onChange(clamp(snap(value + (e.deltaY < 0 ? step : -step) * fine, step * fine), min, max));
}

/** Blocks page scrolling while the wheel adjusts a control. */
function useWheelLock<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const stop = (e: globalThis.WheelEvent) => e.preventDefault();
    el.addEventListener('wheel', stop, { passive: false });
    return () => el.removeEventListener('wheel', stop);
  }, []);
  return ref;
}

/** Rotary knob, drawn like the design's tone knobs (−135° … +135°). */
export function Knob(props: RangeProps & { caption: string }) {
  const { value, min, max, reset, onChange, label, valueText, caption, disabled } = props;
  const ref = useWheelLock<HTMLDivElement>();
  const drag = useRef<{ y: number; value: number } | null>(null);
  const span = max - min;
  // 0 sits at the top for bipolar ranges.
  const angle = ((value - (min + max) / 2) / (span / 2)) * 135;
  const zeroAngle = min < 0 && max > 0 ? ((0 - (min + max) / 2) / (span / 2)) * 135 : -135;

  const pt = (a: number, r: number) => {
    const rad = (a * Math.PI) / 180;
    return [26 + r * Math.sin(rad), 26 - r * Math.cos(rad)] as const;
  };
  const arc = (a0: number, a1: number, r: number) => {
    const [x0, y0] = pt(a0, r);
    const [x1, y1] = pt(a1, r);
    return `M${x0.toFixed(2)} ${y0.toFixed(2)} A${r} ${r} 0 ${Math.abs(a1 - a0) > 180 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
  };
  const [p1x, p1y] = pt(angle, 6);
  const [p2x, p2y] = pt(angle, 13);
  const from = Math.min(zeroAngle, angle);
  const to = Math.max(zeroAngle, angle);

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled || e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { y: e.clientY, value };
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    // 160 px of travel covers the whole range; Shift for fine control.
    const perPx = (span / 160) * (e.shiftKey ? 0.2 : 1);
    onChange(clamp(snap(drag.current.value + (drag.current.y - e.clientY) * perPx, props.step / 10), min, max));
  };

  return (
    <div className="kn">
      <div
        ref={ref}
        className="kn-dial"
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={Math.round(value * 10) / 10}
        aria-valuetext={valueText}
        aria-disabled={disabled}
        data-tip={`${label} · drag, scroll or double-click to reset`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={() => (drag.current = null)}
        onPointerCancel={() => (drag.current = null)}
        onDoubleClick={() => !disabled && onChange(reset)}
        onKeyDown={(e) => keyStep(e, props)}
        onWheel={(e) => wheelStep(e, props)}
      >
        <svg viewBox="0 0 52 52" width="56" height="56" aria-hidden>
          <circle cx="26" cy="26" r="15" style={{ fill: 'var(--bg3)', stroke: 'var(--ln2)' }} strokeWidth="1" />
          <path d={arc(-135, 135, 22)} fill="none" style={{ stroke: 'var(--bg4)' }} strokeWidth="3.2" strokeLinecap="round" />
          {to - from > 0.5 && (
            <path d={arc(from, to, 22)} fill="none" style={{ stroke: 'var(--ac)' }} strokeWidth="3.2" strokeLinecap="round" />
          )}
          <line x1={p1x} y1={p1y} x2={p2x} y2={p2y} style={{ stroke: 'var(--tx)' }} strokeWidth="2" strokeLinecap="round" />
        </svg>
      </div>
      <span className="mono text-[11.5px]">{valueText}</span>
      <span className="cap text-[9.5px]">{caption}</span>
    </div>
  );
}

/** Horizontal slider with a marked centre, for balance, width and trims. */
export function CenterSlider(props: RangeProps & { center: number; minLabel?: string; maxLabel?: string }) {
  const { value, min, max, center, reset, onChange, label, valueText, minLabel, maxLabel, disabled } = props;
  const track = useRef<HTMLDivElement>(null);
  const wheelRef = useWheelLock<HTMLDivElement>();
  const [dragging, setDragging] = useState(false);
  const pos = (v: number) => ((v - min) / (max - min)) * 100;
  const at = (clientX: number) => {
    const r = track.current!.getBoundingClientRect();
    const v = min + clamp((clientX - r.left) / r.width, 0, 1) * (max - min);
    // A little magnetism at the centre makes it easy to find.
    return Math.abs(v - center) < (max - min) * 0.015 ? center : snap(v, props.step / 10);
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled || e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
    onChange(at(e.clientX));
  };

  const [a, b] = [Math.min(pos(value), pos(center)), Math.max(pos(value), pos(center))];
  return (
    <div className="flex items-center gap-2">
      {minLabel && <span className="mono t3 text-[10px]">{minLabel}</span>}
      <div
        ref={wheelRef}
        className={disabled ? 'cslider disabled' : dragging ? 'cslider hot' : 'cslider'}
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={Math.round(value * 100) / 100}
        aria-valuetext={valueText}
        aria-disabled={disabled}
        onPointerDown={onPointerDown}
        onPointerMove={(e) => dragging && onChange(at(e.clientX))}
        onPointerUp={() => setDragging(false)}
        onPointerCancel={() => setDragging(false)}
        onDoubleClick={() => !disabled && onChange(reset)}
        onKeyDown={(e) => keyStep(e, props)}
        onWheel={(e) => wheelStep(e, props)}
      >
        <div ref={track} className="bar">
          <span className="cslider-mid" style={{ left: `${pos(center)}%` }} />
          {b - a > 0.1 && <i style={{ left: `${a}%`, width: `${b - a}%` }} />}
          <span className="knob" style={{ left: `${pos(value)}%` }} />
        </div>
      </div>
      {maxLabel && <span className="mono t3 text-[10px]">{maxLabel}</span>}
    </div>
  );
}

/** Vertical fader for graphic EQ bands, filling from 0 dB. */
export function Fader(props: RangeProps & { caption: string; compact?: boolean; order?: number }) {
  const { value, min, max, reset, onChange, label, valueText, caption, compact, disabled, order } = props;
  const track = useRef<HTMLDivElement>(null);
  const wheelRef = useWheelLock<HTMLDivElement>();
  const [dragging, setDragging] = useState(false);
  const pos = (v: number) => ((v - min) / (max - min)) * 100;
  const at = (clientY: number) => {
    const r = track.current!.getBoundingClientRect();
    return snap(max - clamp((clientY - r.top) / r.height, 0, 1) * (max - min), props.step / 5);
  };
  const zero = pos(0);
  const [lo, hi] = [Math.min(pos(value), zero), Math.max(pos(value), zero)];

  return (
    <div className={compact ? 'fader compact' : 'fader'} style={{ '--i': order } as React.CSSProperties}>
      {!compact && <span className="mono fader-value">{valueText}</span>}
      <div
        ref={wheelRef}
        className={dragging ? 'fader-hit hot' : 'fader-hit'}
        role="slider"
        aria-orientation="vertical"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={Math.round(value * 10) / 10}
        aria-valuetext={valueText}
        data-tip={compact ? `${caption} · ${valueText}` : undefined}
        onPointerDown={(e) => {
          if (disabled || e.button !== 0) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          setDragging(true);
          onChange(at(e.clientY));
        }}
        onPointerMove={(e) => dragging && onChange(at(e.clientY))}
        onPointerUp={() => setDragging(false)}
        onPointerCancel={() => setDragging(false)}
        onDoubleClick={() => !disabled && onChange(reset)}
        onKeyDown={(e) => keyStep(e, props)}
        onWheel={(e) => wheelStep(e, props)}
      >
        <div ref={track} className="fader-track">
          <span className="fader-zero" style={{ bottom: `${zero}%` }} />
          {hi - lo > 0.2 && <i style={{ bottom: `${lo}%`, height: `${hi - lo}%` }} />}
          <span className="fader-knob" style={{ bottom: `${pos(value)}%` }} />
        </div>
      </div>
      <span className="mono t3 fader-caption">{caption}</span>
    </div>
  );
}

/**
 * A number shown in mono that can be typed over, scrolled or nudged with
 * the arrow keys. Invalid input is discarded.
 */
export function ValueField({
  display,
  edit,
  parse,
  onCommit,
  onStep,
  label,
  color,
  disabled,
}: {
  display: string;
  /** Text placed in the field when editing starts. */
  edit: string;
  parse: (text: string) => number | null;
  onCommit: (value: number) => void;
  /** Wheel / arrow nudge, +1 or −1 (Shift passes a fine flag). */
  onStep: (direction: 1 | -1, fine: boolean) => void;
  label: string;
  color?: string;
  disabled?: boolean;
}) {
  const [text, setText] = useState<string | null>(null);
  const ref = useWheelLock<HTMLInputElement>();
  const commit = () => {
    if (text != null) {
      const v = parse(text);
      if (v != null) onCommit(v);
    }
    setText(null);
  };
  return (
    <input
      ref={ref}
      className="vfield mono"
      aria-label={label}
      disabled={disabled}
      style={{ color }}
      value={text ?? display}
      onFocus={(e) => {
        setText(edit);
        requestAnimationFrame(() => e.target.select());
      }}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        else if (e.key === 'Escape') {
          setText(null);
          requestAnimationFrame(() => e.currentTarget?.blur());
        } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
          e.preventDefault();
          onStep(e.key === 'ArrowUp' ? 1 : -1, e.shiftKey);
          setText(null);
        }
      }}
      onWheel={(e) => {
        if (e.deltaY === 0 || disabled) return;
        onStep(e.deltaY < 0 ? 1 : -1, e.shiftKey);
        setText(null);
      }}
    />
  );
}

/** bs2b's crossfeed presets, next to the crossfeed switch. */
export function CrossfeedPresets({ value }: { value: CrossfeedPreset }) {
  return (
    <Segmented<CrossfeedPreset>
      label="Crossfeed preset"
      size="sm"
      value={value}
      onChange={(crossfeedPreset) => patchDsp({ crossfeedPreset })}
      options={CROSSFEED_PRESETS.map(({ value, label, detail }) => ({ value, label, tip: detail }))}
    />
  );
}
