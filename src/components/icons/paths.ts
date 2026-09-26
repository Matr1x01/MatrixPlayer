// Icon paths from the design (Foundations · 04): 24 px grid, 1.6 px stroke,
// round caps. Transport icons and an active favourite are drawn filled.

export const ICONS = {
  // Navigation
  home: 'M4 11 12 4l8 7v9h-5v-6H9v6H4z',
  artist: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4.5 20.5c.6-3.6 3.7-6 7.5-6s6.9 2.4 7.5 6',
  album: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 14.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z',
  songs: 'M9 18V5l11-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM20 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z',
  genre: 'M3.5 12.5v-8a1 1 0 0 1 1-1h8l8.5 8.5-9 9zM8 8h.01',
  folder: 'M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2.5h8.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z',
  playlist: 'M4 6h12M4 11h12M4 16h7M17.5 13.5v7M14 17h7',
  heart: 'M12 20s-7.5-4.6-7.5-10.2A4.2 4.2 0 0 1 12 7.3a4.2 4.2 0 0 1 7.5 2.5C19.5 15.4 12 20 12 20z',
  recent: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7.5V12l3 2',
  queue: 'M4 6h16M4 11h16M4 16h8M16 14.5v6l5-3z',
  eq: 'M6 4v16M12 4v16M18 4v16M3.5 15h5M9.5 8h5M15.5 13h5',
  dsp: 'M3 12h3l2.5-6 4 12 3-9 2 3H21',
  gear: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
  search: 'M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4',

  // Transport
  prev: 'M18.5 5.5v13L9 12zM5.5 5.5h2.2v13H5.5z',
  next: 'M5.5 5.5v13L15 12zM16.3 5.5h2.2v13h-2.2z',
  play: 'M7.5 4.8v14.4L19.5 12z',
  pause: 'M6.5 5h4v14h-4zM13.5 5h4v14h-4z',
  shuffle: 'M16 4h4v4M4 20 20 4M20 16v4h-4M14.5 14.5 20 20M4 4l5 5',
  repeat: 'M17 2.5 20.5 6 17 9.5M3.5 11V9a3 3 0 0 1 3-3h14M7 21.5 3.5 18 7 14.5M20.5 13v2a3 3 0 0 1-3 3h-14',
  vol: 'M11 5 6.5 9h-3v6h3L11 19zM15.5 9a4.5 4.5 0 0 1 0 6M18.3 6.2a8.5 8.5 0 0 1 0 11.6',
  volMuted: 'M11 5 6.5 9h-3v6h3L11 19zM16 9.5l5 5M21 9.5l-5 5',
  hp: 'M4 16v-4a8 8 0 0 1 16 0v4M4 15.5h3.5V21H5a1 1 0 0 1-1-1zM20 15.5h-3.5V21H19a1 1 0 0 0 1-1z',
  lyrics: 'M4.5 4.5h15v11h-10l-5 4zM8.5 8.5h7M8.5 11.5h4.5',
  playNext: 'M4 6h11M4 11h11M4 16h6M15 13v7l5.5-3.5z',

  // Actions
  more: 'M5 12h.01M12 12h.01M19 12h.01',
  x: 'M6 6l12 12M18 6 6 18',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  plus: 'M12 5v14M5 12h14',
  trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3',
  edit: 'M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4',
  info: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5.5M12 7.8h.01',
  warn: 'M12 4 2.5 20h19zM12 10v4.5M12 17.2h.01',
  refresh: 'M20 11.5A8 8 0 1 1 17.7 6M20 4.5V10h-5.5',
  save: 'M5 4h11l3 3v13H5zM8 4v5h7V4M8 20v-6h8v6',
  link: 'M10 14a4 4 0 0 0 5.6 0l3-3a4 4 0 0 0-5.6-5.6l-1 1M14 10a4 4 0 0 0-5.6 0l-3 3a4 4 0 0 0 5.6 5.6l1-1',
  copy: 'M8 8h11v11H8zM5 16V5h11',
  open: 'M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5',
  grip: 'M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01',

  // Chevrons & layout
  chevR: 'M9.5 6l6 6-6 6',
  chevL: 'M14.5 6l-6 6 6 6',
  chevD: 'M6 9.5l6 6 6-6',
  chevU: 'M6 14.5l6-6 6 6',
  up: 'M12 19V5M6 11l6-6 6 6',
  grid: 'M4 4h6.5v6.5H4zM13.5 4H20v6.5h-6.5zM4 13.5h6.5V20H4zM13.5 13.5H20V20h-6.5z',
  list: 'M8.5 6H20M8.5 12H20M8.5 18H20M4 6h.01M4 12h.01M4 18h.01',
  sort: 'M7 4v16M3.5 16.5 7 20l3.5-3.5M17 20V4M13.5 7.5 17 4l3.5 3.5',
  filter: 'M4 5h16l-6 7.5V18l-4 2v-7.5z',
  side: 'M4 4.5h16v15H4zM9.5 4.5v15',
  cols: 'M4 4.5h16v15H4zM9.5 4.5v15M14.5 4.5v15',
  mini: 'M3.5 5h17v14h-17zM12 12.5h6v4h-6z',
  full: 'M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5',
  exitFull: 'M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5',

  // Window controls
  min: 'M6 12h12',
  max: 'M6.5 6.5h11v11h-11z',

  // Files & devices
  note: 'M9 18V5l11-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM20 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z',
  file: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8zM14 3v5h5',
  image: 'M4 4h16v16H4zM4 16l5-5 4 4 3-3 4 4M15 8.5h.01',
  speaker: 'M6 3h12v18H6zM12 17a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM12 7h.01',
  bt: 'M7 7.5l10 9-5 4.5V3l5 4.5-10 9',
  tv: 'M3 5h18v12H3zM8 21h8M12 17v4',
  spectrum: 'M4 20v-5M8 20V9M12 20v-8M16 20V5M20 20v-9',
  waveform: 'M2 12h2M6 8v8M10 4.5v15M14 9v6M18 6.5v11M22 12h-2',

  // Settings sections
  general: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM3 12h3M18 12h3M12 3v3M12 18v3',
  palette:
    'M12 21a9 9 0 1 1 0-18c5 0 9 3.6 9 8 0 2.8-2.2 4-4.5 4H14a2 2 0 0 0-1.5 3.3A1.6 1.6 0 0 1 12 21zM7.5 11h.01M10 7.5h.01M15 7.5h.01',
  nowPlaying: 'M4 4h16v16H4zM4 16l5-5 4 4 3-3 4 4',
  keyboard: 'M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10',
} as const;

export type IconName = keyof typeof ICONS;
