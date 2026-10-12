// Mirrors of the Rust DSP and output types (src-tauri/src/dsp, audio/output,
// metadata/lyrics). Rust validates and applies all of it.

import type { OutputBackend } from './player';

export type FilterKind = 'peak' | 'lowShelf' | 'highShelf' | 'lowPass' | 'highPass' | 'notch';

export interface EqBand {
  enabled: boolean;
  kind: FilterKind;
  /** Centre or corner frequency, Hz. */
  freq: number;
  gainDb: number;
  q: number;
}

export type EqMode = 'parametric' | 'graphic10' | 'graphic31';

/** One complete EQ setting; the A and B compare slots each hold one. */
export interface EqCurve {
  mode: EqMode;
  parametric: EqBand[];
  graphic10: number[];
  graphic31: number[];
  /** Low shelf at 100 Hz, dB. */
  bassDb: number;
  /** High shelf at 8 kHz, dB. */
  trebleDb: number;
}

export type ReplayGainMode = 'off' | 'track' | 'album' | 'smart';
/** bs2b's crossfeed presets. */
export type CrossfeedPreset = 'default' | 'chuMoy' | 'janMeier';
export type AbSlot = 'a' | 'b';

export interface DspSettings {
  /** Settings format; the core migrates older saved settings. */
  version: number;
  eqEnabled: boolean;
  slot: AbSlot;
  /** Hear the signal without the EQ and tone controls. */
  bypass: boolean;
  a: EqCurve;
  b: EqCurve;
  /** Preset the active slot was loaded from. */
  preset: string | null;
  preampDb: number;
  /** Lower the level to make room for EQ boosts (off: a boost is heard as set). */
  autoPreamp: boolean;
  /** EQ boost allowed before auto preamp lowers the level (it always takes back the loudness the EQ adds). */
  autoPreampAllowanceDb: number;
  /** Limit the true peak to −0.5 dBTP at the end of the chain (off: nothing is limited, overs clip). */
  limiter: boolean;
  /** −1 (left) … 1 (right). */
  balance: number;
  /** 0 (mono) … 2; 1 leaves the stereo image alone. */
  width: number;
  crossfeed: boolean;
  crossfeedPreset: CrossfeedPreset;
  replayGain: ReplayGainMode;
  replayGainPreampDb: number;
  replayGainFallbackDb: number;
  clippingPrevention: boolean;
  softFades: boolean;
  dither: boolean;
}

/** An EQ curve's response as the core plays it (for drawing). */
export interface EqResponse {
  /** The whole curve (dB) at each frequency asked for. */
  total: number[];
  /** Each parametric band on its own (dB); empty in the graphic modes. */
  bands: number[][];
  /** Largest boost anywhere in the audible range. */
  maxBoostDb: number;
  /** Loudness the curve adds to music; auto preamp takes back at least this. */
  loudnessDb: number;
}

export interface Preset {
  name: string;
  curve: EqCurve;
  builtin: boolean;
}

export type RgSource = 'off' | 'track' | 'album' | 'untagged';

export interface DspStatus {
  replayGainSource: RgSource;
  /** Gain ReplayGain applies now, including its preamp and peak limit. */
  replayGainDb: number;
  /** The limiter caught overs recently. */
  limiting: boolean;
  /** Samples went over full scale recently: the output clips them. */
  clipping: boolean;
  sampleRate: number;
  channels: number;
}

export interface DspState {
  settings: DspSettings;
  /** The user's presets first, then the built-in ones. */
  presets: Preset[];
  status: DspStatus;
}

export interface AnalysisFrame {
  /** 56 log-spaced bands from 30 Hz to 18 kHz, 0 … 1. */
  bands: number[];
  peakDb: [number, number];
  rmsDb: [number, number];
  /** A short stretch of waveform per channel (L, R), −1 … 1. */
  scope: [number[], number[]];
}

export type DeviceKind = 'usb' | 'bluetooth' | 'hdmi' | 'headphones' | 'internal' | 'other';

export interface OutputDevice {
  id: string;
  name: string;
  /** Connection and capabilities, e.g. "USB · up to 32-bit / 384 kHz". */
  detail: string;
  kind: DeviceKind;
  /** Can be used exclusively (it is an ALSA device). */
  exclusive: boolean;
}

export interface OutputDevices {
  backend: OutputBackend;
  devices: OutputDevice[];
  /** Chosen device id; null follows the system default. */
  selected: string | null;
  /** Bit-perfect playback through the system mixer is on. */
  bitPerfect: boolean;
  /** The selected device is held exclusively. */
  exclusive: boolean;
}

export interface LyricLine {
  /** Start time (ms), already offset-corrected; null for plain lyrics. */
  timeMs: number | null;
  text: string;
}

export interface Lyrics {
  source: 'file' | 'embedded' | 'online';
  fileName: string | null;
  /** Where online lyrics came from (LRCLIB), to credit it. */
  provider: string | null;
  /** The track has no words; `lines` is empty. */
  instrumental: boolean;
  synced: boolean;
  offsetMs: number;
  lines: LyricLine[];
}
