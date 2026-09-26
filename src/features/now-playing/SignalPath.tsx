import { useMemo } from 'react';

import { backendLabel, codecLabel, formatDb, formatStreamCompact, khz } from '@/features/player/format';
import { eqText, replayGainText, toneParts } from '@/features/sound/chain';
import { formatGain } from '@/features/sound/eqMath';
import { useDsp } from '@/stores/dsp';
import { usePlayer } from '@/stores/player';

interface Stage {
  key: string;
  value: string;
  dot: string;
}

/**
 * Every stage between the file and the device, from what the pipeline
 * negotiated and what the DSP chain reports.
 */
export function useSignalStages(): { stages: Stage[]; processed: boolean } {
  const track = usePlayer((s) => s.track);
  const signal = usePlayer((s) => s.signal);
  const volume = usePlayer((s) => s.volume);
  const status = usePlayer((s) => s.status);
  const settings = useDsp((s) => s.settings);
  const dspStatus = useDsp((s) => s.status);

  return useMemo(() => {
    const playing = status === 'playing' || status === 'paused' || status === 'seeking';
    const source = track
      ? [
          codecLabel(track.properties),
          track.properties.bitDepth && track.properties.sampleRate
            ? `${track.properties.bitDepth}/${khz(track.properties.sampleRate)}`
            : track.properties.sampleRate && `${khz(track.properties.sampleRate)} kHz`,
          track.properties.channels && `${track.properties.channels} ch`,
        ]
          .filter(Boolean)
          .join(' · ')
      : '—';

    const eqOn = !!settings && settings.eqEnabled && !settings.bypass;
    const rgOn = !!settings && settings.replayGain !== 'off';
    const tone = settings ? toneParts(settings) : [];
    const resampled = signal.decoded && signal.output && signal.decoded.sampleRate !== signal.output.sampleRate;
    const gain = volume.muted ? 'muted' : formatDb(volume.gainDb);
    const processing = [
      '64-bit float',
      tone.length ? tone.join(' · ') : null,
      resampled ? `resampled to ${khz(signal.output!.sampleRate)} kHz` : 'no resample',
    ]
      .filter(Boolean)
      .join(' · ');

    const stages: Stage[] = [
      { key: 'Source', value: source, dot: 'var(--tx2)' },
      { key: 'ReplayGain', value: settings ? replayGainText(settings, dspStatus, playing) : 'Off', dot: rgOn ? 'var(--tx2)' : 'var(--tx3)' },
      {
        key: 'Equalizer',
        value: !settings || !settings.eqEnabled ? 'Off' : settings.bypass ? 'Bypassed' : `${eqText(settings)}${settings.preampDb ? ` · preamp ${formatGain(settings.preampDb)}` : ''}`,
        dot: eqOn ? 'var(--ac)' : 'var(--tx3)',
      },
      { key: 'Processing', value: `${processing} · volume ${gain}`, dot: 'var(--tx2)' },
      {
        key: 'Output',
        value: `${signal.device?.name ?? backendLabel(signal.backend)}${signal.output ? ` · ${formatStreamCompact(signal.output)}` : ''}`,
        dot: 'var(--ok)',
      },
    ];
    const processed = eqOn || rgOn || tone.length > 0 || !!settings?.crossfeed || volume.muted || volume.level < 1;
    return { stages, processed };
  }, [track, signal, volume, status, settings, dspStatus]);
}

export function SignalPath() {
  const { stages, processed } = useSignalStages();

  return (
    <section className="np-signal" aria-label="Signal path">
      <div className="flex items-center justify-between">
        <span className="cap">Signal path</span>
        {processed ? <span className="chip acc">Processed</span> : <span className="chip ok">Unity gain</span>}
      </div>
      <div className="np-signal-list">
        {stages.map((stage) => (
          <div key={stage.key} className="np-signal-row">
            <span className="np-signal-dot" style={{ background: stage.dot }} />
            <span className="t3 text-[11.5px]">{stage.key}</span>
            <span className="mono el text-[10.5px]" title={stage.value}>
              {stage.value}
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}
