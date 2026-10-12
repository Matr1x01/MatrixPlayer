// The processing chain as it stands, stage by stage, from what the core
// reports: the negotiated formats, the DSP settings and its live status.

import { backendLabel, codecLabel, formatStreamCompact, khz } from '@/features/player/format';
import { activeCurve, useDsp } from '@/stores/dsp';
import { usePlayer } from '@/stores/player';
import type { CrossfeedPreset, DspSettings, DspStatus } from '@/types/dsp';

import { formatGain } from './eqMath';

export type StageState = 'on' | 'active' | 'off' | 'warn';

export interface ChainStage {
  key: 'decode' | 'replayGain' | 'eq' | 'tone' | 'crossfeed' | 'limiter' | 'resampler' | 'output';
  name: string;
  value: string;
  state: StageState;
}

export const STAGE_DOT: Record<StageState, string> = {
  on: 'var(--ok)',
  active: 'var(--ac)',
  off: 'var(--tx3)',
  warn: 'var(--warn)',
};

const RG_MODE: Record<DspSettings['replayGain'], string> = { off: 'Off', track: 'Track', album: 'Album', smart: 'Smart' };

/** bs2b's presets: the cross-path cut-off and how far below the direct sound it is. */
export const CROSSFEED_PRESETS: { value: CrossfeedPreset; label: string; detail: string }[] = [
  { value: 'default', label: 'Default', detail: '700 Hz · 4.5 dB' },
  { value: 'chuMoy', label: 'Chu Moy', detail: '700 Hz · 6 dB' },
  { value: 'janMeier', label: 'Jan Meier', detail: '650 Hz · 9.5 dB' },
];

export function crossfeedDetail(preset: CrossfeedPreset): string {
  return CROSSFEED_PRESETS.find((p) => p.value === preset)?.detail ?? '';
}

export function replayGainText(settings: DspSettings, status: DspStatus, playing: boolean): string {
  if (settings.replayGain === 'off') return 'Off';
  if (!playing || status.replayGainSource === 'off') return `${RG_MODE[settings.replayGain]} mode`;
  const source = status.replayGainSource === 'untagged' ? 'Untagged' : status.replayGainSource === 'track' ? 'Track' : 'Album';
  return `${source} ${formatGain(status.replayGainDb)}`;
}

export function eqText(settings: DspSettings): string {
  const curve = activeCurve(settings);
  const layout =
    curve.mode === 'graphic10'
      ? '10-band'
      : curve.mode === 'graphic31'
        ? '31-band'
        : `${curve.parametric.filter((b) => b.enabled).length}-band`;
  return `${layout} · ${settings.preset ?? 'Custom'}`;
}

/** Tone and stereo adjustments in effect, e.g. "Bass +2 · W 115%". */
export function toneParts(settings: DspSettings): string[] {
  const curve = activeCurve(settings);
  const eqOn = settings.eqEnabled && !settings.bypass;
  const trim = (db: number) => formatGain(db).replace(' dB', '').replace('.0', '');
  const parts: string[] = [];
  if (eqOn && Math.abs(curve.bassDb) > 0.05) parts.push(`Bass ${trim(curve.bassDb)}`);
  if (eqOn && Math.abs(curve.trebleDb) > 0.05) parts.push(`Treble ${trim(curve.trebleDb)}`);
  if (Math.abs(settings.width - 1) > 0.005) parts.push(`W ${Math.round(settings.width * 100)}%`);
  if (Math.abs(settings.balance) > 0.005) {
    parts.push(`Bal ${settings.balance < 0 ? 'L' : 'R'}${Math.round(Math.abs(settings.balance) * 100)}`);
  }
  return parts;
}

