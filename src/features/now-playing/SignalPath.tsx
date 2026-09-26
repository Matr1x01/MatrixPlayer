import { backendLabel, codecLabel, formatDb, formatStreamCompact, khz } from '@/features/player/format';
import { usePlayer } from '@/stores/player';

interface Stage {
  key: string;
  value: string;
  dot: string;
}

/**
 * Every stage between the file and the device, from what the pipeline
 * actually negotiated. ReplayGain and EQ are listed as off until the DSP
 * milestone implements them.
 */
export function useSignalStages(): Stage[] {
  const track = usePlayer((s) => s.track);
  const signal = usePlayer((s) => s.signal);
  const volume = usePlayer((s) => s.volume);

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

  const gain = volume.muted ? 'muted' : formatDb(volume.gainDb);
  const channels = (n: number) => (n === 2 ? 'stereo' : `${n} ch`);
  return [
    { key: 'Source', value: source, dot: 'var(--tx2)' },
    {
      key: 'Decoded',
      value: signal.decoded ? `${formatStreamCompact(signal.decoded)} · ${channels(signal.decoded.channels)}` : '—',
      dot: 'var(--tx2)',
    },
    { key: 'ReplayGain', value: 'Off', dot: 'var(--tx3)' },
    { key: 'Equalizer', value: 'Off', dot: 'var(--tx3)' },
    { key: 'Processing', value: `64-bit float · ${gain}`, dot: 'var(--ac)' },
    {
      key: 'Output',
      value: signal.output
        ? `${backendLabel(signal.backend)} · ${formatStreamCompact(signal.output)}`
        : backendLabel(signal.backend),
      dot: 'var(--ok)',
    },
  ];
}

export function SignalPath() {
  const stages = useSignalStages();
  const volume = usePlayer((s) => s.volume);
  const unity = !volume.muted && volume.level >= 1;

  return (
    <section className="np-signal" aria-label="Signal path">
      <div className="flex items-center justify-between">
        <span className="cap">Signal path</span>
        {unity ? <span className="chip ok">Unity gain</span> : <span className="chip acc">Processed</span>}
      </div>
      <div className="np-signal-list">
        {stages.map((stage) => (
          <div key={stage.key} className="np-signal-row">
            <span className="np-signal-dot" style={{ background: stage.dot }} />
            <span className="t3 text-[11.5px]">{stage.key}</span>
            <span className="mono el text-[10.5px]">{stage.value}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
