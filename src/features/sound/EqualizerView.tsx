import { useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import { contextMenu, openMenu, type MenuItem } from '@/components/ui/ContextMenu';
import { askConfirm, askText } from '@/components/ui/Dialog';
import { Segmented } from '@/components/ui/Segmented';
import { Toggle } from '@/components/ui/Toggle';
import { dspApi } from '@/services/ipc';
import { activeCurve, applyState, patchCurve, patchDsp, setDsp, useDsp } from '@/stores/dsp';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { run, showToast } from '@/stores/toasts';
import type { DspSettings, EqBand, EqCurve, EqMode, FilterKind, Preset } from '@/types/dsp';

import { CenterSlider, CrossfeedPresets, Fader, Knob, ValueField } from './controls';
import {
  GRAPHIC_10,
  GRAPHIC_31,
  KIND_LABEL,
  KIND_SHORT,
  MAX_BANDS,
  MAX_GAIN_DB,
  formatFreq,
  formatGain,
  hasGain,
  parseFreq,
  parseNumber,
  sameCurve,
} from './eqMath';
import { useEqResponse } from './eqResponse';
import { ResponseGraph } from './ResponseGraph';

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));
const round = (v: number, step: number) => Math.round(v / step) * step;

const MODES: { value: EqMode; label: string }[] = [
  { value: 'parametric', label: 'Parametric' },
  { value: 'graphic10', label: 'Graphic · 10' },
  { value: 'graphic31', label: 'Graphic · 31' },
];

/** "8 BANDS", "10-BAND", for preset rows. */
function curveSummary(curve: EqCurve): string {
  if (curve.mode === 'graphic10') return '10-BAND';
  if (curve.mode === 'graphic31') return '31-BAND';
  const n = curve.parametric.filter((b) => b.enabled).length;
  return `${n} BAND${n === 1 ? '' : 'S'}`;
}

function flattened(curve: EqCurve): EqCurve {
  return {
    ...curve,
    parametric: curve.parametric.map((b) => ({ ...b, gainDb: 0 })),
    graphic10: curve.graphic10.map(() => 0),
    graphic31: curve.graphic31.map(() => 0),
    bassDb: 0,
    trebleDb: 0,
  };
}

function withCurve(settings: DspSettings, curve: EqCurve): DspSettings {
  return settings.slot === 'a' ? { ...settings, a: curve } : { ...settings, b: curve };
}

