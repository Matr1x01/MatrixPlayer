import { Icon } from '@/components/icons/Icon';
import { IconButton } from '@/components/ui/IconButton';
import { playerApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';

import { cycleRepeat, showsPause, toggleShuffle } from './hooks';

const SCALES = {
  // Player bar: 32 px buttons, 40 px play.
  bar: { side: 32, sideIcon: 16, skip: 32, skipIcon: 18, play: 40, playIcon: 18, gap: 14 },
  // Now Playing: 38 / 44 / 68 px (Components · B2).
  large: { side: 38, sideIcon: 18, skip: 44, skipIcon: 24, play: 68, playIcon: 28, gap: 18 },
} as const;

const REPEAT_LABELS = { off: 'Repeat off', all: 'Repeat all', one: 'Repeat one' } as const;

/** Shuffle · previous · play/pause · next · repeat, bound to the core. */
export function Transport({ scale = 'bar' }: { scale?: keyof typeof SCALES }) {
  const s = SCALES[scale];
  const status = usePlayer((p) => p.status);
  const hasQueue = usePlayer((p) => p.queue.entries.length > 0);
  const shuffle = usePlayer((p) => p.queue.shuffle);
  const repeat = usePlayer((p) => p.queue.repeat);
  const pause = showsPause(status);

  return (
    <div className="flex items-center" style={{ gap: s.gap }}>
      <IconButton
        icon="shuffle"
        label={shuffle ? 'Shuffle on' : 'Shuffle off'}
        shortcut="S"
        active={shuffle}
        size={s.side}
        iconSize={s.sideIcon}
        onClick={toggleShuffle}
      />
      <IconButton
        icon="prev"
        label="Previous"
        shortcut="Ctrl ←"
        filled
        size={s.skip}
        iconSize={s.skipIcon}
        style={{ color: 'var(--tx)' }}
        disabled={!hasQueue}
        onClick={() => void run(playerApi.previous())}
      />
      <button
        type="button"
        className="play"
        aria-label={pause ? 'Pause' : 'Play'}
        data-tip={pause ? 'Pause' : 'Play'}
        data-tip-key="Space"
        disabled={!hasQueue}
        style={{
          width: s.play,
          height: s.play,
          boxShadow: scale === 'large' ? '0 10px 30px color-mix(in oklab, var(--ac) 35%, transparent)' : undefined,
        }}
        onClick={() => void run(playerApi.toggle())}
      >
        <Icon name={pause ? 'pause' : 'play'} filled size={s.playIcon} />
      </button>
      <IconButton
        icon="next"
        label="Next"
        shortcut="Ctrl →"
        filled
        size={s.skip}
        iconSize={s.skipIcon}
        style={{ color: 'var(--tx)' }}
        disabled={!hasQueue}
        onClick={() => void run(playerApi.next())}
      />
      <IconButton
        icon="repeat"
        label={REPEAT_LABELS[repeat]}
        shortcut="R"
        active={repeat !== 'off'}
        badge={repeat === 'one' ? '1' : undefined}
        size={s.side}
        iconSize={s.sideIcon}
        onClick={cycleRepeat}
      />
    </div>
  );
}
