import { Fragment, useCallback, useEffect, useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import type { IconName } from '@/components/icons/paths';
import { Segmented } from '@/components/ui/Segmented';
import { Toggle } from '@/components/ui/Toggle';
import { backendLabel, channelLabel, codecLabel, formatKbps, formatStream, khz } from '@/features/player/format';
import { useAnalysis } from '@/services/analysis';
import { dspApi, outputApi, playerApi } from '@/services/ipc';
import { patchDsp, useDsp } from '@/stores/dsp';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';
import type { DeviceKind, OutputDevices, ReplayGainMode } from '@/types/dsp';

import { CenterSlider } from './controls';
import { STAGE_DOT, processingSummary, replayGainText, useChain, type ChainStage } from './chain';
import { formatGain } from './eqMath';

const DEVICE_ICON: Record<DeviceKind, IconName> = {
  usb: 'hp',
  headphones: 'hp',
  bluetooth: 'bt',
  hdmi: 'tv',
  internal: 'speaker',
  other: 'speaker',
};

const RG_HINT: Record<ReplayGainMode, string> = {
  off: 'Tracks play at their mastered level',
  track: 'Every track at the same loudness',
  album: 'Album mode keeps intended dynamics',
  smart: 'Album gain in order, track gain on shuffle',
};

function Setting({
  id,
  title,
  hint,
  children,
  column,
}: {
  id?: string;
  title: string;
  hint?: string;
  children: React.ReactNode;
  column?: boolean;
}) {
  return (
    <div id={id} className={column ? 'set col' : 'set'} tabIndex={id ? -1 : undefined}>
      <div className="l">
        <b>{title}</b>
        {hint && <span>{hint}</span>}
      </div>
      {children}
    </div>
  );
}

/** Scale marks, equally spaced: the top of the meter is expanded. */
const METER_MARKS = [-48, -36, -24, -18, -12, -6, -3, 0];

/** Position (%) of a level on the meter scale. */
function meterPos(db: number): number {
  if (db <= METER_MARKS[0]!) return 0;
  for (let i = 1; i < METER_MARKS.length; i++) {
    const [lo, hi] = [METER_MARKS[i - 1]!, METER_MARKS[i]!];
    if (db <= hi) return ((i - 1 + (db - lo) / (hi - lo)) / (METER_MARKS.length - 1)) * 100;
  }
  return 100;
}

/** Horizontal L/R level meters: RMS fill, peak tick, peak readout. */
export function LevelMeters({ large }: { large?: boolean }) {
  const frame = useAnalysis();
  return (
    <div className={large ? 'meters large' : 'meters'}>
      {(['L', 'R'] as const).map((ch, i) => (
        <div key={ch} className="meter-row">
          <span className="mono t3 meter-ch">{ch}</span>
          <div className="meter">
            <span className="meter-fill" style={{ width: `${meterPos(frame.rmsDb[i]!)}%` }} />
            <span className="meter-peak" style={{ left: `${meterPos(frame.peakDb[i]!)}%`, opacity: frame.peakDb[i]! > -60 ? 1 : 0 }} />
          </div>
          <span className="mono meter-db">{frame.peakDb[i]! > -89 ? frame.peakDb[i]!.toFixed(1).replace('-', '−') : '−∞'}</span>
        </div>
      ))}
      {large && (
        <div className="mono t3 meter-scale">
          {METER_MARKS.map((db) => (
            <span key={db}>{db === 0 ? '0 dBFS' : `−${-db}`}</span>
          ))}
        </div>
      )}
    </div>
  );
}

/** Signal chain, output device, playback processing and audio information (Audio / DSP board). */
export function DspView() {
  const settings = useDsp((s) => s.settings);
  const dspStatus = useDsp((s) => s.status);
  const track = usePlayer((s) => s.track);
  const signal = usePlayer((s) => s.signal);
  const status = usePlayer((s) => s.status);
  const gapless = usePlayer((s) => s.queue.gapless);
  const navigate = useNavigation((s) => s.navigate);
  const chain = useChain();
  const [devices, setDevices] = useState<OutputDevices | null>(null);
  const [probing, setProbing] = useState(false);
  const [switching, setSwitching] = useState<string | null | undefined>(undefined);

  const probe = useCallback(async () => {
    setProbing(true);
    const found = await run(outputApi.devices());
    if (found) setDevices(found);
    setProbing(false);
  }, []);
  useEffect(() => void probe(), [probe]);

  const choose = async (id: string | null) => {
    setSwitching(id);
    await run(outputApi.setDevice(id));
    setSwitching(undefined);
    void probe();
  };

  const jump = (stage: ChainStage) => {
    if (stage.key === 'eq' || stage.key === 'tone') {
      navigate('equalizer');
      return;
    }
    const target = document.getElementById(`dsp-${stage.key}`);
    if (!target) return;
    target.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    target.classList.remove('flash');
    void target.offsetWidth;
    target.classList.add('flash');
    target.focus({ preventScroll: true });
  };

  const playing = status === 'playing' || status === 'paused' || status === 'seeking';
  const resampled = signal.decoded && signal.output && signal.decoded.sampleRate !== signal.output.sampleRate;
  const current = signal.device?.id ?? null;
  const pickable = signal.backend === 'pipeWire' || signal.backend === 'pulseAudio' || signal.backend === 'alsa';

  const info: [string, string, string?][] = track
    ? [
        ['Codec', `${codecLabel(track.properties)}${track.properties.lossless ? ' (lossless)' : ''}`],
        ['Container', `${track.properties.container}${track.metadata.tagFormat ? ` · ${track.metadata.tagFormat}` : ''}`],
        ['Sample rate', track.properties.sampleRate ? `${track.properties.sampleRate.toLocaleString('en-US')} Hz` : '—'],
        ['Bit depth', track.properties.bitDepth ? `${track.properties.bitDepth}-bit` : 'n/a (lossy)'],
        ['Bitrate', track.properties.bitrateKbps ? formatKbps(track.properties.bitrateKbps) : '—'],
        ['Channels', `${track.properties.channels ?? '–'} · ${channelLabel(track.properties.channels).toLowerCase()}`],
        ['Decoded as', signal.decoded ? formatStream(signal.decoded) : '—'],
        ['Output device', signal.device?.name ?? `${backendLabel(signal.backend)} default`],
        ['Output format', signal.output ? formatStream(signal.output) : '—', 'var(--ok)'],
        ['ReplayGain', settings ? replayGainText(settings, dspStatus, playing) : '—'],
        ['Processing', processingSummary(settings), 'var(--ac)'],
        [
          'Resampling',
          resampled ? `${khz(signal.decoded!.sampleRate)} → ${khz(signal.output!.sampleRate)} kHz` : 'None',
          resampled ? 'var(--warn)' : undefined,
        ],
        ['Buffer', signal.bufferMs != null ? `${signal.bufferMs} ms` : '—'],
      ]
    : [];

  return (
    <div className="view">
      <div className="page-header">
        <div className="flex min-w-0 flex-col gap-1">
          <div className="cap">Sound</div>
          <div className="flex min-w-0 items-baseline gap-3">
            <h1 className="page-title">Audio / DSP</h1>
            <span className="mono t3 el text-[11.5px]">Signal chain for the current output</span>
          </div>
        </div>
        <div className="flex flex-none items-center gap-2">
          <span className="chip acc h-6 px-2.5">PROCESSED · 64-BIT FLOAT</span>
          <button
            type="button"
            className="btn gh"
            onClick={async () => {
              const restored = await run(dspApi.restoreDefaults());
              if (restored) useDsp.setState({ settings: restored });
              if (!gapless) void run(playerApi.setGapless(true));
            }}
            data-tip="Resets playback processing; the equalizer is kept"
          >
            Restore defaults
          </button>
        </div>
      </div>

      <div className="view-body flex flex-col gap-4 px-7 pb-5">
        <section className="panel flex flex-col gap-3 px-4 py-3.5" aria-label="Signal chain">
          <div className="flex items-center justify-between gap-3">
            <span className="cap">Signal chain</span>
            <span className="mono t3 el text-[10.5px]">Click a stage to jump to its settings</span>
          </div>
          <div className="chain">
            {chain.map((stage, i) => (
              <Fragment key={stage.key}>
                {i > 0 && <span className="lnk" style={{ '--i': i } as React.CSSProperties} />}
                <button
                  type="button"
                  style={{ '--i': i } as React.CSSProperties}
                  className={stage.state === 'off' ? 'node off' : stage.state === 'active' ? 'node act' : 'node'}
                  data-tip={`${stage.name} · ${stage.value}`}
                  onClick={() => jump(stage)}
                >
                  <span className="flex items-center justify-between gap-1.5">
                    <span className="el text-[12px] font-semibold">{stage.name}</span>
                    <span className="h-1.5 w-1.5 flex-none rounded-full" style={{ background: STAGE_DOT[stage.state] }} />
                  </span>
                  <span className="node-value mono text-[10px]" style={{ color: 'var(--tx2)' }}>
                    {stage.value}
                  </span>
                </button>
              </Fragment>
            ))}
          </div>
        </section>

        <div className="dsp-grid">
          <section id="dsp-output" tabIndex={-1} className="panel dsp-col" aria-label="Output">
            <div className="flex items-center justify-between">
              <span className="text-[14px] font-bold">Output</span>
              <button
                type="button"
                className="ib h-[26px] w-[26px]"
                aria-label="Rescan devices"
                data-tip="Rescan devices"
                disabled={probing}
                onClick={() => void probe()}
              >
                <Icon name="refresh" size={15} className={probing ? 'spin' : undefined} />
              </button>
            </div>
            <div className="flex flex-col gap-0.5" role="radiogroup" aria-label="Output device">
              <DeviceRow
                on={current == null}
                busy={switching === null}
                icon="speaker"
                name="System default"
                detail={`${backendLabel(signal.backend)} · follows the system setting`}
                onPick={() => void choose(null)}
              />
              {devices?.devices.map((d, i) => (
                <DeviceRow
                  key={d.id}
                  order={i + 1}
                  on={current === d.id}
                  busy={switching === d.id}
                  icon={DEVICE_ICON[d.kind]}
                  name={d.name}
                  detail={d.detail}
                  onPick={() => void choose(d.id)}
                />
              ))}
              {devices && !devices.devices.length && (
                <p className="t3 px-2.5 py-1.5 text-[11.5px] [text-wrap:pretty]">
                  {pickable
                    ? `${backendLabel(signal.backend)} lists no other outputs.`
                    : 'Choosing a device needs PipeWire or PulseAudio output.'}
                </p>
              )}
            </div>
            <div className="mt-1 flex flex-col">
              <Setting title="Exclusive access" hint="Not available yet · audio goes through the system mixer">
                <Toggle on={false} disabled onChange={() => {}} label="Exclusive access" />
              </Setting>
              <Setting title="Sample rate" hint={resampled ? 'Resampled for the output' : 'Each track is sent at its own rate'}>
                <span className="mono text-[11.5px]">{signal.output ? `${khz(signal.output.sampleRate)} kHz` : '—'}</span>
              </Setting>
              <Setting title="Output format" hint="Negotiated with the sound server">
                <span className="mono text-[11.5px]">
                  {signal.output ? `${signal.output.bits}-bit${signal.output.float ? ' float' : ''}` : '—'}
                </span>
              </Setting>
              <Setting title="Buffer" hint="Higher is safer, lower reacts faster">
                <span className="mono text-[11.5px]">{signal.bufferMs != null ? `${signal.bufferMs} ms` : '—'}</span>
              </Setting>
            </div>
          </section>

          <section className="panel dsp-col" aria-label="Playback processing">
            <span className="text-[14px] font-bold">Playback</span>
            {settings && (
              <div className="flex flex-col">
                <Setting title="Gapless playback" hint="Seamless joins for live and concept albums">
                  <Toggle on={gapless} onChange={(on) => void run(playerApi.setGapless(on))} label="Gapless playback" />
                </Setting>
                <Setting title="Crossfade" hint="Not available yet · tracks join gaplessly">
                  <Toggle on={false} disabled onChange={() => {}} label="Crossfade" />
                </Setting>
                <Setting id="dsp-replayGain" title="ReplayGain" hint={RG_HINT[settings.replayGain]}>
                  <Segmented<ReplayGainMode>
                    label="ReplayGain mode"
                    size="sm"
                    value={settings.replayGain}
                    onChange={(replayGain) => patchDsp({ replayGain })}
                    options={[
                      { value: 'off', label: 'Off' },
                      { value: 'track', label: 'Track' },
                      { value: 'album', label: 'Album' },
                      { value: 'smart', label: 'Smart' },
                    ]}
                  />
                </Setting>
                <Setting column title="ReplayGain preamp" hint={`Added to every tagged file's gain`}>
                  <div className="flex items-center gap-3">
                    <div className="flex-1">
                      <CenterSlider
                        label="ReplayGain preamp"
                        value={settings.replayGainPreampDb}
                        min={-15}
                        max={15}
                        center={0}
                        step={0.5}
                        reset={0}
                        valueText={formatGain(settings.replayGainPreampDb)}
                        onChange={(v) => patchDsp({ replayGainPreampDb: Math.round(v * 10) / 10 })}
                      />
                    </div>
                    <span className="mono w-[60px] text-right text-[11.5px]">{formatGain(settings.replayGainPreampDb)}</span>
                  </div>
                </Setting>
                <Setting column title="Untagged files" hint="Files without ReplayGain tags; 0 dB plays them as mastered">
                  <div className="flex items-center gap-3">
                    <div className="flex-1">
                      <CenterSlider
                        label="Gain for untagged files"
                        value={settings.replayGainFallbackDb}
                        min={-15}
                        max={6}
                        center={0}
                        step={0.5}
                        reset={0}
                        valueText={formatGain(settings.replayGainFallbackDb)}
                        onChange={(v) => patchDsp({ replayGainFallbackDb: Math.round(v * 10) / 10 })}
                      />
                    </div>
                    <span className="mono w-[60px] text-right text-[11.5px]">{formatGain(settings.replayGainFallbackDb)}</span>
                  </div>
                </Setting>
                <Setting id="dsp-limiter" title="Clipping prevention" hint="Uses ReplayGain peak values; a limiter catches the rest">
                  <Toggle
                    on={settings.clippingPrevention}
                    onChange={(clippingPrevention) => patchDsp({ clippingPrevention })}
                    label="Clipping prevention"
                  />
                </Setting>
                <Setting
                  column
                  title="EQ boost allowance"
                  hint="Boost auto preamp lets through before lowering the level; the limiter catches the peaks"
                >
                  <div className="flex items-center gap-3">
                    <div className="flex-1">
                      <CenterSlider
                        label="EQ boost allowance"
                        disabled={!settings.autoPreamp}
                        value={settings.autoPreampAllowanceDb}
                        min={0}
                        max={12}
                        center={0}
                        step={0.5}
                        reset={3}
                        valueText={formatGain(settings.autoPreampAllowanceDb)}
                        onChange={(v) => patchDsp({ autoPreampAllowanceDb: Math.round(v * 10) / 10 })}
                      />
                    </div>
                    <span className="mono w-[60px] text-right text-[11.5px]">{formatGain(settings.autoPreampAllowanceDb)}</span>
                  </div>
                </Setting>
                <Setting title="Soft fade on pause & seek" hint="120 ms ramp, avoids clicks">
                  <Toggle on={settings.softFades} onChange={(softFades) => patchDsp({ softFades })} label="Soft fade" />
                </Setting>
                <Setting id="dsp-crossfeed" title="Headphone crossfeed" hint="Softens hard-panned mixes">
                  <Toggle on={settings.crossfeed} onChange={(crossfeed) => patchDsp({ crossfeed })} label="Headphone crossfeed" />
                </Setting>
                <Setting title="Dither" hint="Applied only when reducing bit depth">
                  <Toggle on={settings.dither} onChange={(dither) => patchDsp({ dither })} label="Dither" />
                </Setting>
              </div>
            )}
          </section>

          <section className="panel dsp-info" aria-label="Audio information">
            <div className="flex flex-col gap-2.5 border-b border-ln px-4 pb-3 pt-3.5">
              <div className="flex items-center justify-between">
                <span className="cap">Audio information</span>
                {status === 'playing' && (
                  <span className="flex items-center gap-1.5 text-[11px] font-semibold" style={{ color: 'var(--ok)' }}>
                    <span className="live-dot h-1.5 w-1.5 rounded-full" style={{ background: 'var(--ok)' }} />
                    Live
                  </span>
                )}
              </div>
              {track ? (
                <>
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
                  <LevelMeters />
                </>
              ) : (
                <span className="t3 text-[12.5px]">Nothing is playing.</span>
              )}
            </div>
            <div className="flex flex-col px-4 pb-3.5 pt-1.5">
              {info.map(([k, v, color]) => (
                <div key={k} className="kv-row">
                  <span>{k}</span>
                  <span className="el" style={{ color }} title={v}>
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

function DeviceRow({
  order = 0,
  on,
  busy,
  icon,
  name,
  detail,
  onPick,
}: {
  order?: number;
  on: boolean;
  busy: boolean;
  icon: IconName;
  name: string;
  detail: string;
  onPick: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={on}
      className={on ? 'dev on' : 'dev'}
      style={{ '--i': order } as React.CSSProperties}
      onClick={() => !on && onPick()}
      disabled={busy}
    >
      <span className={on ? 'rad on' : 'rad'} />
      <Icon name={icon} size={18} style={{ color: on ? 'var(--ok)' : 'var(--tx2)' }} />
      <span className="flex min-w-0 flex-col gap-px text-left">
        <span className="el font-semibold">{name}</span>
        <span className="el mono t3 text-[10.5px]">{busy ? 'Switching…' : detail}</span>
      </span>
    </button>
  );
}