export function useChain(): ChainStage[] {
  const track = usePlayer((s) => s.track);
  const signal = usePlayer((s) => s.signal);
  const status = usePlayer((s) => s.status);
  const settings = useDsp((s) => s.settings);
  const dspStatus = useDsp((s) => s.status);
  const playing = status === 'playing' || status === 'paused' || status === 'seeking';

  const decode = track
    ? [
        codecLabel(track.properties),
        signal.decoded ? formatStreamCompact(signal.decoded) : track.properties.sampleRate && `${khz(track.properties.sampleRate)} kHz`,
      ]
        .filter(Boolean)
        .join(' · ')
    : 'Idle';
  const resampled = signal.decoded && signal.output && signal.decoded.sampleRate !== signal.output.sampleRate;
  const outputName = signal.device?.name ?? backendLabel(signal.backend);

  const stages: ChainStage[] = [{ key: 'decode', name: 'Decode', value: decode, state: track ? 'on' : 'off' }];
  if (settings) {
    const eqOn = settings.eqEnabled && !settings.bypass;
    const tone = toneParts(settings);
    stages.push(
      {
        key: 'replayGain',
        name: 'ReplayGain',
        value: replayGainText(settings, dspStatus, playing),
        state: settings.replayGain === 'off' ? 'off' : 'on',
      },
      {
        key: 'eq',
        name: 'Equalizer',
        value: !settings.eqEnabled ? 'Off' : settings.bypass ? 'Bypassed' : eqText(settings),
        state: eqOn ? 'active' : 'off',
      },
      { key: 'tone', name: 'Tone & space', value: tone.length ? tone.join(' · ') : 'Neutral', state: tone.length ? 'active' : 'off' },
      {
        key: 'crossfeed',
        name: 'Crossfeed',
        value: settings.crossfeed ? `On · ${crossfeedDetail(settings.crossfeedPreset)}` : 'Bypassed',
        state: settings.crossfeed ? 'active' : 'off',
      },
      {
        key: 'limiter',
        name: 'Limiter',
        value: settings.limiter
          ? dspStatus.limiting
            ? 'Limiting'
            : 'Idle · −0.5 dBTP'
          : dspStatus.clipping
            ? 'Off · clipping'
            : 'Off',
        state: dspStatus.limiting || dspStatus.clipping ? 'warn' : settings.limiter ? 'on' : 'off',
      },
    );
  }
  if (signal.bitPerfect) {
    // Nothing in MatrixPlayer changes a sample: every processing stage is
    // out of the path (its settings are kept for afterwards).
    for (const stage of stages) {
      if (stage.key !== 'decode') Object.assign(stage, { value: 'Bypassed · bit-perfect', state: 'off' });
    }
  }
  // Through PipeWire, it may resample after us (the verdict says).
  const serverResampled = signal.verdict.state === 'resampled' ? signal.verdict : null;
  stages.push(
    {
      key: 'resampler',
      name: 'Resampler',
      value: resampled
        ? `${khz(signal.decoded!.sampleRate)} → ${khz(signal.output!.sampleRate)} kHz`
        : serverResampled
          ? `PipeWire · ${khz(serverResampled.from)} → ${khz(serverResampled.to)} kHz`
          : 'Bypassed · native',
      // A warning only where it breaks bit-perfect playback.
      state: resampled || serverResampled ? (signal.bitPerfect ? 'warn' : 'active') : 'off',
    },
    {
      key: 'output',
      name: 'Output',
      value: `${signal.output ? `${formatStreamCompact(signal.output)} · ` : ''}${outputName}${signal.exclusive ? ' · exclusive' : ''}`,
      state: signal.output ? 'on' : 'off',
    },
  );
  return stages;
}

/** Short list of what is processing the sound, e.g. "EQ, RG, tone". */
export function processingSummary(settings: DspSettings | null, bitPerfect = false): string {
  if (bitPerfect) return 'None · bit-perfect';
  if (!settings) return '64-bit float';
  const parts: string[] = [];
  if (settings.eqEnabled && !settings.bypass) parts.push('EQ');
  if (settings.replayGain !== 'off') parts.push('RG');
  if (toneParts(settings).length) parts.push('tone');
  if (settings.crossfeed) parts.push('crossfeed');
  return parts.length ? `64-bit float · ${parts.join(', ')}` : '64-bit float · volume only';
}
