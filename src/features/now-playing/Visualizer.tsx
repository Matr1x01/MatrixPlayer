import { useRef } from 'react';

import { Artwork } from '@/components/ui/Artwork';
import { Segmented } from '@/components/ui/Segmented';
import { LevelMeters } from '@/features/sound/DspView';
import { useAnalysis } from '@/services/analysis';
import { setViewPrefs, useViewPrefs, type VisualizerMode } from '@/stores/viewPrefs';
import type { AnalysisFrame } from '@/types/dsp';
import type { TrackInfo } from '@/types/player';

const MODES: { value: VisualizerMode; label: string; meta: string }[] = [
  { value: 'spectrum', label: 'Spectrum', meta: 'LOG · 56 BANDS · PEAK HOLD' },
  { value: 'waveform', label: 'Waveform', meta: 'OSCILLOSCOPE · L/R' },
  { value: 'frequency', label: 'Frequency', meta: 'FFT 4096 · SMOOTHED' },
  { value: 'meters', label: 'Meters', meta: 'PEAK METER · dBFS' },
  { value: 'artwork', label: 'Artwork', meta: 'VISUALIZER OFF' },
];

/** Peak-hold markers that fall slowly after each peak. */
function usePeakHold(bands: number[]): number[] {
  const held = useRef<{ level: number; age: number }[]>([]);
  if (held.current.length !== bands.length) held.current = bands.map(() => ({ level: 0, age: 0 }));
  return bands.map((v, i) => {
    const h = held.current[i]!;
    if (v >= h.level) {
      h.level = v;
      h.age = 0;
    } else if (++h.age > 18) {
      h.level = Math.max(v, h.level - 0.012);
    }
    return h.level;
  });
}

function Spectrum({ frame }: { frame: AnalysisFrame }) {
  const peaks = usePeakHold(frame.bands);
  return (
    <>
      <div className="viz-spec">
        {frame.bands.map((v, i) => (
          <div key={i} className="sp">
            <s style={{ bottom: `${Math.min(98, peaks[i]! * 100 + 1.5)}%`, opacity: peaks[i]! > 0.02 ? 0.8 : 0 }} />
            <b style={{ height: `${v * 100}%` }} />
          </div>
        ))}
      </div>
      <div className="viz-axis mono t3">
        {['30', '60', '125', '250', '500', '1k', '2k', '4k', '8k', '16k'].map((t) => (
          <span key={t}>{t}</span>
        ))}
      </div>
    </>
  );
}

function Scope({ frame }: { frame: AnalysisFrame }) {
  const line = (samples: number[]) =>
    samples.map((v, i) => `${i ? 'L' : 'M'}${((i / (samples.length - 1)) * 500).toFixed(1)} ${(65 - v * 58).toFixed(1)}`).join(' ');
  return (
    <>
      <svg viewBox="0 0 500 130" preserveAspectRatio="none" className="viz-svg" aria-hidden>
        <path d="M0 65H500" style={{ stroke: 'var(--ln2)' }} strokeWidth="1" strokeDasharray="3 4" vectorEffect="non-scaling-stroke" />
        <path d={line(frame.scope[1])} fill="none" style={{ stroke: 'var(--tx3)' }} strokeWidth="1" opacity=".6" vectorEffect="non-scaling-stroke" />
        <path d={line(frame.scope[0])} fill="none" style={{ stroke: 'var(--ac)' }} strokeWidth="1.6" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
      </svg>
      <div className="viz-axis mono t3">
        <span>L —— R - - -</span>
        <span>25 ms window</span>
      </div>
    </>
  );
}

function Frequency({ frame }: { frame: AnalysisFrame }) {
  const peaks = usePeakHold(frame.bands);
  const n = frame.bands.length;
  const pts = (values: number[], lift = 0) =>
    values.map((v, i) => `${i ? 'L' : 'M'}${((i / (n - 1)) * 500).toFixed(1)} ${(126 - Math.min(1, v + lift) * 118).toFixed(1)}`).join(' ');
  const line = pts(frame.bands);
  return (
    <>
      <svg viewBox="0 0 500 130" preserveAspectRatio="none" className="viz-svg" aria-hidden>
        <defs>
          <linearGradient id="viz-fg" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" style={{ stopColor: 'var(--ac)', stopOpacity: 0.45 }} />
            <stop offset="1" style={{ stopColor: 'var(--ac)', stopOpacity: 0 }} />
          </linearGradient>
        </defs>
        <path d="M0 32.5H500M0 65H500M0 97.5H500" style={{ stroke: 'var(--ln)' }} strokeWidth="1" vectorEffect="non-scaling-stroke" />
        <path d={`${line} L500 130 L0 130 Z`} fill="url(#viz-fg)" />
        <path d={line} fill="none" style={{ stroke: 'var(--ac)' }} strokeWidth="1.8" vectorEffect="non-scaling-stroke" />
        <path d={pts(peaks, 0.03)} fill="none" style={{ stroke: 'var(--tx2)' }} strokeWidth="1" strokeDasharray="2 3" vectorEffect="non-scaling-stroke" />
      </svg>
      <div className="viz-axis mono t3">
        <span>30 Hz</span>
        <span>100</span>
        <span>1k</span>
        <span>10k</span>
        <span>18k</span>
      </div>
    </>
  );
}

/** Visualization panel: live analysis from the core's DSP tap, or artwork. */
export function Visualizer({ track }: { track: TrackInfo }) {
  const mode = useViewPrefs((s) => s.visualizer);
  const current = MODES.find((m) => m.value === mode) ?? MODES[0]!;
  const analysing = mode !== 'artwork' && mode !== 'meters';
  const frame = useAnalysis(analysing);

  return (
    <section className="np-viz" aria-label="Visualization">
      <div className="flex items-center justify-between pl-2 pr-2.5 pt-2">
        <Segmented<VisualizerMode>
          label="Visualization mode"
          size="sm"
          className="glass"
          value={mode}
          onChange={(visualizer) => setViewPrefs({ visualizer })}
          options={MODES.map((m) => ({ value: m.value, label: m.label }))}
        />
        <span className="np-viz-meta mono t3 text-[10px]">{current.meta}</span>
      </div>
      <div key={mode} className="viz-body">
        {mode === 'spectrum' && <Spectrum frame={frame} />}
        {mode === 'waveform' && <Scope frame={frame} />}
        {mode === 'frequency' && <Frequency frame={frame} />}
        {mode === 'meters' && (
          <div className="flex flex-1 flex-col justify-center">
            <LevelMeters large />
          </div>
        )}
        {mode === 'artwork' && (
          <Artwork
            artwork={track.artwork}
            full
            title={track.metadata.album ?? track.metadata.title}
            className="np-viz-art"
            style={{ width: '100%', height: '100%', aspectRatio: 'auto', borderRadius: 6 }}
          />
        )}
      </div>
    </section>
  );
}
