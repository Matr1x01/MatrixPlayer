// Interface preferences. Rust validates and persists them; the store applies
// changes optimistically so theme switches feel instant, and rolls back if
// the core rejects them.

import { create } from 'zustand';

import { systemApi } from '@/services/ipc';
import type { UiPreferences, UiPreferencesPatch } from '@/types/preferences';

import { errorMessage, showToast } from './toasts';

interface PreferencesStore {
  prefs: UiPreferences;
  loaded: boolean;
}

export const DEFAULT_PREFERENCES: UiPreferences = {
  theme: 'graphite',
  accent: null,
  ambient: 'subtle',
  density: 'compact',
  sidebarExpanded: true,
  reduceMotion: false,
  interfaceScale: 1,
  trackNotifications: true,
  restoreSession: true,
  onlineLyrics: true,
  logRetentionDays: 7,
};

export const usePreferences = create<PreferencesStore>()(() => ({
  prefs: DEFAULT_PREFERENCES,
  loaded: false,
}));

export async function loadPreferences() {
  try {
    usePreferences.setState({ prefs: await systemApi.preferences(), loaded: true });
  } catch (err) {
    usePreferences.setState({ loaded: true });
    showToast({ tone: 'error', message: `Could not load preferences: ${errorMessage(err)}` });
  }
}

export async function updatePreferences(patch: UiPreferencesPatch) {
  const previous = usePreferences.getState().prefs;
  usePreferences.setState({ prefs: { ...previous, ...patch } });
  try {
    usePreferences.setState({ prefs: await systemApi.updatePreferences(patch) });
  } catch (err) {
    usePreferences.setState({ prefs: previous });
    showToast({ tone: 'error', message: `Could not save preference: ${errorMessage(err)}` });
  }
}
