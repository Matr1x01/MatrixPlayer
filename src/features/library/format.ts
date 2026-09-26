// Badges and readouts for library rows, matching the player's conventions.

import { formatBadge, isHiRes } from '@/features/player/format';
import type { AlbumRow, TrackRow } from '@/types/library';
import type { AudioProperties } from '@/types/player';

type Format = Pick<AudioProperties, 'codec' | 'lossless' | 'sampleRate' | 'bitDepth' | 'bitrateKbps'>;

function props(f: Format): AudioProperties {
  // PCM badges are named after the container; library rows carry the codec
  // name only, which the scanner already sets to the container for PCM.
  return { container: f.codec, channels: null, durationMs: 0, fileSize: 0, ...f };
}

export function trackBadge(t: TrackRow): string {
  return formatBadge(props(t));
}

export function albumBadge(a: AlbumRow): string | null {
  return a.codec ? formatBadge(props({ ...a, codec: a.codec })) : null;
}

export function badgeClass(f: { codec: string | null; lossless: boolean; sampleRate: number | null; bitDepth: number | null }): string {
  return f.codec && isHiRes(props({ ...f, codec: f.codec, bitrateKbps: null })) ? 'chip hr' : 'chip';
}

/** "Hi-Res" in the design: lossless above CD quality. */
export function isHiResAlbum(a: AlbumRow): boolean {
  return a.lossless && ((a.bitDepth ?? 0) > 16 || (a.sampleRate ?? 0) > 48000);
}

/** "12,408" */
export function count(n: number): string {
  return n.toLocaleString('en-US');
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${count(n)} ${n === 1 ? one : many}`;
}

/** "486.2 GB" with one decimal for library-scale totals. */
export function formatSize(bytes: number): string {
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(2)} TB`;
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
}

/** "486 GB", "1.9 TB": whole units once large, for tight spaces. */
export function formatSizeShort(bytes: number): string {
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`;
  if (bytes >= 1e11) return `${Math.round(bytes / 1e9)} GB`;
  return formatSize(bytes);
}

/** "38 d 4 h", "3 h 51 min", "53:04" for library-scale durations. */
export function formatLength(ms: number): string {
  const minutes = Math.floor(ms / 60000);
  if (minutes >= 24 * 60) return `${Math.floor(minutes / 1440)} d ${Math.floor((minutes % 1440) / 60)} h`;
  if (minutes >= 60) return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** "Today, 21:37", "Yesterday", "12 Mar 2019". */
export function formatDate(seconds: number | null, withTime = false): string {
  if (seconds == null) return '—';
  const date = new Date(seconds * 1000);
  const now = new Date();
  const days = Math.round((startOfDay(now) - startOfDay(date)) / 86_400_000);
  const time = date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  if (days === 0) return withTime ? `Today, ${time}` : 'Today';
  if (days === 1) return withTime ? `Yesterday, ${time}` : 'Yesterday';
  return date.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
}

/** "2 min ago", "3 h ago", or a date. */
export function formatAgo(seconds: number | null): string {
  if (seconds == null) return 'never';
  const diff = Date.now() / 1000 - seconds;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} h ago`;
  return formatDate(seconds);
}

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** A–Z index letter for a sort key: `#` for anything not starting with a letter. */
export function indexLetter(sortKey: string): string {
  const c = sortKey.charAt(0).toUpperCase();
  return c >= 'A' && c <= 'Z' ? c : '#';
}
