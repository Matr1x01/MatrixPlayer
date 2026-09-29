import { useEffect, useMemo, useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import type { IconName } from '@/components/icons/paths';
import { Segmented } from '@/components/ui/Segmented';
import { Toggle } from '@/components/ui/Toggle';
import { backendLabel } from '@/features/player/format';
import { playerApi, systemApi } from '@/services/ipc';
import { patchDsp, useDsp } from '@/stores/dsp';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { updatePreferences, usePreferences } from '@/stores/preferences';
import { run } from '@/stores/toasts';
import { setViewPrefs, useViewPrefs, type VisualizerMode } from '@/stores/viewPrefs';
import type { AppInfo } from '@/types/preferences';

import { AppearanceSettings, SettingRow } from './AppearanceSettings';
import { LibrarySettings } from './LibrarySettings';
import { useSettingsSearch } from './search';
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

/** Words each section answers to in the settings search, beyond its name:
 * its rows' titles and what they are about. */
const KEYWORDS: Record<Section, string> = {
  general: 'startup start launch resume restore session queue last',
  library: 'music folders add remove rescan scan tags reread watching moved renamed files',
  playback: 'gapless soft fades replaygain loudness previous restart broken files skip',
  output: 'backend pipewire pulseaudio alsa device headphones dac speakers processing exclusive bit-perfect',
  dsp: 'equalizer eq signal chain limiter crossfeed dither preamp replaygain bit-perfect bitperfect exclusive',
  appearance: 'theme dark light accent colour color background density rows interface scale zoom motion animation sidebar',
  nowPlaying: 'visualizer spectrum waveform frequency meters artwork background',
  lyrics: 'lyrics lrc synced embedded sources offset',
  shortcuts: `keyboard keys hotkeys ${SHORTCUTS.map((s) => s.action).join(' ')}`,
  desktop: 'mpris media keys lock screen notifications opening files command line launcher actions',
  about: 'version gstreamer storage database',
};

function matches(section: (typeof SECTIONS)[number], query: string) {
  const text = `${section.label} ${KEYWORDS[section.id]}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => text.includes(word));
}

function GeneralSettings() {
  const prefs = usePreferences((s) => s.prefs);
  return (
    <SettingRow title="Startup" hint="What MatrixPlayer opens with">
      <label className="flex items-center gap-2.5">
        <Toggle on={prefs.restoreSession} onChange={(restoreSession) => void updatePreferences({ restoreSession })} label="Resume the last session" />
        Resume the last session, paused where it was
      </label>
    </SettingRow>
  );
}

function PlaybackSettings() {
  const gapless = usePlayer((s) => s.queue.gapless);
  const settings = useDsp((s) => s.settings);
  const navigate = useNavigation((s) => s.navigate);
  return (
    <>
      <SettingRow title="Gapless playback" hint="Tracks follow each other without silence">
        <Toggle on={gapless} onChange={(on) => void run(playerApi.setGapless(on))} label="Gapless playback" />
      </SettingRow>
      <SettingRow title="Soft fades" hint="Ramps on pause and resume (120 ms), and on seek, skip and stop (40 ms)">
        <Toggle on={!!settings?.softFades} disabled={!settings} onChange={(softFades) => patchDsp({ softFades })} label="Soft fades" />
      </SettingRow>
      <SettingRow title="ReplayGain" hint="Loudness levelling from the files' tags">
        <button type="button" className="btn gh" onClick={() => navigate('dsp')}>
          {settings ? { off: 'Off', track: 'Track', album: 'Album', smart: 'Smart' }[settings.replayGain] : '…'} · Audio / DSP
          <Icon name="chevR" size={14} />
        </button>
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
  const navigate = useNavigation((s) => s.navigate);
  return (
    <>
      <SettingRow title="Backend" hint="Chosen automatically: PipeWire, then PulseAudio, then ALSA">
        <span className="mono">{backendLabel(signal.backend)}</span>
      </SettingRow>
      <SettingRow title="Device" hint="Where the music plays">
        <button type="button" className="btn gh" onClick={() => navigate('dsp')}>
          {signal.device?.name ?? 'System default'} · Change
          <Icon name="chevR" size={14} />
        </button>
      </SettingRow>
      <SettingRow title="Processing" hint="Precision of MatrixPlayer's own processing">
        <span className="mono">{signal.bitPerfect ? 'None · bit-perfect' : '64-bit float'}</span>
      </SettingRow>
      <SettingRow title="Bit-perfect & exclusive access" hint="Samples unchanged; with exclusive access, straight to the device">
        <button type="button" className="btn gh" onClick={() => navigate('dsp')}>
          {signal.exclusive ? 'Exclusive' : signal.bitPerfect ? 'Bit-perfect' : 'Off'} · Change
          <Icon name="chevR" size={14} />
        </button>
      </SettingRow>
    </>
  );
}

function DspSettingsSection() {
  const settings = useDsp((s) => s.settings);
  const navigate = useNavigation((s) => s.navigate);
  const eq = !settings?.eqEnabled ? 'Off' : settings.bypass ? 'Bypassed' : (settings.preset ?? 'Custom curve');
  return (
    <>
      <SettingRow title="Equalizer" hint="Parametric and graphic EQ, tone and stereo controls, presets">
        <button type="button" className="btn gh" onClick={() => navigate('equalizer')}>
          {eq} · Open
          <Icon name="chevR" size={14} />
        </button>
      </SettingRow>
      <SettingRow title="Signal chain" hint="ReplayGain, limiter, crossfeed, dither and output">
        <button type="button" className="btn gh" onClick={() => navigate('dsp')}>
          Audio / DSP
          <Icon name="chevR" size={14} />
        </button>
      </SettingRow>
    </>
  );
}

function NowPlayingSettings() {
  const visualizer = useViewPrefs((s) => s.visualizer);
  return (
    <>
      <SettingRow title="Visualizer" hint="What the Now Playing panel shows">
        <Segmented<VisualizerMode>
          label="Visualizer"
          size="sm"
          value={visualizer}
          onChange={(v) => setViewPrefs({ visualizer: v })}
          options={[
            { value: 'spectrum', label: 'Spectrum' },
            { value: 'waveform', label: 'Waveform' },
            { value: 'frequency', label: 'Frequency' },
            { value: 'meters', label: 'Meters' },
            { value: 'artwork', label: 'Artwork' },
          ]}
        />
      </SettingRow>
      <SettingRow title="Background" hint="Light cast by the artwork">
        <span className="t2">Set under Appearance</span>
      </SettingRow>
    </>
  );
}

function LyricsSettings() {
  return (
    <>
      <SettingRow title="Sources" hint="Where lyrics are found, first match wins">
        <span className="t2">
          A <span className="mono">.lrc</span> file named like the track, then lyrics embedded in its tags
        </span>
      </SettingRow>
      <SettingRow title="Synced lyrics" hint="LRC timestamps, including [offset:]">
        <span className="t2">Follow playback; click a line to jump to it</span>
      </SettingRow>
    </>
  );
}

function DesktopSettings() {
  const prefs = usePreferences((s) => s.prefs);
  return (
    <>
      <SettingRow title="Media keys & controls" hint="Desktop media controls, lock screen and media keys">
        <span className="t2">
          Through MPRIS as <span className="mono">org.mpris.MediaPlayer2.matrixplayer</span>
        </span>
      </SettingRow>
      <SettingRow title="Track notifications" hint="Announce each new track while MatrixPlayer is in the background">
        <Toggle
          on={prefs.trackNotifications}
          onChange={(trackNotifications) => void updatePreferences({ trackNotifications })}
          label="Track notifications"
        />
      </SettingRow>
      <SettingRow title="Opening files" hint="From the file manager or the command line">
        <span className="t2">Plays them in the running window</span>
      </SettingRow>
      <SettingRow title="Launcher actions" hint="Right-click the launcher icon">
        <span className="mono t2">Play/Pause · Next · Previous</span>
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

export function SettingsView({ initial }: { initial?: string }) {
  const [chosen, setSection] = useState<Section>(SECTIONS.some((s) => s.id === initial) ? (initial as Section) : 'general');
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    void systemApi.appInfo().then(setInfo);
  }, []);
  const query = useSettingsSearch((s) => s.query.trim());
  const found = useMemo(() => (query ? SECTIONS.filter((s) => matches(s, query)) : SECTIONS), [query]);
  // While searching, show a matching section: the chosen one if it matches.
  const section = query && found.length > 0 && !found.some((s) => s.id === chosen) ? found[0]!.id : chosen;
  const current = SECTIONS.find((s) => s.id === section)!;

  return (
    <div className="flex min-h-0 flex-1">
      <nav className="set-nav" aria-label="Settings sections">
        {found.map((s) => (
          <button key={s.id} type="button" className={s.id === section ? 'sn on' : 'sn'} onClick={() => setSection(s.id)}>
            <Icon name={s.icon} size={16} filled={s.icon === 'play'} />
            {s.label}
          </button>
        ))}
        {found.length === 0 && <span className="t3 px-2.5 py-2 text-[12.5px]">No settings match “{query}”</span>}
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
          {section === 'general' && <GeneralSettings />}
          {section === 'appearance' && <AppearanceSettings />}
          {section === 'library' && <LibrarySettings />}
          {section === 'playback' && <PlaybackSettings />}
          {section === 'output' && <OutputSettings />}
          {section === 'dsp' && <DspSettingsSection />}
          {section === 'nowPlaying' && <NowPlayingSettings />}
          {section === 'lyrics' && <LyricsSettings />}
          {section === 'desktop' && <DesktopSettings />}
          {section === 'shortcuts' && <ShortcutSettings />}
          {section === 'about' && <About info={info} />}
        </div>
        {section === 'appearance' ? <SettingsPreview /> : <div />}
      </main>
    </div>
  );
}
