import type { Accent, Theme } from '@/types/preferences';

/** Theme swatches from the Settings and Foundations boards. */
export const THEMES: { id: Theme; label: string; sub: string; bg1: string; bg2: string; ln: string; ac: string }[] = [
  { id: 'graphite', label: 'Graphite', sub: 'DEFAULT', bg1: '#101113', bg2: '#15171A', ln: '#2E3136', ac: '#F0A43A' },
  { id: 'violet', label: 'Violet', sub: 'DARK', bg1: '#110F17', bg2: '#16141E', ln: '#322D3F', ac: '#A68CFF' },
  { id: 'cobalt', label: 'Cobalt', sub: 'DARK', bg1: '#0F1218', bg2: '#141821', ln: '#2D3544', ac: '#52A5FF' },
  { id: 'crimson', label: 'Crimson', sub: 'DARK', bg1: '#141010', bg2: '#1A1515', ln: '#362D2D', ac: '#FF5F64' },
  { id: 'light', label: 'Light', sub: 'LIGHT', bg1: '#F6F5F2', bg2: '#ECEBE7', ln: '#C6C5BF', ac: '#C2410C' },
];

export const ACCENTS: { id: Accent; label: string; color: string; ink: string; light: string }[] = [
  { id: 'amber', label: 'Amber', color: '#F0A43A', ink: '#1A1206', light: '#B45309' },
  { id: 'violet', label: 'Violet', color: '#A68CFF', ink: '#140D2B', light: '#6D4AE0' },
  { id: 'cobalt', label: 'Cobalt', color: '#52A5FF', ink: '#06142A', light: '#1D64D8' },
  { id: 'crimson', label: 'Crimson', color: '#FF5F64', ink: '#2A0708', light: '#C8232C' },
  { id: 'jade', label: 'Jade', color: '#4FC48A', ink: '#05200F', light: '#15803D' },
  { id: 'teal', label: 'Teal', color: '#3CC6C6', ink: '#032222', light: '#0E7C7B' },
  { id: 'rose', label: 'Rose', color: '#F07AB0', ink: '#2A0718', light: '#BE185D' },
  { id: 'mono', label: 'Mono', color: '#E6E4DE', ink: '#141414', light: '#1F2023' },
];

/** The accent each theme uses when none is chosen. */
export const THEME_ACCENT: Record<Theme, Accent> = {
  graphite: 'amber',
  violet: 'violet',
  cobalt: 'cobalt',
  crimson: 'crimson',
  light: 'amber',
};

/** Hex shown in the accent field for the effective accent. */
export function accentHex(theme: Theme, accent: Accent | null): string {
  if (theme === 'light' && accent === null) return '#C2410C';
  const spec = ACCENTS.find((a) => a.id === (accent ?? THEME_ACCENT[theme]))!;
  return theme === 'light' ? spec.light : spec.color;
}

/** Artwork colours used in theme preview cards (Settings board). */
export const PREVIEW_ART = ['#a3402b', '#3c7a92', '#e8f1e4', '#f2c14e'];
