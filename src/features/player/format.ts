// Presentation of audio facts, following the design's badge and readout
// conventions. Pure functions of data received from the core.

import type { AudioProperties, OutputBackend, StreamFormat, TrackInfo } from '@/types/player';

/** m:ss, or h:mm:ss for an hour or more. */
export function formatTime(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return '–:––';
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** "3 h 12 min", "38 min", "45 s". */
export function formatSpan(ms: number): string {
  const minutes = Math.round(ms / 60000);
  if (ms < 60000) return `${Math.round(ms / 1000)} s`;
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

/** 44100 → "44.1", 96000 → "96". */
export function khz(rate: number): string {
  const k = rate / 1000;
  return Number.isInteger(k) ? String(k) : k.toFixed(1).replace(/\.0$/, '');
}

/** "96.0 kHz" as in the Now Playing chips. */
export function formatRate(rate: number): string {
  return `${(rate / 1000).toFixed(1)} kHz`;
}

export function formatKbps(kbps: number): string {
  return `${kbps.toLocaleString('en-US')} kbps`;
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${Math.round(bytes / 1e3)} KB`;
  return `${bytes} B`;
}

export function formatDb(db: number | null, digits = 1): string {
  if (db == null) return '−∞ dB';
  const rounded = Number(db.toFixed(digits));
  const sign = rounded > 0 ? '+' : rounded < 0 ? '−' : '';
  return `${sign}${Math.abs(rounded).toFixed(digits)} dB`;
}

export function channelLabel(channels: number | null): string {
  switch (channels) {
    case null:
      return '–';
    case 1:
      return 'Mono';
    case 2:
      return 'Stereo';
    case 6:
      return '5.1';
    case 8:
      return '7.1';
    default:
      return `${channels} ch`;
  }
}

/** Codec name for badges; PCM is named after its container (WAV, AIFF). */
export function codecLabel(props: AudioProperties): string {
  if (props.codec === 'PCM') return props.container;
  return props.codec.toUpperCase();
}

/** "FLAC 24/96", "MP3 320", "OPUS 160" (Components · B10). */
export function formatBadge(props: AudioProperties): string {
  const codec = codecLabel(props);
  if (props.lossless && props.bitDepth && props.sampleRate) {
    return `${codec} ${props.bitDepth}/${khz(props.sampleRate)}`;
  }
  if (props.bitrateKbps) return `${codec} ${props.bitrateKbps}`;
  return codec;
}

/** Accent badges are reserved for 24-bit and high-rate material. */
export function isHiRes(props: AudioProperties): boolean {
  return (props.bitDepth ?? 0) >= 24 || (props.sampleRate ?? 0) > 48000;
}

export function backendLabel(backend: OutputBackend): string {
  switch (backend) {
    case 'pipeWire':
      return 'PipeWire';
    case 'pulseAudio':
      return 'PulseAudio';
    case 'alsa':
      return 'ALSA';
    case 'auto':
      return 'System output';
  }
}

/** "F32LE · 96 kHz · 2 ch" for signal path readouts. */
export function formatStream(format: StreamFormat): string {
  const depth = format.float ? `${format.bits}-bit float` : `${format.bits}-bit`;
  return `${depth} / ${khz(format.sampleRate)} kHz · ${format.channels} ch`;
}

/** "24/96" for integer PCM, "32f/96" for float, as on signal-chain nodes. */
export function formatStreamCompact(format: StreamFormat): string {
  return `${format.bits}${format.float ? 'f' : ''}/${khz(format.sampleRate)}`;
}

/** "Artist · Album", tolerating missing tags. */
export function artistAlbum(track: TrackInfo): string {
  const artist = track.metadata.artist ?? 'Unknown artist';
  return track.metadata.album ? `${artist} · ${track.metadata.album}` : artist;
}
