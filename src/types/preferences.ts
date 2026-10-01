// Mirror of src-tauri/src/preferences.rs.

export type Theme = 'graphite' | 'violet' | 'cobalt' | 'crimson' | 'light';
export type Accent = 'amber' | 'violet' | 'cobalt' | 'crimson' | 'jade' | 'teal' | 'rose' | 'mono';
export type Ambient = 'off' | 'subtle' | 'immersive';
export type Density = 'compact' | 'standard' | 'comfortable';

export interface UiPreferences {
  theme: Theme;
  /** null follows the theme's own accent. */
  accent: Accent | null;
  ambient: Ambient;
  density: Density;
  sidebarExpanded: boolean;
  reduceMotion: boolean;
  /** Interface zoom, 0.85–1.5. */
  interfaceScale: number;
  /** Announce new tracks with a desktop notification while in the background. */
  trackNotifications: boolean;
  /** At startup, bring back the last queue, paused where it was. */
  restoreSession: boolean;
  /** How long log files are kept, 1–90 days. */
  logRetentionDays: number;
}

export type UiPreferencesPatch = Partial<UiPreferences>;

export interface AppInfo {
  name: string;
  version: string;
  gstreamerVersion: string;
  /** Where the log files are; null if they cannot be written. */
  logDir: string | null;
}
