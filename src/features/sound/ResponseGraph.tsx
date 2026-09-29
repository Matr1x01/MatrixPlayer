import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';

import { bandFrequency, useAnalysis } from '@/services/analysis';
import { reducedMotion } from '@/services/motion';
import type { EqBand, EqCurve, EqResponse } from '@/types/dsp';

import { GRAPHIC_10, GRAPHIC_10_Q, GRAPHIC_31, GRAPHIC_31_Q, KIND_LABEL, MAX_GAIN_DB, formatFreq, formatGain, hasGain } from './eqMath';
import { DRAW_FREQS } from './eqResponse';

// Plot geometry, as in the Equalizer board (viewBox 840 × 290).
const W = 840;
const H = 290;
const X0 = 44;
const X1 = 828;
const MID = 138;
const TOP = 14;
const BOTTOM = 262;
const SCALE = (MID - TOP) / MAX_GAIN_DB;
const LOG20 = Math.log10(20);

const fx = (f: number) => X0 + ((Math.log10(f) - LOG20) / 3) * (X1 - X0);
const fromX = (x: number) => Math.pow(10, LOG20 + ((x - X0) / (X1 - X0)) * 3);
const gy = (g: number) => MID - g * SCALE;
const fromY = (y: number) => (MID - y) / SCALE;
const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

/** Bandwidth in octaves ↔ Q, for the Q handles. */
const octaves = (q: number) => (2 / Math.LN2) * Math.asinh(1 / (2 * q));
const qFor = (oct: number) => 1 / (2 * Math.sinh((Math.LN2 / 2) * oct));

