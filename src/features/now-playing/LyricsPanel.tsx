import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useSeekTarget } from '@/features/player/hooks';
import { playerApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import { usePreferences } from '@/stores/preferences';
import { errorMessage, run } from '@/stores/toasts';
import type { Lyrics } from '@/types/dsp';
import type { TrackInfo } from '@/types/player';

/** Identifies a track across events (the same file may be queued twice). */
export function trackKey(track: TrackInfo | null): string | null {
  return track ? (track.library ? `lib:${track.library.trackId}` : `path:${track.displayPath}`) : null;
}

/** A line is shown as sung this long before its timestamp. */
const LEAD_MS = 150;
/** How far the position is carried on between the core's reports (every 200 ms). */
const MAX_EXTRAPOLATION_MS = 400;

/** Lyrics found so far, per track (and whether online lookup was on). Failures are not kept. */
const cache = new Map<string, Lyrics | null>();

interface Loaded {
  key: string | null;
  lyrics: Lyrics | null;
  /** Why LRCLIB could not be asked; asking again may work. */
  error: string | null;
}

/** Lyrics of the current track, loaded once per track. */
function useLyrics(online: boolean): { lyrics: Lyrics | null; loading: boolean; error: string | null; retry: () => void } {
  const track = usePlayer((s) => s.track);
  const id = trackKey(track);
  const key = id && `${online ? 'online' : 'local'}:${id}`;
  const [state, setState] = useState<Loaded>({ key: null, lyrics: null, error: null });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!key) return;
    if (cache.has(key)) {
      setState({ key, lyrics: cache.get(key)!, error: null });
      return;
    }
    let cancelled = false;
    setState((s) => (s.key === key ? { ...s, error: null } : s));
    playerApi
      .lyrics()
      .then((lyrics) => {
        cache.set(key, lyrics);
        if (!cancelled) setState({ key, lyrics, error: null });
      })
      .catch((err: unknown) => {
        if (!cancelled) setState({ key, lyrics: null, error: errorMessage(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [key, attempt]);

  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  const current = state.key === key;
  return {
    lyrics: current ? state.lyrics : null,
    loading: !current || (state.lyrics == null && state.error == null && !cache.has(key ?? '')),
    error: current ? state.error : null,
    retry,
  };
}

/**
 * The playback position, carried on smoothly between the core's reports
 * while playing, so a line lights up when it is sung rather than up to a
 * report later.
 */
function useSmoothPosition(active: boolean): number {
  const positionMs = usePlayer((s) => s.positionMs);
  const playing = usePlayer((s) => s.status === 'playing');
  // When the report arrived: the position moves on from there.
  const base = useMemo(() => ({ ms: positionMs, at: performance.now() }), [positionMs]);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!active || !playing) return;
    const timer = window.setInterval(() => tick((n) => n + 1), 50);
    return () => window.clearInterval(timer);
  }, [active, playing]);
  if (!playing) return positionMs;
  return base.ms + Math.min(performance.now() - base.at, MAX_EXTRAPOLATION_MS);
}

/** Index of the line being sung at `ms`, or −1 before the first. */
function currentLine(lyrics: Lyrics, ms: number): number {
  let lo = 0;
  let hi = lyrics.lines.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if ((lyrics.lines[mid]!.timeMs ?? 0) <= ms) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return found;
}

function sourceLabel(lyrics: Lyrics): string {
  const kind = lyrics.synced ? 'Synced lyrics' : 'Lyrics';
  if (lyrics.source === 'file') return `${kind} · ${lyrics.fileName ?? '.lrc'}`;
  if (lyrics.source === 'online') return `${kind} · from ${lyrics.provider ?? 'online'}`;
  return `${kind} · embedded in tags`;
}

/** Synced lyrics follow playback (click a line to jump there); plain lyrics scroll. */
export function LyricsPanel() {
  const online = usePreferences((s) => s.prefs.onlineLyrics);
  const { lyrics, loading, error, retry } = useLyrics(online);
  const synced = lyrics?.synced ?? false;
  const positionMs = useSmoothPosition(synced);
  const [targetMs, setTargetMs] = useSeekTarget(usePlayer((s) => s.positionMs));
  const list = useRef<HTMLDivElement>(null);
  const [userScrolled, setUserScrolled] = useState(false);
  const active = lyrics && synced ? currentLine(lyrics, (targetMs ?? positionMs) + LEAD_MS) : -1;

  useEffect(() => {
    if (active < 0 || userScrolled) return;
    const row = list.current?.querySelector<HTMLElement>(`[data-line="${active}"]`);
    row?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  }, [active, userScrolled]);

  useEffect(() => {
    if (!userScrolled) return;
    const id = window.setTimeout(() => setUserScrolled(false), 4000);
    return () => window.clearTimeout(id);
  }, [userScrolled]);

  if (loading) return <div className="lyrics-empty t3">{online ? 'Looking for lyrics, here and on LRCLIB…' : 'Looking for lyrics…'}</div>;
  if (error) {
    return (
      <div className="lyrics-empty t3 flex flex-col items-start gap-3 [text-wrap:pretty]">
        <span>Lyrics could not be looked up online. {error}</span>
        <button type="button" className="btn" onClick={retry}>
          Try again
        </button>
      </div>
    );
  }
  if (!lyrics) {
    return (
      <div className="lyrics-empty t3 [text-wrap:pretty]">
        {online ? 'No lyrics for this track, in its folder, its tags or on LRCLIB. ' : 'No lyrics for this track. '}
        Put a <span className="mono">.lrc</span> file with the same name next to it, or embed lyrics in its tags.
        {!online && ' Looking lyrics up online is off (Settings › Lyrics).'}
      </div>
    );
  }
  if (lyrics.instrumental) {
    return (
      <div className="lyrics-empty t3">
        Instrumental: {lyrics.provider ?? 'the lyrics source'} lists no words for this track.
      </div>
    );
  }

  const offset = lyrics.offsetMs ? ` · offset ${lyrics.offsetMs > 0 ? '+' : '−'}${(Math.abs(lyrics.offsetMs) / 1000).toFixed(2)} s` : '';

  return (
    <div
      ref={list}
      className={synced ? 'lyrics synced' : 'lyrics'}
      onWheel={() => setUserScrolled(true)}
      onTouchMove={() => setUserScrolled(true)}
    >
      {lyrics.lines.map((line, i) => {
        const distance = active < 0 ? 99 : Math.abs(i - active);
        const cls = !synced ? 'ly plain' : i === active ? 'ly now' : i < active ? 'ly past' : distance === 1 ? 'ly next' : 'ly';
        if (!line.text) return <span key={i} data-line={i} className="ly gap" aria-hidden />;
        return synced && line.timeMs != null ? (
          <button
            key={i}
            type="button"
            data-line={i}
            className={cls}
            style={distance > 2 && i > active ? { opacity: Math.max(0.35, 1 - (distance - 2) * 0.18) } : undefined}
            onClick={() => {
              setUserScrolled(false);
              setTargetMs(line.timeMs!);
              void run(playerApi.seek(line.timeMs!));
            }}
          >
            {line.text}
          </button>
        ) : (
          <span key={i} data-line={i} className={cls}>
            {line.text}
          </span>
        );
      })}
      <span className="mono t3 lyrics-source">
        {sourceLabel(lyrics)}
        {offset}
      </span>
    </div>
  );
}
