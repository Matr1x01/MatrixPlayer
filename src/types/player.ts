// Mirrors of the Rust player types (src-tauri/src/player, audio, metadata).
// Rust owns all of this state; the frontend only renders it.

import type { OutputDevice } from './dsp';

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
  /** Fields MatrixPlayer inferred rather than read from the file's tags. */
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

export interface LibraryRef {
  trackId: number;
  albumId: number | null;
  artistId: number | null;
  folderId: number;
}

export interface TrackInfo {
  /** Where the track sits in the library, when it is indexed. */
  library: LibraryRef | null;
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
  /** Queued with "play next" / "add to queue" rather than started. */
  added: boolean;
  /** Could not be played the last time it was tried. */
  failed: boolean;
}

export interface QueueSnapshot {
  /** In play order. */
  entries: QueueEntry[];
  currentIndex: number | null;
  shuffle: boolean;
  repeat: RepeatMode;
  /** Tracks continue into the next without a gap. */
  gapless: boolean;
}

export interface VolumeState {
  /** Slider position, 0–1. */
  level: number;
  muted: boolean;
  /** Applied gain in dB; null means silence. */
  gainDb: number | null;
  /** Bit-perfect playback fixes the volume at 100% (set it on the device). */
  locked: boolean;
}

export interface StreamFormat {
  sampleFormat: string;
  bits: number;
  float: boolean;
  sampleRate: number;
  channels: number;
}

export type OutputBackend = 'pipeWire' | 'pulseAudio' | 'alsa' | 'auto' | 'null';

/** Whether the samples reach the device unchanged right now, and if not, why. */
export type BitPerfectVerdict =
  | { state: 'off' }
  | { state: 'pending' }
  /** `shared`: through PipeWire, so while no other app plays and its volume is at 100%. */
  | { state: 'yes'; shared: boolean }
  | { state: 'resampled'; from: number; to: number }
  | { state: 'truncated'; from: number; to: number }
  | { state: 'remixed'; from: number; to: number }
  /** A lossy file's float samples rounded to integers for the device. */
  | { state: 'fromFloat' };

/** PipeWire's clock settings (rates it may switch to for a stream). */
export interface PipeWireClock {
  rate: number;
  allowedRates: number[];
  forceRate: number;
}

export interface SignalPath {
  backend: OutputBackend;
  /** The chosen output device; null follows the system default. */
  device: OutputDevice | null;
  /** The output's buffer, when the sink reports one. */
  bufferMs: number | null;
  decoded: StreamFormat | null;
  output: StreamFormat | null;
  /** The device's hardware is held exclusively. */
  exclusive: boolean;
  /** Bit-perfect playback is on (always while exclusive). */
  bitPerfect: boolean;
  verdict: BitPerfectVerdict;
  pipewireClock: PipeWireClock | null;
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
