// Mirrors of the Rust player types (src-tauri/src/player, audio, metadata).
// Rust owns all of this state; the frontend only renders it.

export type PlaybackStatus = 'idle' | 'loading' | 'playing' | 'paused' | 'seeking' | 'stopped' | 'error';
export type RepeatMode = 'off' | 'all' | 'one';
export type InsertMode = 'replace' | 'next' | 'append';

export interface ReplayGain {
  trackGain: number | null;
  trackPeak: number | null;
  albumGain: number | null;
  albumPeak: number | null;
}

export interface TrackMetadata {
  title: string;
  artist: string | null;
  album: string | null;
  albumArtist: string | null;
  genre: string | null;
  year: number | null;
  trackNumber: number | null;
  trackTotal: number | null;
  discNumber: number | null;
  discTotal: number | null;
  composer: string | null;
  comment: string | null;
  copyright: string | null;
  replayGain: ReplayGain;
  tagFormat: string | null;
  tagItemCount: number;
  /** Fields Fermata inferred rather than read from the file's tags. */
  derivedFields: string[];
}

export interface AudioProperties {
  codec: string;
  container: string;
  lossless: boolean;
  sampleRate: number | null;
  bitDepth: number | null;
  channels: number | null;
  bitrateKbps: number | null;
  durationMs: number;
  fileSize: number;
}

export type Rgb = [number, number, number];

export interface ArtworkInfo {
  id: string;
  width: number;
  height: number;
  source: 'embedded' | 'folder';
  palette: [Rgb, Rgb, Rgb];
}

export interface TrackInfo {
  displayPath: string;
  fileName: string;
  metadata: TrackMetadata;
  properties: AudioProperties;
  artwork: ArtworkInfo | null;
  probeError: string | null;
}

export interface QueueEntry {
  id: number;
  track: TrackInfo;
}

export interface QueueSnapshot {
  /** In play order. */
  entries: QueueEntry[];
  currentIndex: number | null;
  shuffle: boolean;
  repeat: RepeatMode;
}

export interface VolumeState {
  /** Slider position, 0–1. */
  level: number;
  muted: boolean;
  /** Applied gain in dB; null means silence. */
  gainDb: number | null;
}

export interface StreamFormat {
  sampleFormat: string;
  bits: number;
  float: boolean;
  sampleRate: number;
  channels: number;
}

export type OutputBackend = 'pipeWire' | 'pulseAudio' | 'alsa' | 'auto';

export interface SignalPath {
  backend: OutputBackend;
  decoded: StreamFormat | null;
  output: StreamFormat | null;
}

export type PlayerErrorKind = 'notFound' | 'unreadable' | 'unsupportedFormat' | 'corrupt' | 'outputUnavailable' | 'other';

export interface PlayerError {
  kind: PlayerErrorKind;
  message: string;
  detail: string | null;
  fileName: string | null;
}

export interface PlayerSnapshot {
  status: PlaybackStatus;
  track: TrackInfo | null;
  positionMs: number;
  durationMs: number | null;
  volume: VolumeState;
  queue: QueueSnapshot;
  signal: SignalPath;
  error: PlayerError | null;
}

/** Error shape of every rejected command. */
export interface CommandError {
  kind: string;
  message: string;
}