function path(points: [number, number][]) {
  return points.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)}`).join(' ');
}

const MORPH_MS = 260;

/**
 * The response values to draw: they glide to new targets (a preset, a
 * reset, a typed value), but follow the pointer exactly while dragging.
 */
function useMorph(target: number[], follow: boolean): number[] {
  const [shown, setShown] = useState(target);
  const current = useRef(target);
  useEffect(() => {
    if (follow || reducedMotion()) {
      current.current = target;
      setShown(target);
      return;
    }
    const from = current.current;
    const start = performance.now();
    let raf = requestAnimationFrame(function step(now) {
      const t = Math.min(1, (now - start) / MORPH_MS);
      const e = 1 - Math.pow(1 - t, 3);
      const next = target.map((v, i) => (from[i] ?? v) + (v - (from[i] ?? v)) * e);
      current.current = next;
      setShown(next);
      if (t < 1) raf = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(raf);
  }, [target, follow]);
  return shown;
}

const GRID_MINOR: number[] = [30, 40, 50, 60, 70, 80, 90, 200, 300, 400, 500, 600, 700, 800, 900, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000];
const GRID_MAJOR = [20, 100, 1000, 10000, 20000];
const F_LABELS: [number, string][] = [
  [20, '20'],
  [50, '50'],
  [100, '100'],
  [200, '200'],
  [500, '500'],
  [1000, '1k'],
  [2000, '2k'],
  [5000, '5k'],
  [10000, '10k'],
  [20000, '20k'],
];

export interface GraphNode {
  index: number;
  freq: number;
  gainDb: number;
  band: EqBand;
}

interface ResponseGraphProps {
  curve: EqCurve;
  /** The curve's response as the core plays it, at `DRAW_FREQS`. */
  response: EqResponse | null;
  /** Whether `response` is for this very curve (not the one before it). */
  current: boolean;
  /** The EQ is shaping the sound (on and not bypassed). */
  active: boolean;
  selected: number | null;
  onSelect: (index: number | null) => void;
  /** Parametric band edits. */
  onBand: (index: number, patch: Partial<EqBand>) => void;
  /** Graphic band edits. */
  onGraphic: (index: number, gainDb: number) => void;
  /** Double-click on empty space in parametric mode. */
  onAdd: (freq: number, gainDb: number) => void;
  onRemove: (index: number) => void;
  showSpectrum: boolean;
  legend: React.ReactNode;
}

/** The EQ's frequency response over the live spectrum, with draggable bands. */
export function ResponseGraph({
  curve,
  response,
  current,
  active,
  selected,
  onSelect,
  onBand,
  onGraphic,
  onAdd,
  onRemove,
  showSpectrum,
  legend,
}: ResponseGraphProps) {
  const svg = useRef<SVGSVGElement>(null);
  const [drag, setDrag] = useState<{ index: number; handle: 'node' | 'qLeft' | 'qRight' } | null>(null);
  const frame = useAnalysis(showSpectrum);

  const parametric = curve.mode === 'parametric';
  const nodes: GraphNode[] = useMemo(() => {
    if (parametric) return curve.parametric.map((band, index) => ({ index, freq: band.freq, gainDb: band.gainDb, band }));
    const [freqs, gains, q] =
      curve.mode === 'graphic10' ? [GRAPHIC_10, curve.graphic10, GRAPHIC_10_Q] : [GRAPHIC_31, curve.graphic31, GRAPHIC_31_Q];
    return freqs.map((freq, index) => {
      const gainDb = gains[index] ?? 0;
      return { index, freq, gainDb, band: { enabled: true, kind: 'peak', freq, gainDb, q } };
    });
  }, [curve, parametric]);

  const target = useMemo(
    () => DRAW_FREQS.map((_, i) => clamp(response?.total[i] ?? 0, -MAX_GAIN_DB - 2, MAX_GAIN_DB + 2)),
    [response],
  );
  // The first response jumps into place instead of rising from a flat line.
  const hadResponse = useRef(response != null);
  const shown = useMorph(target, drag != null || !hadResponse.current);
  useEffect(() => {
    if (response) hadResponse.current = true;
  }, [response]);
  const curvePath = useMemo(() => path(DRAW_FREQS.map((f, i) => [fx(f), gy(shown[i]!)])), [shown]);
  const sel = selected != null ? nodes[selected] : undefined;
  const selPath = useMemo(() => {
    const one = sel && parametric && current ? response?.bands[sel.index] : undefined;
    if (!one) return null;
    return path(DRAW_FREQS.map((f, i) => [fx(f), gy(clamp(one[i] ?? 0, -MAX_GAIN_DB - 2, MAX_GAIN_DB + 2))]));
  }, [sel, parametric, response, current]);

  const spectrumPath = useMemo(() => {
    if (!showSpectrum) return null;
    const pts: [number, number][] = frame.bands.map((level, i) => [clamp(fx(bandFrequency(i)), X0, X1), BOTTOM - level * 200]);
    if (!pts.length || frame.bands.every((v) => v === 0)) return null;
    return `${path([[X0, BOTTOM], ...pts, [X1, BOTTOM]])} Z`;
  }, [frame, showSpectrum]);

  const toPlot = (e: { clientX: number; clientY: number }) => {
    const r = svg.current!.getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * W, ((e.clientY - r.top) / r.height) * H] as const;
  };

  const moveTo = (index: number, x: number, y: number) => {
    const node = nodes[index];
    if (!node) return;
    const gain = Math.round(clamp(fromY(y), -MAX_GAIN_DB, MAX_GAIN_DB) * 10) / 10;
    if (!parametric) {
      onGraphic(index, gain);
      return;
    }
    const freq = clamp(fromX(x), 20, 20000);
    const rounded = freq >= 1000 ? Math.round(freq / 10) * 10 : Math.round(freq);
    onBand(index, hasGain(node.band.kind) ? { freq: rounded, gainDb: gain } : { freq: rounded });
  };

  const onPointerMove = (e: PointerEvent<SVGSVGElement>) => {
    if (!drag) return;
    const [x, y] = toPlot(e);
    if (drag.handle === 'node') moveTo(drag.index, x, y);
    else {
      const node = nodes[drag.index];
      if (!node) return;
      const oct = Math.abs(Math.log2(fromX(x) / node.freq)) * 2;
      onBand(drag.index, { q: Math.round(clamp(qFor(Math.max(oct, 0.05)), 0.1, 20) * 100) / 100 });
    }
  };

  const onNodeKey = (e: KeyboardEvent, node: GraphNode) => {
    const fine = e.shiftKey ? 0.1 : 1;
    const semitone = Math.pow(2, 1 / 12);
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      const g = clamp(node.gainDb + (e.key === 'ArrowUp' ? 0.5 : -0.5) * fine, -MAX_GAIN_DB, MAX_GAIN_DB);
      if (parametric) onBand(node.index, { gainDb: Math.round(g * 10) / 10 });
      else onGraphic(node.index, Math.round(g * 10) / 10);
    } else if (parametric && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      const f = node.freq * Math.pow(semitone, (e.key === 'ArrowRight' ? 1 : -1) * (e.shiftKey ? 0.25 : 1));
      onBand(node.index, { freq: Math.round(clamp(f, 20, 20000)) });
    } else if (parametric && (e.key === 'Delete' || e.key === 'Backspace')) {
      onRemove(node.index);
    } else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const onWheel = (e: React.WheelEvent) => {
    if (!sel || !parametric || e.deltaY === 0) return;
    const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
    onBand(sel.index, { q: Math.round(clamp(sel.band.q * factor, 0.1, 20) * 100) / 100 });
  };

  const small = curve.mode === 'graphic31';
  const qHandles = sel && parametric && (sel.band.kind === 'peak' || sel.band.kind === 'notch');
  const half = sel ? (hasGain(sel.band.kind) ? sel.gainDb / 2 : 0) : 0;
  const bw = sel ? octaves(sel.band.q) / 2 : 0;
  const tip = sel ? { x: (fx(sel.freq) / W) * 100, y: (gy(hasGain(sel.band.kind) ? sel.gainDb : 0) / H) * 100 } : null;

  return (
    <section className="panel eq-graph" aria-label="Frequency response">
      <div className="eq-graph-head">{legend}</div>
      <div className="relative" onWheel={onWheel}>
        <svg
          ref={svg}
          viewBox={`0 0 ${W} ${H}`}
          className={['eq-svg', !active && 'inactive', drag && 'dragging'].filter(Boolean).join(' ')}
          onPointerMove={onPointerMove}
          onPointerUp={() => setDrag(null)}
          onPointerCancel={() => setDrag(null)}
          onPointerDown={(e) => {
            if (e.target === svg.current || (e.target as Element).classList.contains('eq-bg')) onSelect(null);
          }}
          onDoubleClick={(e) => {
            if (!parametric || (e.target as Element).closest('.eq-node')) return;
            const [x, y] = toPlot(e);
            if (x < X0 || x > X1) return;
            onAdd(clamp(fromX(x), 20, 20000), clamp(fromY(y), -MAX_GAIN_DB, MAX_GAIN_DB));
          }}
        >
          <defs>
            <linearGradient id="eqf" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" style={{ stopColor: 'var(--ac)', stopOpacity: 0.28 }} />
              <stop offset="1" style={{ stopColor: 'var(--ac)', stopOpacity: 0.02 }} />
            </linearGradient>
          </defs>
          <rect className="eq-bg" x={X0} y={TOP} width={X1 - X0} height={BOTTOM - TOP} fill="transparent" />
          {spectrumPath && <path d={spectrumPath} className="eq-spectrum" />}
          <path
            d={[...GRID_MINOR.map((f) => `M${fx(f).toFixed(1)} ${TOP}V${BOTTOM}`), ...[-9, -3, 3, 9].map((g) => `M${X0} ${gy(g)}H${X1}`)].join('')}
            style={{ stroke: 'var(--ln)' }}
            strokeWidth="1"
          />
          <path
            d={[...GRID_MAJOR.map((f) => `M${fx(f).toFixed(1)} ${TOP}V${BOTTOM}`), ...[-12, -6, 6, 12].map((g) => `M${X0} ${gy(g)}H${X1}`)].join('')}
            style={{ stroke: 'var(--ln2)' }}
            strokeWidth="1"
          />
          <path d={`M${X0} ${MID}H${X1}`} style={{ stroke: 'var(--tx3)' }} strokeWidth="1" opacity=".6" />
          {[12, 6, 0, -6, -12].map((g) => (
            <text key={g} x="36" y={gy(g) + 3} textAnchor="end" className="eq-axis">
              {g > 0 ? `+${g}` : g < 0 ? `−${-g}` : '0'}
            </text>
          ))}
          {F_LABELS.map(([f, t]) => (
            <text key={f} x={fx(f)} y="282" textAnchor="middle" className="eq-axis">
              {t}
            </text>
          ))}
          {selPath && (
            <>
              <path d={`${selPath} L${X1} ${MID} L${X0} ${MID} Z`} style={{ fill: 'var(--ac)' }} opacity=".10" />
              <path d={selPath} fill="none" style={{ stroke: 'var(--ac)' }} strokeWidth="1.2" strokeDasharray="4 3" />
            </>
          )}
          {response && (
            <>
              <path d={`${curvePath} L${X1} ${MID} L${X0} ${MID} Z`} fill="url(#eqf)" className="eq-fill" />
              <path d={curvePath} fill="none" className="eq-curve" strokeWidth="2.2" strokeLinejoin="round" />
            </>
          )}
          {qHandles && sel && (
            <>
              <line
                x1={fx(sel.freq * Math.pow(2, -bw))}
                x2={fx(sel.freq * Math.pow(2, bw))}
                y1={gy(half)}
                y2={gy(half)}
                style={{ stroke: 'var(--ac)' }}
                strokeWidth="1"
                opacity=".8"
              />
              {(['qLeft', 'qRight'] as const).map((handle) => (
                <circle
                  key={handle}
                  className="eq-q"
                  cx={fx(sel.freq * Math.pow(2, handle === 'qLeft' ? -bw : bw))}
                  cy={gy(half)}
                  r="4"
                  onPointerDown={(e) => {
                    e.stopPropagation();
                    svg.current?.setPointerCapture(e.pointerId);
                    setDrag({ index: sel.index, handle });
                  }}
                >
                  <title>Drag to change the bandwidth (Q)</title>
                </circle>
              ))}
            </>
          )}
          {nodes.map((node) => {
            const isSel = node.index === selected;
            const y = gy(hasGain(node.band.kind) ? node.gainDb : 0);
            const r = small ? (isSel ? 6 : 4.5) : isSel ? 10 : 8;
            const label = `Band ${node.index + 1}, ${KIND_LABEL[node.band.kind]}, ${formatFreq(node.freq)}${
              hasGain(node.band.kind) ? `, ${formatGain(node.gainDb)}` : ''
            }`;
            return (
              <g
                key={node.index}
                className={['eq-node', isSel && 'sel', !node.band.enabled && 'off'].filter(Boolean).join(' ')}
                style={{ transform: `translate(${fx(node.freq)}px, ${y}px)` }}
                role="slider"
                tabIndex={0}
                aria-label={label}
                aria-valuenow={Math.round(node.gainDb * 10) / 10}
                aria-valuemin={-MAX_GAIN_DB}
                aria-valuemax={MAX_GAIN_DB}
                onFocus={() => onSelect(node.index)}
                onKeyDown={(e) => onNodeKey(e, node)}
                onPointerDown={(e) => {
                  if (e.button !== 0) return;
                  e.stopPropagation();
                  svg.current?.setPointerCapture(e.pointerId);
                  onSelect(node.index);
                  setDrag({ index: node.index, handle: 'node' });
                }}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  if (parametric) onBand(node.index, { gainDb: 0 });
                  else onGraphic(node.index, 0);
                }}
              >
                <circle r={r + 6} fill="transparent" />
                <circle r={r} className="eq-dot" strokeWidth="1.6" />
                {!small && (
                  <text y={3.5} textAnchor="middle" className="eq-num">
                    {node.index + 1}
                  </text>
                )}
              </g>
            );
          })}
        </svg>
        {sel && tip && (
          <div
            className="eq-tip"
            style={{
              left: `${tip.x}%`,
              top: `${tip.y}%`,
              transform: tip.x > 70 ? 'translate(calc(-100% - 18px), -50%)' : 'translate(18px, -50%)',
            }}
          >
            <span className="cap" style={{ color: 'var(--ac)' }}>
              Band {sel.index + 1} · {KIND_LABEL[sel.band.kind]}
            </span>
            <span className="mono text-[11px]">
              {formatFreq(sel.freq)}
              {hasGain(sel.band.kind) && ` · ${formatGain(sel.gainDb)}`}
              {parametric && ` · Q ${sel.band.q.toFixed(2)}`}
            </span>
          </div>
        )}
      </div>
    </section>
  );
}
