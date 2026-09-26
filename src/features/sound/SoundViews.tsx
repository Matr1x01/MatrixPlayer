import { Fragment } from 'react';

import { Icon } from '@/components/icons/Icon';
import { PageHeader } from '@/components/shell/PageHeader';
import { EmptyCard } from '@/components/ui/EmptyState';
import { useSignalStages } from '@/features/now-playing/SignalPath';
import { backendLabel, channelLabel, codecLabel, formatKbps, formatStream, khz } from '@/features/player/format';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';

export function EqualizerView() {
  const navigate = useNavigation((s) => s.navigate);
  return (
    <div className="view">
      <PageHeader section="Sound" title="Equalizer" meta="Off" />
      <div className="view-body flex items-start justify-center border-t border-ln pt-16">
        <EmptyCard
          icon="eq"
          title="The equalizer isn't built yet"
          className="w-[380px]"
          actions={
            <button type="button" className="btn" onClick={() => navigate('dsp')}>
              View the signal chain
            </button>
          }
        >
          Parametric and graphic EQ, presets and per-device profiles arrive with the DSP milestone. Audio already runs through
          Fermata's 64-bit float processing chain, which is where the EQ will be inserted.
        </EmptyCard>
      </div>
    </div>
  );
}

/** Signal chain, output and audio information (Audio / DSP board). */
export function DspView() {
  const track = usePlayer((s) => s.track);
  const signal = usePlayer((s) => s.signal);
  const stages = useSignalStages();
  const nodes = stages.map((stage) => ({ ...stage, off: stage.value === 'Off' }));
  const resampled = signal.decoded && signal.output && signal.decoded.sampleRate !== signal.output.sampleRate;

  const info: [string, string, string?][] = track
    ? [
        ['Codec', `${codecLabel(track.properties)}${track.properties.lossless ? ' (lossless)' : ''}`],
        ['Container', `${track.properties.container}${track.metadata.tagFormat ? ` · ${track.metadata.tagFormat}` : ''}`],
        ['Sample rate', track.properties.sampleRate ? `${track.properties.sampleRate.toLocaleString('en-US')} Hz` : '—'],
        ['Bit depth', track.properties.bitDepth ? `${track.properties.bitDepth}-bit` : 'n/a (lossy)'],
        ['Bitrate', track.properties.bitrateKbps ? formatKbps(track.properties.bitrateKbps) : '—'],
        ['Channels', `${track.properties.channels ?? '–'} · ${channelLabel(track.properties.channels).toLowerCase()}`],
        ['Decoded as', signal.decoded ? formatStream(signal.decoded) : '—'],
        ['Output', backendLabel(signal.backend)],
        ['Output format', signal.output ? formatStream(signal.output) : '—', 'var(--ok)'],
        ['Processing', '64-bit float · volume', 'var(--ac)'],
        [
          'Resampling',
          resampled ? `${khz(signal.decoded!.sampleRate)} → ${khz(signal.output!.sampleRate)} kHz` : 'None',
          resampled ? 'var(--warn)' : undefined,
        ],
      ]
    : [];

  return (
    <div className="view">
      <PageHeader
        section="Sound"
        title="Audio / DSP"
        meta="Signal chain for the current output"
        actions={<span className="chip acc h-6 px-2.5">PROCESSED · 64-BIT FLOAT</span>}
      />
      <div className="view-body flex flex-col gap-4 px-7 pb-5">
        <section className="panel flex flex-col gap-3 px-4 py-3.5" aria-label="Signal chain">
          <div className="flex items-center justify-between">
            <span className="cap">Signal chain</span>
            <span className="mono t3 text-[10.5px]">Stages marked off are not implemented yet</span>
          </div>
          <div className="flex items-center">
            {nodes.map((node, i) => (
              <Fragment key={node.key}>
                {i > 0 && <span className="lnk" />}
                <div className={node.off ? 'node off' : node.key === 'Processing' ? 'node act' : 'node'}>
                  <span className="flex items-center justify-between gap-1.5">
                    <span className="el text-[12px] font-semibold">{node.key}</span>
                    <span className="h-1.5 w-1.5 flex-none rounded-full" style={{ background: node.dot }} />
                  </span>
                  <span className="mono el text-[10px]" style={{ color: 'var(--tx2)' }}>
                    {node.value}
                  </span>
                </div>
              </Fragment>
            ))}
          </div>
        </section>

        <div className="grid min-h-0 grid-cols-[minmax(0,1fr)_360px] gap-4">
          <section className="panel flex flex-col gap-2.5 px-4 py-3.5" aria-label="Output">
            <span className="text-[14px] font-bold">Output</span>
            <div className="dev on">
              <span className="rad on" />
              <Icon name="hp" style={{ color: 'var(--ok)' }} />
              <span className="flex min-w-0 flex-col gap-px">
                <span className="el font-semibold">{backendLabel(signal.backend)} — default output</span>
                <span className="el mono t3 text-[10.5px]">{signal.output ? formatStream(signal.output) : 'Idle'}</span>
              </span>
            </div>
            <p className="t3 text-[12px] [text-wrap:pretty]">
              Fermata plays through the system's default output. Choosing a device, exclusive access and sample-rate matching
              arrive with the output-device work in a later milestone.
            </p>
          </section>

          <section className="panel dsp-info" aria-label="Audio information">
            <div className="flex flex-col gap-2.5 border-b border-ln px-4 pb-3 pt-3.5">
              <div className="flex items-center justify-between">
                <span className="cap">Audio information</span>
                {track && (
                  <span className="flex items-center gap-1.5 text-[11px] font-semibold" style={{ color: 'var(--ok)' }}>
                    <span className="h-1.5 w-1.5 rounded-full" style={{ background: 'var(--ok)' }} />
                    Live
                  </span>
                )}
              </div>
              {track ? (
                <div className="flex items-baseline gap-2.5">
                  <span className="mono text-[30px] font-semibold tracking-[-0.02em]">
                    {track.properties.bitDepth ?? '–'}
                    <span className="t3 text-[18px]">/</span>
                    {track.properties.sampleRate ? khz(track.properties.sampleRate) : '–'}
                  </span>
                  <span className="t2 text-[12px]">bit · kHz</span>
                  {track.properties.lossless && (
                    <span className={(track.properties.bitDepth ?? 0) >= 24 ? 'chip hr ml-auto' : 'chip ml-auto'}>
                      {(track.properties.bitDepth ?? 0) >= 24 ? 'HI-RES LOSSLESS' : 'LOSSLESS'}
                    </span>
                  )}
                </div>
              ) : (
                <span className="t3 text-[12.5px]">Nothing is playing.</span>
              )}
            </div>
            <div className="flex flex-col px-4 pb-3.5 pt-1.5">
              {info.map(([k, v, color]) => (
                <div key={k} className="kv-row">
                  <span>{k}</span>
                  <span className="el" style={{ color }}>
                    {v}
                  </span>
                </div>
              ))}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
