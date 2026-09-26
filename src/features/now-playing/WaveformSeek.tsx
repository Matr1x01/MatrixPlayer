import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';

import { formatTime } from '@/features/player/format';
import { isSeekable } from '@/features/player/hooks';
import { playerApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';

import { trackKey } from './LyricsPanel';

const BARS = 170;
const cache = new Map<string, number[] | null>();

/** Peak levels of the current track, decoded once by the core and cached. */
function useWaveform(): number[] | null {
  const track = usePlayer((s) => s.track);
  const key = trackKey(track);
  const [state, setState] = useState<{ key: string | null; peaks: number[] | null }>({ key: null, peaks: null });

  useEffect(() => {
    if (!key) return;
    if (cache.has(key)) {
      setState({ key, peaks: cache.get(key)! });
      return;
    }
    let cancelled = false;
    void playerApi
      .waveform(BARS)
      .catch(() => null)
      .then((peaks) => {
        cache.set(key, peaks);
        if (!cancelled) setState({ key, peaks });
      });
    return () => {
      cancelled = true;
    };
  }, [key]);

  return state.key === key ? state.peaks : null;
}

/** Bar height (px) for a peak (0–255): a dB scale reads like the music. */
function barHeight(peak: number): number {
  const db = 20 * Math.log10(Math.max(peak, 1) / 255);
  return 4 + Math.max(0, (db + 36) / 36) * 56;
}

/** Seek over the track's waveform (Now Playing board): click or drag to seek. */
export function WaveformSeek() {
  const positionMs = usePlayer((s) => s.positionMs);
  const durationMs = usePlayer((s) => s.durationMs);
  const status = usePlayer((s) => s.status);
  const peaks = useWaveform();
  const area = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [scrub, setScrub] = useState<number | null>(null);

  const duration = durationMs ?? 0;
  const enabled = duration > 0 && isSeekable(status);
  const fraction = scrub ?? (duration > 0 ? Math.min(1, positionMs / duration) : 0);
  const shownMs = scrub != null ? scrub * duration : positionMs;

  const at = (clientX: number) => {
    const r = area.current!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width));
  };
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (!enabled || e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    setScrub(at(e.clientX));
  };
  const onPointerUp = (e: PointerEvent<HTMLDivElement>) => {
    if (scrub == null) return;
    const f = at(e.clientX);
    setScrub(null);
    void run(playerApi.seek(f * duration));
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!enabled || e.ctrlKey || e.altKey || e.metaKey) return;
    const step = e.shiftKey ? 30_000 : 5_000;
    const moves: Record<string, number> = {
      ArrowRight: positionMs + step,
      ArrowLeft: positionMs - step,
      Home: 0,
      End: duration - 1000,
    };
    const next = moves[e.key];
    if (next === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    void run(playerApi.seek(Math.max(0, Math.min(duration, next))));
  };

  const played = Math.round(fraction * BARS);
  const marker = scrub ?? hover;

  return (
    <div className="flex flex-col gap-2">
      <div
        ref={area}
        className={enabled ? 'wave' : 'wave disabled'}
        role="slider"
        tabIndex={enabled ? 0 : -1}
        aria-label="Seek"
        aria-valuemin={0}
        aria-valuemax={Math.round(duration / 1000)}
        aria-valuenow={Math.round(shownMs / 1000)}
        aria-valuetext={`${formatTime(shownMs)} of ${formatTime(duration)}`}
        onPointerDown={onPointerDown}
        onPointerMove={(e) => {
          if (!enabled) return;
          const f = at(e.clientX);
          setHover(f);
          if (scrub != null) setScrub(f);
        }}
        onPointerUp={onPointerUp}
        onPointerCancel={() => setScrub(null)}
        onPointerLeave={() => setHover(null)}
        onKeyDown={onKeyDown}
      >
        {Array.from({ length: BARS }, (_, i) => (
          <span
            key={i}
            className={i < played ? 'wb p' : 'wb'}
            style={{ height: peaks ? barHeight(peaks[i] ?? 0) : 4, transitionDelay: peaks ? `${(i * 2) % 340}ms` : '0ms' }}
          />
        ))}
        <span className="wave-head" style={{ left: `${fraction * 100}%` }} />
        {marker != null && enabled && (
          <>
            <span className="wave-mark" style={{ left: `${marker * 100}%` }} />
            <span className="wave-bubble mono" style={{ left: `${marker * 100}%` }}>
              {formatTime(marker * duration)}
            </span>
          </>
        )}
      </div>
      <div className="mono flex justify-between text-[12px]">
        <span>{formatTime(shownMs)}</span>
        <span className="t3">{duration > 0 ? `−${formatTime(Math.max(0, duration - shownMs))}` : '–:––'}</span>
      </div>
    </div>
  );
}
