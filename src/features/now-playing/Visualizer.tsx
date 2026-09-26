import { useState } from 'react';

import { Artwork } from '@/components/ui/Artwork';
import { Segmented } from '@/components/ui/Segmented';
import type { TrackInfo } from '@/types/player';

type Mode = 'spectrum' | 'waveform' | 'frequency' | 'meters' | 'artwork';

const MODES: { value: Mode; label: string; meta: string; available: boolean }[] = [
  { value: 'spectrum', label: 'Spectrum', meta: 'LOG · 56 BANDS · PEAK HOLD', available: false },
  { value: 'waveform', label: 'Waveform', meta: 'OSCILLOSCOPE · L/R', available: false },
  { value: 'frequency', label: 'Frequency', meta: 'FFT · SMOOTHED', available: false },
  { value: 'meters', label: 'Meters', meta: 'PEAK METER · dBFS', available: false },
  { value: 'artwork', label: 'Artwork', meta: 'VISUALIZER OFF', available: true },
];

/**
 * Visualization panel. Analysis-driven modes need a native analysis tap in
 * the audio pipeline and stay disabled until it exists; they are never faked.
 */
export function Visualizer({ track }: { track: TrackInfo }) {
  const [mode, setMode] = useState<Mode>('artwork');
  const current = MODES.find((m) => m.value === mode)!;

  return (
    <section className="np-viz" aria-label="Visualization">
      <div className="flex items-center justify-between pl-2 pr-2.5 pt-2">
        <Segmented<Mode>
          label="Visualization mode"
          size="sm"
          className="glass"
          value={mode}
          onChange={setMode}
          options={MODES.map((m) => ({
            value: m.value,
            label: m.label,
            disabled: !m.available,
            tip: m.available ? undefined : 'Needs the audio analysis stage (not built yet)',
          }))}
        />
        <span className="np-viz-meta mono t3 text-[10px]">{current.meta}</span>
      </div>
      <div className="flex min-h-0 flex-1 flex-col px-3.5 pb-2 pt-2.5">
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
