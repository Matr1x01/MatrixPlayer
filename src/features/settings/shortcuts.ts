/** Shortcuts handled by useGlobalShortcuts, listed in Settings. */
export const SHORTCUTS: { action: string; keys: string[] }[] = [
  { action: 'Play / pause', keys: ['Space'] },
  { action: 'Next track', keys: ['Ctrl →'] },
  { action: 'Previous track', keys: ['Ctrl ←'] },
  { action: 'Seek forward / back 10 s', keys: ['Shift →', 'Shift ←'] },
  { action: 'Volume up / down', keys: ['Ctrl ↑', 'Ctrl ↓'] },
  { action: 'Mute', keys: ['M'] },
  { action: 'Shuffle', keys: ['S'] },
  { action: 'Repeat (off, all, one)', keys: ['R'] },
  { action: 'Open files', keys: ['Ctrl O'] },
  { action: 'Open folder', keys: ['Ctrl Shift O'] },
  { action: 'Now Playing', keys: ['Ctrl P'] },
  { action: 'Settings', keys: ['Ctrl ,'] },
  { action: 'Back / forward', keys: ['Alt ←', 'Alt →'] },
  { action: 'Full screen', keys: ['F11'] },
  { action: 'Media keys (window focused)', keys: ['Play', 'Next', 'Prev', 'Stop'] },
];
