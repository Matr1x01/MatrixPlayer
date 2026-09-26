import { useState } from 'react';

import { Slider } from '@/components/ui/Slider';
import { playerApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';

import { formatTime } from './format';
import { isSeekable } from './hooks';

/**
 * Position readout and seek slider for the player bar, driven by core
 * position events. Now Playing seeks over the waveform instead.
 */
export function SeekBar() {
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

  return (
    <div className="flex w-full items-center gap-2.5">
      <span className="mono t3 w-9 text-right text-[10.5px]">{formatTime(shownMs)}</span>
      {slider}
      <span className="mono t3 w-9 text-[10.5px]">{formatTime(durationMs)}</span>
    </div>
  );
}