/** Parametric and graphic EQ, tone and stereo controls, presets (Equalizer board). */
export function EqualizerView() {
  const settings = useDsp((s) => s.settings);
  const presets = useDsp((s) => s.presets);
  const limiting = useDsp((s) => s.status.limiting);
  const sampleRate = useDsp((s) => s.status.sampleRate);
  const [selected, setSelected] = useState<number | null>(null);
  const [spectrum, setSpectrum] = useState(true);
  const [filter, setFilter] = useState('');

  const curve = settings ? activeCurve(settings) : null;
  // Solved and designed by the core, at the playing stream's rate.
  const { response, current } = useEqResponse(curve, sampleRate);
  const boost = response?.maxBoostDb ?? 0;

  if (!settings || !curve) {
    return (
      <div className="view">
        <div className="page-header">
          <div className="flex flex-col gap-1">
            <div className="cap">Sound</div>
            <h1 className="page-title">Equalizer</h1>
          </div>
        </div>
      </div>
    );
  }

  const active = settings.eqEnabled && !settings.bypass;
  const allowance = settings.autoPreamp ? settings.autoPreampAllowanceDb : 0;
  const preamp = settings.preampDb - (settings.autoPreamp ? Math.max(0, boost - allowance) : 0);
  // Peaks the allowance lets through are the limiter's by design.
  const mayClip = active && boost + preamp > allowance + 0.05;
  const preset = presets.find((p) => p.name === settings.preset);
  const modified = preset ? !sameCurve(preset.curve, curve) : false;
  const bandSel = selected != null && selected < (curve.mode === 'parametric' ? curve.parametric.length : 99) ? selected : null;

  const updateBand = (index: number, patch: Partial<EqBand>) => {
    patchCurve({ parametric: curve.parametric.map((b, i) => (i === index ? { ...b, ...patch } : b)) });
  };
  const setGraphic = (index: number, gainDb: number) => {
    const key = curve.mode === 'graphic31' ? 'graphic31' : 'graphic10';
    patchCurve({ [key]: curve[key].map((g, i) => (i === index ? gainDb : g)) });
  };
  const addBand = (freq: number, gainDb: number) => {
    if (curve.parametric.length >= MAX_BANDS) {
      showToast({ tone: 'info', message: `The parametric EQ has at most ${MAX_BANDS} bands` });
      return;
    }
    const band: EqBand = { enabled: true, kind: 'peak', freq: Math.round(freq), gainDb: round(gainDb, 0.1), q: 1 };
    const parametric = [...curve.parametric, band].sort((a, b) => a.freq - b.freq);
    patchCurve({ parametric });
    setSelected(parametric.indexOf(band));
  };
  /** Adds a flat band in the widest gap of the spectrum. */
  const addInGap = () => {
    const freqs = [20, ...curve.parametric.map((b) => b.freq).sort((a, b) => a - b), 20000];
    let best = 1000;
    let widest = 0;
    for (let i = 1; i < freqs.length; i++) {
      const gap = Math.log2(freqs[i]! / freqs[i - 1]!);
      if (gap > widest) [widest, best] = [gap, Math.sqrt(freqs[i]! * freqs[i - 1]!)];
    }
    addBand(best, 0);
  };
  const removeBand = (index: number) => {
    if (curve.parametric.length <= 1) return;
    patchCurve({ parametric: curve.parametric.filter((_, i) => i !== index) });
    setSelected(null);
  };

  const loadPreset = (p: Preset) => {
    setDsp({ ...withCurve(settings, structuredClone(p.curve)), preset: p.name, eqEnabled: true, bypass: false });
    setSelected(null);
  };
  const savePreset = async (asNew: boolean) => {
    let name = !asNew && preset && !preset.builtin ? preset.name : null;
    if (!name) {
      name = await askText({
        title: 'Save preset',
        label: 'Name',
        initial: preset && !preset.builtin ? `${preset.name} copy` : '',
        placeholder: 'e.g. Warm headphones',
        confirm: 'Save',
      });
    }
    if (!name) return;
    const state = await run(dspApi.savePreset(name));
    if (state) {
      applyState(state);
      showToast({ tone: 'info', message: `Saved “${name}”` });
    }
  };
  const deletePreset = async (p: Preset) => {
    const ok = await askConfirm({ title: 'Delete preset?', body: `“${p.name}” will be removed.`, confirm: 'Delete', danger: true });
    if (!ok) return;
    const state = await run(dspApi.deletePreset(p.name));
    if (state) applyState(state);
  };
  const reset = () => {
    const flat = flattened(curve);
    const flatPreset = presets.find((p) => p.builtin && p.name === 'Flat');
    setDsp({ ...withCurve(settings, flat), preset: flatPreset && sameCurve(flatPreset.curve, flat) ? 'Flat' : null });
  };
  const importPresets = async () => {
    const n = await run(dspApi.importPresets());
    if (n == null) return;
    applyState(await dspApi.state());
    showToast({ tone: 'info', message: n ? `Imported ${n} ${n === 1 ? 'preset' : 'presets'}` : 'No presets found in that file' });
  };
  const exportPresets = async () => {
    const n = await run(dspApi.exportPresets());
    if (n != null) showToast({ tone: 'info', message: `Exported ${n} ${n === 1 ? 'preset' : 'presets'}` });
  };

  const presetMenu = (anchor: HTMLElement) => {
    const item = (p: Preset): MenuItem => ({
      label: p.name,
      icon: p.name === settings.preset ? 'check' : undefined,
      onSelect: () => loadPreset(p),
    });
    const mine = presets.filter((p) => !p.builtin);
    openMenu(anchor, [...mine.map(item), ...(mine.length ? ['separator' as const] : []), ...presets.filter((p) => p.builtin).map(item)], {
      label: 'Presets',
    });
  };

  const q = filter.trim().toLowerCase();
  const shown = presets.filter((p) => !q || p.name.toLowerCase().includes(q));
  const mine = shown.filter((p) => !p.builtin);
  const builtin = shown.filter((p) => p.builtin);

  const legend = (
    <>
      <div className="flex items-center gap-4 text-[11.5px] t2">
        <span className="flex items-center gap-1.5">
          <span className="h-[2px] w-3.5" style={{ background: 'var(--ac)' }} />
          Response
        </span>
        {bandSel != null && curve.mode === 'parametric' && (
          <span className="flex items-center gap-1.5">
            <span className="w-3.5" style={{ borderTop: '1.5px dashed var(--ac)' }} />
            Band {bandSel + 1}
          </span>
        )}
        <button
          type="button"
          className={spectrum ? 'eq-legend-btn on' : 'eq-legend-btn'}
          aria-pressed={spectrum}
          onClick={() => setSpectrum(!spectrum)}
          data-tip="Show the live spectrum behind the curve"
        >
          <span className="h-2 w-3.5" style={{ background: 'var(--bg4)' }} />
          Live spectrum
        </button>
        {curve.mode === 'parametric' && curve.parametric.length < MAX_BANDS && (
          <button type="button" className="eq-legend-btn on" onClick={addInGap} data-tip="Add a band (or double-click the graph)">
            <Icon name="plus" size={12} />
            Add band
          </button>
        )}
      </div>
      <div className="mono flex items-center gap-3.5 text-[10.5px]">
        <span className="t3">
          Max boost <span style={{ color: 'var(--tx)' }}>{response ? formatGain(active ? boost : 0) : '—'}</span>
        </span>
        <span className="t3">
          Preamp <span style={{ color: 'var(--tx)' }}>{formatGain(active ? preamp : 0)}</span>
        </span>
        {limiting || mayClip ? (
          <span className="flex items-center gap-1.5" style={{ color: 'var(--warn)' }} data-tip="The limiter keeps peaks below full scale">
            <span className="h-1.5 w-1.5 rounded-full" style={{ background: 'var(--warn)' }} />
            {limiting ? 'Limiting' : 'May clip'}
          </span>
        ) : (
          <span className="flex items-center gap-1.5" style={{ color: 'var(--ok)' }}>
            <span className="h-1.5 w-1.5 rounded-full" style={{ background: 'var(--ok)' }} />
            No clipping
          </span>
        )}
      </div>
    </>
  );

  return (
    <div className="view">
      <div className="page-header eq-header">
        <div className="flex flex-wrap items-center gap-4">
          <div className="flex flex-col gap-1">
            <div className="cap">Sound</div>
            <h1 className="page-title">Equalizer</h1>
          </div>
          <label className="mt-3.5 flex items-center gap-2 text-[12px] font-semibold">
            <Toggle on={settings.eqEnabled} onChange={(eqEnabled) => patchDsp({ eqEnabled })} label="Equalizer enabled" />
            {settings.eqEnabled ? 'On' : 'Off'}
          </label>
          <Segmented<EqMode>
            label="Equalizer type"
            className="mt-3.5"
            value={curve.mode}
            onChange={(mode) => {
              patchCurve({ mode });
              setSelected(null);
            }}
            options={MODES}
          />
        </div>
        <div className="mt-3.5 flex flex-wrap items-center justify-end gap-2">
          <Segmented<'a' | 'b' | 'bypass'>
            label="A/B compare"
            value={settings.bypass ? 'bypass' : settings.slot}
            onChange={(v) => (v === 'bypass' ? patchDsp({ bypass: true }) : patchDsp({ slot: v, bypass: false }))}
            options={[
              { value: 'a', label: <span className="mono">A</span>, tip: 'Compare slot A' },
              { value: 'b', label: <span className="mono">B</span>, tip: 'Compare slot B' },
              { value: 'bypass', label: 'Bypass', tip: 'Hear the music without the EQ' },
            ]}
          />
          <button
            type="button"
            className="btn eq-preset-btn"
            aria-haspopup="menu"
            onClick={(e) => presetMenu(e.currentTarget)}
          >
            <span className="flex min-w-0 items-center gap-2">
              <span className="h-[7px] w-[7px] flex-none rounded-full" style={{ background: active ? 'var(--ac)' : 'var(--tx3)' }} />
              <span className="el">{preset?.name ?? 'Custom'}</span>
              {modified && <span className="t3 text-[12px] italic">modified</span>}
            </span>
            <Icon name="chevD" size={14} style={{ color: 'var(--tx3)' }} />
          </button>
          <button type="button" className="btn pri" onClick={() => void savePreset(false)}>
            <Icon name="save" size={15} />
            Save
          </button>
          <button type="button" className="btn gh" onClick={reset} data-tip="Flatten the curve">
            Reset
          </button>
        </div>
      </div>

      <BitPerfectNotice />
      <div className="eq-body">
        <div className="flex min-w-0 flex-col gap-3">
          <ResponseGraph
            curve={curve}
            response={response}
            current={current}
            active={active}
            selected={bandSel}
            onSelect={setSelected}
            onBand={updateBand}
            onGraphic={setGraphic}
            onAdd={addBand}
            onRemove={removeBand}
            showSpectrum={spectrum}
            legend={legend}
          />

          {curve.mode === 'parametric' ? (
            <div className="eq-bands" style={{ gridTemplateColumns: `repeat(${Math.max(curve.parametric.length, 8)}, minmax(0, 1fr))` }}>
              {curve.parametric.map((band, i) => (
                <BandCard
                  key={i}
                  index={i}
                  order={i}
                  band={band}
                  selected={i === bandSel}
                  removable={curve.parametric.length > 1}
                  onSelect={() => setSelected(i)}
                  onChange={(patch) => updateBand(i, patch)}
                  onRemove={() => removeBand(i)}
                />
              ))}
            </div>
          ) : (
            <GraphicBands curve={curve} onChange={setGraphic} />
          )}

          <section className="panel eq-tone" aria-label="Tone and space">
            <Knob
              caption="Preamp"
              label="Preamp"
              value={clamp(settings.preampDb, -MAX_GAIN_DB, MAX_GAIN_DB)}
              min={-MAX_GAIN_DB}
              max={MAX_GAIN_DB}
              step={0.5}
              reset={0}
              valueText={formatGain(settings.preampDb)}
              onChange={(v) => patchDsp({ preampDb: round(v, 0.1) })}
            />
            <Knob
              caption="Bass · 100 Hz"
              label="Bass"
              value={curve.bassDb}
              min={-MAX_GAIN_DB}
              max={MAX_GAIN_DB}
              step={0.5}
              reset={0}
              valueText={formatGain(curve.bassDb)}
              onChange={(v) => patchCurve({ bassDb: round(v, 0.1) })}
            />
            <Knob
              caption="Treble · 8 kHz"
              label="Treble"
              value={curve.trebleDb}
              min={-MAX_GAIN_DB}
              max={MAX_GAIN_DB}
              step={0.5}
              reset={0}
              valueText={formatGain(curve.trebleDb)}
              onChange={(v) => patchCurve({ trebleDb: round(v, 0.1) })}
            />
            <span className="eq-tone-sep" />
            <div className="eq-tone-grid">
              <div className="flex flex-col gap-2">
                <div className="flex justify-between">
                  <span className="cap">Balance</span>
                  <span className="mono text-[11px]">{balanceText(settings.balance)}</span>
                </div>
                <CenterSlider
                  label="Balance"
                  value={settings.balance}
                  min={-1}
                  max={1}
                  center={0}
                  step={0.05}
                  reset={0}
                  minLabel="L"
                  maxLabel="R"
                  valueText={balanceText(settings.balance)}
                  onChange={(balance) => patchDsp({ balance: round(balance, 0.01) })}
                />
              </div>
              <div className="flex flex-col gap-2">
                <div className="flex justify-between">
                  <span className="cap">Stereo width</span>
                  <span className="mono text-[11px]">{Math.round(settings.width * 100)} %</span>
                </div>
                <CenterSlider
                  label="Stereo width"
                  value={settings.width}
                  min={0}
                  max={2}
                  center={1}
                  step={0.05}
                  reset={1}
                  minLabel="0"
                  maxLabel="200"
                  valueText={`${Math.round(settings.width * 100)} percent`}
                  onChange={(width) => patchDsp({ width: round(width, 0.01) })}
                />
              </div>
              <label className="flex items-center justify-between gap-2.5">
                <span className="flex flex-col gap-px">
                  <span className="font-semibold">Headphone crossfeed</span>
                  <span className="t3 text-[11.5px]">Softens hard-panned mixes</span>
                </span>
                <Toggle on={settings.crossfeed} onChange={(crossfeed) => patchDsp({ crossfeed })} label="Headphone crossfeed" />
              </label>
              {settings.crossfeed && <CrossfeedPresets value={settings.crossfeedPreset} />}
              <label className="flex items-center justify-between gap-2.5">
                <span className="flex flex-col gap-px">
                  <span className="font-semibold">Auto preamp</span>
                  <span className="t3 text-[11.5px]">
                    {settings.autoPreampAllowanceDb > 0
                      ? `Lowers the level for boosts over ${formatGain(settings.autoPreampAllowanceDb)}`
                      : 'Keeps peak boost below 0 dBFS'}
                  </span>
                </span>
                <Toggle on={settings.autoPreamp} onChange={(autoPreamp) => patchDsp({ autoPreamp })} label="Auto preamp" />
              </label>
            </div>
          </section>
        </div>

        <aside className="panel eq-presets" aria-label="Presets">
          <div className="flex flex-col gap-2.5 border-b border-ln px-3 pb-2 pt-3">
            <div className="flex items-center justify-between">
              <span className="font-bold">Presets</span>
              <button
                type="button"
                className="ib h-[26px] w-[26px]"
                aria-label="Save as a new preset"
                data-tip="Save as a new preset"
                onClick={() => void savePreset(true)}
              >
                <Icon name="plus" size={15} />
              </button>
            </div>
            <label className="fld h-7">
              <Icon name="search" size={14} />
              <input
                className="min-w-0 flex-1 bg-transparent text-[12px] outline-none"
                placeholder="Find preset"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                aria-label="Find preset"
              />
            </label>
          </div>
          <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-1.5 py-2.5">
            <div className="flex flex-col gap-px">
              <div className="cap px-2.5 pb-1.5">My presets</div>
              {mine.map((p, i) => (
                <div key={p.name} className="pr-row">
                  <button
                    type="button"
                    className={p.name === settings.preset ? 'pr on' : 'pr'}
                    style={{ '--i': i } as React.CSSProperties}
                    onClick={() => loadPreset(p)}
                    onContextMenu={contextMenu(() => [
                      { label: 'Load', icon: 'check', onSelect: () => loadPreset(p) },
                      { label: 'Delete preset', icon: 'trash', danger: true, onSelect: () => void deletePreset(p) },
                    ])}
                  >
                    <span className="el">{p.name}</span>
                    <span className="r">{curveSummary(p.curve)}</span>
                  </button>
                  <button type="button" className="pr-del" aria-label={`Delete ${p.name}`} onClick={() => void deletePreset(p)}>
                    <Icon name="trash" size={13} />
                  </button>
                </div>
              ))}
              {!mine.length && (
                <span className="t3 px-2.5 text-[11.5px] [text-wrap:pretty]">
                  {q ? 'No matches.' : 'Save the current curve to keep it here.'}
                </span>
              )}
            </div>
            <div className="flex flex-col gap-px">
              <div className="cap px-2.5 pb-1.5">Built-in</div>
              {builtin.map((p, i) => (
                <button
                  key={p.name}
                  type="button"
                  className={p.name === settings.preset ? 'pr on' : 'pr'}
                  style={{ '--i': mine.length + i } as React.CSSProperties}
                  onClick={() => loadPreset(p)}
                >
                  <span className="el">{p.name}</span>
                  <span className="r">{p.name === settings.preset && modified ? 'MODIFIED' : ''}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="mt-auto flex gap-1.5 border-t border-ln px-3 py-2.5">
            <button type="button" className="btn h-7 flex-1 justify-center text-[12px] font-medium" onClick={() => void importPresets()}>
              Import
            </button>
            <button
              type="button"
              className="btn h-7 flex-1 justify-center text-[12px] font-medium"
              disabled={!presets.some((p) => !p.builtin)}
              onClick={() => void exportPresets()}
            >
              Export
            </button>
          </div>
        </aside>
      </div>
    </div>
  );
}

function balanceText(balance: number): string {
  const pct = Math.round(Math.abs(balance) * 100);
  if (pct === 0) return 'C 0';
  return `${balance < 0 ? 'L' : 'R'} ${pct}`;
}

const KINDS: FilterKind[] = ['peak', 'lowShelf', 'highShelf', 'lowPass', 'highPass', 'notch'];

function BandCard({
  index,
  order,
  band,
  selected,
  removable,
  onSelect,
  onChange,
  onRemove,
}: {
  index: number;
  order: number;
  band: EqBand;
  selected: boolean;
  removable: boolean;
  onSelect: () => void;
  onChange: (patch: Partial<EqBand>) => void;
  onRemove: () => void;
}) {
  const gain = hasGain(band.kind);
  const semitone = Math.pow(2, 1 / 12);
  return (
    <div
      className={['band', selected && 'sel', !band.enabled && 'off'].filter(Boolean).join(' ')}
      style={{ '--i': order } as React.CSSProperties}
      onPointerDown={onSelect}
      onFocus={onSelect}
    >
      <div className="flex items-center justify-between gap-1">
        <span className="flex min-w-0 items-center gap-1.5 text-[12px] font-bold">
          <span className="band-num mono">{index + 1}</span>
          {KIND_SHORT[band.kind]}
        </span>
        <span className="flex items-center gap-1">
          {removable && (
            <button type="button" className="band-del" aria-label={`Remove band ${index + 1}`} onClick={onRemove}>
              <Icon name="x" size={12} />
            </button>
          )}
          <Toggle small on={band.enabled} onChange={(enabled) => onChange({ enabled })} label={`Band ${index + 1} enabled`} />
        </span>
      </div>
      <label className="dd">
        <select
          value={band.kind}
          aria-label={`Band ${index + 1} type`}
          onChange={(e) => onChange({ kind: e.target.value as FilterKind })}
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {KIND_LABEL[k]}
            </option>
          ))}
        </select>
        <Icon name="chevD" size={12} style={{ color: 'var(--tx3)' }} />
      </label>
      <div className="kv">
        <span className="t3">Freq</span>
        <ValueField
          label={`Band ${index + 1} frequency`}
          display={formatFreq(band.freq)}
          edit={band.freq >= 1000 ? `${+(band.freq / 1000).toFixed(3)}k` : String(Math.round(band.freq))}
          parse={parseFreq}
          onCommit={(freq) => onChange({ freq: clamp(freq, 20, 20000) })}
          onStep={(dir, fine) => onChange({ freq: Math.round(clamp(band.freq * Math.pow(semitone, dir * (fine ? 0.25 : 1)), 20, 20000)) })}
        />
      </div>
      <div className="kv">
        <span className="t3">Gain</span>
        <ValueField
          label={`Band ${index + 1} gain`}
          disabled={!gain}
          color={gain && band.gainDb > 0.05 ? 'var(--ac)' : undefined}
          display={gain ? formatGain(band.gainDb) : '—'}
          edit={band.gainDb.toFixed(1)}
          parse={parseNumber}
          onCommit={(g) => onChange({ gainDb: clamp(round(g, 0.1), -MAX_GAIN_DB, MAX_GAIN_DB) })}
          onStep={(dir, fine) => onChange({ gainDb: clamp(round(band.gainDb + dir * (fine ? 0.1 : 0.5), 0.1), -MAX_GAIN_DB, MAX_GAIN_DB) })}
        />
      </div>
      <div className="kv">
        <span className="t3">Q</span>
        <ValueField
          label={`Band ${index + 1} Q`}
          display={band.q.toFixed(2)}
          edit={band.q.toFixed(2)}
          parse={parseNumber}
          onCommit={(q) => onChange({ q: clamp(q, 0.1, 20) })}
          onStep={(dir, fine) => onChange({ q: Math.round(clamp(band.q * Math.pow(1.1, dir * (fine ? 0.25 : 1)), 0.1, 20) * 100) / 100 })}
        />
      </div>
    </div>
  );
}

