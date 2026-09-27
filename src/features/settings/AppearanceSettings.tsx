import { useState, type ReactNode } from 'react';

import { Icon } from '@/components/icons/Icon';
import { Segmented } from '@/components/ui/Segmented';
import { Slider } from '@/components/ui/Slider';
import { Toggle } from '@/components/ui/Toggle';
import { updatePreferences, usePreferences } from '@/stores/preferences';
import type { Ambient, Density } from '@/types/preferences';

import { ACCENTS, PREVIEW_ART, THEME_ACCENT, THEMES, accentHex } from './themes';

const SCALE_MIN = 0.85;
const SCALE_MAX = 1.5;

export function SettingRow({ title, hint, children }: { title: string; hint: string; children: ReactNode }) {
  return (
    <div className="set-row">
      <div className="set-row-label">
        <b>{title}</b>
        <span>{hint}</span>
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function ThemePicker() {
  const { theme } = usePreferences((s) => s.prefs);
  return (
    <div className="grid grid-cols-5 gap-3.5" role="radiogroup" aria-label="Theme">
      {THEMES.map((t) => {
        const on = t.id === theme;
        return (
          <button
            key={t.id}
            type="button"
            className="thm"
            role="radio"
            aria-checked={on}
            onClick={() => void updatePreferences({ theme: t.id, accent: null })}
          >
            <span className="pv" style={{ background: t.bg1, boxShadow: on ? '0 0 0 2px var(--ac), 0 0 0 5px var(--acs)' : '0 0 0 1px var(--ln2)' }}>
              <span className="pv-side" style={{ background: t.bg2 }}>
                <span style={{ background: t.ac }} />
                <span style={{ background: t.ln }} />
                <span style={{ background: t.ln }} />
                <span style={{ background: t.ln }} />
              </span>
              <span className="pv-grid">
                {PREVIEW_ART.map((c) => (
                  <span key={c} style={{ background: c }} />
                ))}
              </span>
              <span className="pv-bar" style={{ background: t.bg2, borderTopColor: t.ln }}>
                <span className="pv-dot" style={{ background: t.ac }} />
                <span className="pv-track" style={{ background: t.ln }}>
                  <span style={{ background: t.ac }} />
                </span>
              </span>
            </span>
            <span className="flex items-center justify-between">
              <span className="text-[12.5px] font-semibold">{t.label}</span>
              <span className="mono t3 text-[9.5px]">{t.sub}</span>
            </span>
          </button>
        );
      })}
    </div>
  );
}

function AccentPicker() {
  const { theme, accent } = usePreferences((s) => s.prefs);
  const effective = accent ?? THEME_ACCENT[theme];
  return (
    <div className="flex flex-wrap items-center gap-2.5" role="radiogroup" aria-label="Accent colour">
      {ACCENTS.map((a) => {
        const on = a.id === effective;
        const color = theme === 'light' ? a.light : a.color;
        const ink = theme === 'light' ? '#fff' : a.ink;
        return (
          <button
            key={a.id}
            type="button"
            className="sw"
            role="radio"
            aria-checked={on}
            aria-label={a.label}
            data-tip={a.label}
            style={{ background: color, boxShadow: on ? `0 0 0 2px var(--bg1), 0 0 0 4px ${color}` : 'inset 0 0 0 1px rgba(0,0,0,.2)' }}
            onClick={() => void updatePreferences({ accent: a.id === THEME_ACCENT[theme] ? null : a.id })}
          >
            {on && <Icon name="check" size={14} strokeWidth={2.6} style={{ color: ink }} />}
          </button>
        );
      })}
      <span className="vsep mx-0.5" style={{ height: 22 }} />
      <button type="button" className="btn sm" style={{ fontWeight: 500 }} disabled data-tip="Needs artwork analysis (planned)">
        <span className="h-3 w-3 rounded-full" style={{ background: 'conic-gradient(#ec8a3a,#3c3a55,#f4dca8,#ec8a3a)' }} />
        From artwork
      </button>
      <span className="fld mono" style={{ height: 28, width: 96, fontSize: 11, color: 'var(--tx)' }}>
        <span className="h-2.5 w-2.5 rounded-[2px]" style={{ background: 'var(--ac)' }} />
        {accentHex(theme, accent)}
      </span>
    </div>
  );
}

function ScaleSlider() {
  const scale = usePreferences((s) => s.prefs.interfaceScale);
  const [preview, setPreview] = useState<number | null>(null);
  const toScale = (f: number) => Math.round((SCALE_MIN + f * (SCALE_MAX - SCALE_MIN)) * 20) / 20;
  const shown = preview ?? scale;
  return (
    <div className="flex items-center gap-3.5">
      <span className="mono t3 text-[10.5px]">85%</span>
      <Slider
        variant="setting"
        className="max-w-[320px] flex-1"
        label="Interface scale"
        value={(scale - SCALE_MIN) / (SCALE_MAX - SCALE_MIN)}
        step={0.05 / (SCALE_MAX - SCALE_MIN)}
        valueText={(f) => `${Math.round(toScale(f) * 100)}%`}
        onInput={(f) => setPreview(toScale(f))}
        onCommit={(f) => {
          setPreview(null);
          void updatePreferences({ interfaceScale: toScale(f) });
        }}
      />
      <span className="mono t3 text-[10.5px]">150%</span>
      <span className="mono w-11 text-right text-[12px]">{Math.round(shown * 100)}%</span>
    </div>
  );
}

export function AppearanceSettings() {
  const prefs = usePreferences((s) => s.prefs);
  return (
    <>
      <div className="flex flex-col gap-3 border-t border-ln pb-4 pt-3.5">
        <div className="flex items-center justify-between">
          <div className="flex flex-col gap-[3px]">
            <b className="font-semibold">Theme</b>
            <span className="t3 text-[12px]">Surfaces, contrast and default accent</span>
          </div>
          <label className="t2 flex items-center gap-2 text-[12px]" data-tip="Planned: follow the desktop's light/dark setting">
            Follow system light / dark
            <Toggle on={false} onChange={() => {}} label="Follow system light or dark" disabled />
          </label>
        </div>
        <ThemePicker />
      </div>
      <SettingRow title="Accent colour" hint="Used for playback state, focus and active controls">
        <AccentPicker />
      </SettingRow>
      <SettingRow title="Now Playing background" hint="Soft light taken from the album artwork">
        <Segmented<Ambient>
          label="Now Playing background"
          size="lg"
          value={prefs.ambient}
          onChange={(ambient) => void updatePreferences({ ambient })}
          options={[
            { value: 'off', label: 'Off' },
            { value: 'subtle', label: 'Subtle' },
            { value: 'immersive', label: 'Immersive' },
          ]}
        />
      </SettingRow>
      <SettingRow title="Density" hint="Row height in lists and tables">
        <Segmented<Density>
          label="Density"
          size="lg"
          value={prefs.density}
          onChange={(density) => void updatePreferences({ density })}
          options={[
            { value: 'compact', label: 'Compact · 32' },
            { value: 'standard', label: 'Standard · 40' },
            { value: 'comfortable', label: 'Comfortable · 48' },
          ]}
        />
      </SettingRow>
      <SettingRow title="Interface scale" hint="Independent of the desktop scale factor">
        <ScaleSlider />
      </SettingRow>
      <SettingRow title="Motion" hint="Visualizer and transition animations">
        <label className="flex items-center gap-2.5">
          <Toggle on={prefs.reduceMotion} onChange={(reduceMotion) => void updatePreferences({ reduceMotion })} label="Reduce motion" />
          Reduce motion
        </label>
      </SettingRow>
      <SettingRow title="Sidebar" hint="Names and counts, or icons only (narrow windows always use icons)">
        <label className="flex items-center gap-2.5">
          <Toggle
            on={prefs.sidebarExpanded}
            onChange={(sidebarExpanded) => void updatePreferences({ sidebarExpanded })}
            label="Sidebar expanded"
          />
          Sidebar expanded
        </label>
      </SettingRow>
    </>
  );
}
