import { IconButton } from '@/components/ui/IconButton';
import { Slider } from '@/components/ui/Slider';
import { usePlayer } from '@/stores/player';

import { formatDb } from './format';
import { toggleMute, useLiveVolume } from './hooks';

/** Level in dB for a slider position, matching the core's cubic curve. */
const levelToDb = (level: number) => (level > 0 ? 60 * Math.log10(level) : null);

interface VolumeControlProps {
  width: number;
  /** Show the applied gain in dB beside the slider (Now Playing). */
  showDb?: boolean;
  iconSize?: number;
}

export function VolumeControl({ width, showDb, iconSize = 17 }: VolumeControlProps) {
  const volume = usePlayer((s) => s.volume);
  const live = useLiveVolume();
  const silent = volume.muted || (!volume.locked && volume.level === 0);
  // Bit-perfect playback plays at 100%: the device's volume sets the level.
  const lockedTip = volume.locked ? 'Fixed at 100% for bit-perfect playback · use your DAC or amp' : undefined;

  return (
    <div className="flex items-center gap-1" data-tip={lockedTip}>
      <IconButton
        icon={silent ? 'volMuted' : 'vol'}
        label={volume.muted ? 'Unmute' : 'Mute'}
        shortcut="M"
        iconSize={iconSize}
        onClick={toggleMute}
      />
      <Slider
        variant="volume"
        label="Volume"
        value={volume.locked ? 1 : volume.level}
        disabled={volume.locked}
        style={{ width, opacity: volume.muted ? 0.5 : 1 }}
        step={0.05}
        valueText={(v) => formatDb(levelToDb(v))}
        onInput={live.input}
        onCommit={live.commit}
      />
      {showDb && (
        <span className="vol-db mono t3 ml-2.5 w-14 text-[11px]">
          {volume.muted ? 'Muted' : volume.locked ? 'Fixed' : formatDb(volume.gainDb)}
        </span>
      )}
    </div>
  );
}