function GraphicBands({ curve, onChange }: { curve: EqCurve; onChange: (index: number, gainDb: number) => void }) {
  const wide = curve.mode === 'graphic10';
  const freqs = wide ? GRAPHIC_10 : GRAPHIC_31;
  const gains = wide ? curve.graphic10 : curve.graphic31;
  const caption = (f: number) => (f >= 1000 ? `${+(f / 1000).toFixed(1)}k` : String(f));
  return (
    <section className={wide ? 'panel eq-faders' : 'panel eq-faders dense'} aria-label="Graphic equalizer bands">
      {freqs.map((f, i) => (
        <Fader
          key={f}
          order={i}
          compact={!wide}
          label={`${formatFreq(f)} band`}
          caption={caption(f)}
          value={gains[i] ?? 0}
          min={-MAX_GAIN_DB}
          max={MAX_GAIN_DB}
          step={0.5}
          reset={0}
          valueText={formatGain(gains[i] ?? 0)}
          onChange={(g) => onChange(i, round(g, 0.1))}
        />
      ))}
    </section>
  );
}

/** While bit-perfect playback is on, the equalizer is out of the path. */
function BitPerfectNotice() {
  const bitPerfect = usePlayer((s) => s.signal.bitPerfect);
  const navigate = useNavigation((s) => s.navigate);
  if (!bitPerfect) return null;
  return (
    <div className="bp-notice" role="status">
      <Icon name="info" size={15} />
      <span>Bit-perfect playback is on, so the equalizer and tone controls are bypassed. Changes here apply once it is off.</span>
      <button type="button" className="btn gh sm" onClick={() => navigate('dsp')}>
        Audio / DSP
        <Icon name="chevR" size={13} />
      </button>
    </div>
  );
}
