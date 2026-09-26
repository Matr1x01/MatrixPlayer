import { useState } from 'react';

import { Slider } from '@/components/ui/Slider';
import { playerApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';

import { formatTime } from './format';
import { isSeekable } from './hooks';

interface SeekBarProps {
  /** "bar": times either side (player bar). "split": times below (Now Playing). */
  layout?: 'bar' | 'split';
}

/** Position readout and seek slider, driven by core position events. */
export function SeekBar({ layout = 'bar' }: SeekBarProps) {
  const positionMs = usePlayer((s) => s.positionMs);
  const durationMs = usePlayer((s) => s.durationMs);
  const status = usePlayer((s) => s.status);
  // While dragging, the readout follows the pointer; the core is only asked
  // to seek on release.
  const [scrubMs, setScrubMs] = useState<number | null>(null);

  const duration = durationMs ?? 0;
  const enabled = duration > 0 && isSeekable(status);
  const shownMs = scrubMs ?? positionMs;
  const fraction = duration > 0 ? Math.min(1, positionMs / duration) : 0;

  const slider = (
    <Slider
      className="flex-1"
      label="Seek"
      value={fraction}
      disabled={!enabled}
      step={duration > 0 ? 5000 / duration : 0.02}
      valueText={(f) => formatTime(f * duration)}
      onInput={(f) => setScrubMs(f * duration)}
      onCommit={(f) => {
        setScrubMs(null);
        void run(playerApi.seek(f * duration));
      }}
    />
  );

  if (layout === 'split') {
    return (
      <div className="flex flex-col gap-2">
        {slider}
        <div className="mono flex justify-between text-[12px]">
          <span>{formatTime(shownMs)}</span>
          <span className="t3">{duration > 0 ? `−${formatTime(Math.max(0, duration - shownMs))}` : '–:––'}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="flex w-full items-center gap-2.5">
      <span className="mono t3 w-9 text-right text-[10.5px]">{formatTime(shownMs)}</span>
      {slider}
      <span className="mono t3 w-9 text-[10.5px]">{formatTime(durationMs)}</span>
    </div>
  );
}
