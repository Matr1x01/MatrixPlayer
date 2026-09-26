import { useEffect, useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import type { IconName } from '@/components/icons/paths';
import { backendLabel } from '@/features/player/format';
import { systemApi } from '@/services/ipc';
import { usePlayer } from '@/stores/player';
import type { AppInfo } from '@/types/preferences';

import { AppearanceSettings, SettingRow } from './AppearanceSettings';
import { SettingsPreview } from './SettingsPreview';
import { SHORTCUTS } from './shortcuts';

type Section =
  | 'general'
  | 'library'
  | 'playback'
  | 'output'
  | 'dsp'
  | 'appearance'
  | 'nowPlaying'
  | 'lyrics'
  | 'shortcuts'
  | 'desktop'
  | 'about';

const SECTIONS: { id: Section; label: string; icon: IconName }[] = [
  { id: 'general', label: 'General', icon: 'general' },
  { id: 'library', label: 'Library & folders', icon: 'folder' },
  { id: 'playback', label: 'Playback', icon: 'play' },
  { id: 'output', label: 'Audio output', icon: 'hp' },
  { id: 'dsp', label: 'Equalizer & DSP', icon: 'eq' },
  { id: 'appearance', label: 'Appearance', icon: 'palette' },
  { id: 'nowPlaying', label: 'Now Playing', icon: 'nowPlaying' },
  { id: 'lyrics', label: 'Lyrics', icon: 'lyrics' },
  { id: 'shortcuts', label: 'Keyboard shortcuts', icon: 'keyboard' },
  { id: 'desktop', label: 'Desktop integration', icon: 'tv' },
  { id: 'about', label: 'About', icon: 'info' },
];

/** What each not-yet-configurable section will hold, stated plainly. */
const PENDING: Partial<Record<Section, string>> = {
  general: 'Startup behaviour and session restore will live here.',
  library: 'Music folders, rescans and file watching arrive with library indexing.',
  dsp: 'Equalizer presets and DSP stages arrive with the DSP milestone.',
  nowPlaying: 'Visualizer and layout options arrive with the audio analysis stage. The background style is under Appearance.',
  lyrics: 'Lyrics sources and sync offsets arrive with lyrics support.',
  desktop: 'MPRIS, media keys, notifications and the tray arrive with desktop integration.',
};

function PlaybackSettings() {
  return (
    <>
      <SettingRow title="Gapless playback" hint="Tracks follow each other without silence">
        <span className="chip ok">Always on</span>
      </SettingRow>
      <SettingRow title="Previous" hint="Behaviour of the previous button">
        <span className="t2">Restarts the track after 3 s, otherwise goes to the previous track</span>
      </SettingRow>
      <SettingRow title="Broken files" hint="Files that cannot be decoded">
        <span className="t2">Reported, then skipped</span>
      </SettingRow>
    </>
  );
}

function OutputSettings() {
  const signal = usePlayer((s) => s.signal);
  return (
    <>
      <SettingRow title="Output" hint="Chosen automatically: PipeWire, then PulseAudio, then ALSA">
        <span className="mono">{backendLabel(signal.backend)}</span>
      </SettingRow>
      <SettingRow title="Processing" hint="Precision of MatrixPlayer's own processing">
        <span className="mono">64-bit float</span>
      </SettingRow>
      <SettingRow title="Device selection" hint="Per-device output and exclusive mode">
        <span className="t3">Not available yet: the system default output is used</span>
      </SettingRow>
    </>
  );
}

function ShortcutSettings() {
  return (
    <div className="border-t border-ln pt-2">
      {SHORTCUTS.map((s) => (
        <div key={s.action} className="flex h-9 items-center justify-between border-b border-dashed border-ln">
          <span>{s.action}</span>
          <span className="flex gap-1">
            {s.keys.map((k) => (
              <span key={k} className="kbd">
                {k}
              </span>
            ))}
          </span>
        </div>
      ))}
    </div>
  );
}

function About({ info }: { info: AppInfo | null }) {
  return (
    <>
      <SettingRow title="Version" hint="MatrixPlayer">
        <span className="mono">{info?.version ?? '…'}</span>
      </SettingRow>
      <SettingRow title="Audio engine" hint="Decoding and playback">
        <span className="mono">{info?.gstreamerVersion ?? '…'}</span>
      </SettingRow>
      <SettingRow title="Storage" hint="Settings and library data">
        <span className="t2">SQLite database in the app data directory</span>
      </SettingRow>
    </>
  );
}

export function SettingsView() {
  const [section, setSection] = useState<Section>('appearance');
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    void systemApi.appInfo().then(setInfo);
  }, []);
  const current = SECTIONS.find((s) => s.id === section)!;

  return (
    <div className="flex min-h-0 flex-1">
      <nav className="set-nav" aria-label="Settings sections">
        {SECTIONS.map((s) => (
          <button key={s.id} type="button" className={s.id === section ? 'sn on' : 'sn'} onClick={() => setSection(s.id)}>
            <Icon name={s.icon} size={16} filled={s.icon === 'play'} />
            {s.label}
          </button>
        ))}
        <div className="mt-auto flex flex-col gap-[3px] border-t border-ln p-2.5">
          <span className="mono t3 text-[10px]">MatrixPlayer {info?.version ?? ''}</span>
          <span className="mono t3 text-[10px]">Settings are saved automatically</span>
        </div>
      </nav>
      <main className="set-main">
        <div className="flex min-w-0 flex-col">
          <div className="flex flex-col gap-1 pb-4">
            <span className="cap">Settings</span>
            <h1 className="page-title">{current.label}</h1>
          </div>
          {section === 'appearance' && <AppearanceSettings />}
          {section === 'playback' && <PlaybackSettings />}
          {section === 'output' && <OutputSettings />}
          {section === 'shortcuts' && <ShortcutSettings />}
          {section === 'about' && <About info={info} />}
          {PENDING[section] && <p className="t3 border-t border-ln pt-3.5 text-[12.5px]">{PENDING[section]}</p>}
        </div>
        {section === 'appearance' ? <SettingsPreview /> : <div />}
      </main>
    </div>
  );
}
