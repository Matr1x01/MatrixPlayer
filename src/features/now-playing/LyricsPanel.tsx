import { useEffect, useRef, useState } from 'react';

import { playerApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';
import type { Lyrics } from '@/types/dsp';
import type { TrackInfo } from '@/types/player';

/** Identifies a track across events (the same file may be queued twice). */
export function trackKey(track: TrackInfo | null): string | null {
  return track ? (track.library ? `lib:${track.library.trackId}` : `path:${track.displayPath}`) : null;
}

const cache = new Map<string, Lyrics | null>();

/** Lyrics of the current track, loaded once per track. */
function useLyrics(): { lyrics: Lyrics | null; loading: boolean } {
  const track = usePlayer((s) => s.track);
  const key = trackKey(track);
  const [state, setState] = useState<{ key: string | null; lyrics: Lyrics | null }>({ key: null, lyrics: null });

  useEffect(() => {
    if (!key) return;
    if (cache.has(key)) {
      setState({ key, lyrics: cache.get(key)! });
      return;
    }
    let cancelled = false;
    void playerApi
      .lyrics()
      .catch(() => null)
      .then((lyrics) => {
        cache.set(key, lyrics);
        if (!cancelled) setState({ key, lyrics });
      });
    return () => {
      cancelled = true;
    };
  }, [key]);

  return { lyrics: state.key === key ? state.lyrics : null, loading: state.key !== key };
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

/** Synced lyrics follow playback (click a line to jump there); plain lyrics scroll. */
export function LyricsPanel() {
  const { lyrics, loading } = useLyrics();
  const positionMs = usePlayer((s) => s.positionMs);
  const list = useRef<HTMLDivElement>(null);
  const [userScrolled, setUserScrolled] = useState(false);
  const active = lyrics?.synced ? currentLine(lyrics, positionMs + 150) : -1;

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

  if (loading) return <div className="lyrics-empty t3">Looking for lyrics…</div>;
  if (!lyrics) {
    return (
      <div className="lyrics-empty t3 [text-wrap:pretty]">
        No lyrics for this track. Put a <span className="mono">.lrc</span> file with the same name next to it, or embed lyrics
        in its tags.
      </div>
    );
  }

  const source =
    lyrics.source === 'file'
      ? `${lyrics.synced ? 'Synced lyrics' : 'Lyrics'} · ${lyrics.fileName ?? '.lrc'}`
      : `${lyrics.synced ? 'Synced lyrics' : 'Lyrics'} · embedded in tags`;
  const offset = lyrics.offsetMs ? ` · offset ${lyrics.offsetMs > 0 ? '+' : '−'}${(Math.abs(lyrics.offsetMs) / 1000).toFixed(2)} s` : '';

  return (
    <div
      ref={list}
      className={lyrics.synced ? 'lyrics synced' : 'lyrics'}
      onWheel={() => setUserScrolled(true)}
      onTouchMove={() => setUserScrolled(true)}
    >
      {lyrics.lines.map((line, i) => {
        const distance = active < 0 ? 99 : Math.abs(i - active);
        const cls = !lyrics.synced ? 'ly plain' : i === active ? 'ly now' : i < active ? 'ly past' : distance === 1 ? 'ly next' : 'ly';
        if (!line.text) return <span key={i} data-line={i} className="ly gap" aria-hidden />;
        return lyrics.synced && line.timeMs != null ? (
          <button
            key={i}
            type="button"
            data-line={i}
            className={cls}
            style={distance > 2 && i > active ? { opacity: Math.max(0.35, 1 - (distance - 2) * 0.18) } : undefined}
            onClick={() => {
              setUserScrolled(false);
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
        {source}
        {offset}
      </span>
    </div>
  );
}
