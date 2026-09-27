// Sample library for the UI harness, taken from the design boards (Albums,
// Album, Artist, Songs, Folders and Search). Development only.

import type {
  AlbumDetail,
  AlbumRow,
  ArtistDetail,
  ArtistRow,
  FolderDetail,
  FolderNode,
  GenreRow,
  HistoryEntry,
  LibraryStats,
  PlaylistRow,
  RootInfo,
  ScanStatus,
  SearchResults,
  TrackList,
  TrackQuery,
  TrackRow,
} from '@/types/library';

/** Cover art approximations of the design's gradients, as SVG data URIs. */
const COLORS: Record<string, [string, string, string]> = {
  lfw: ['#1b2a40', '#3c3a55', '#ec8a3a'],
  salt: ['#3f6f7a', '#0f2a33', '#dee8e6'],
  north: ['#22262c', '#5f6672', '#d1453b'],
  glass: ['#0e3b4f', '#3aa6a0', '#e8f1e4'],
  paper: ['#1d3557', '#0b1422', '#f5efe0'],
  after: ['#ff7a59', '#ffd166', '#7bdff2'],
  night: ['#16142a', '#3a1f3f', '#f2c14e'],
  fjell: ['#2f4038', '#c7c0ab', '#e9e4d8'],
  vane: ['#edd3ae', '#d98e5a', '#b5542f'],
  contour: ['#2e3b2f', '#151c16', '#cbd5c0'],
  cinder: ['#140807', '#7a1e12', '#ff5a36'],
  tidal: ['#0c1a2b', '#1f4e6b', '#86b8c2'],
  rain: ['#34404f', '#11151b', '#c8dcff'],
  radar: ['#0f2a2a', '#081414', '#78dcb4'],
  soft: ['#2a2a2f', '#3b3b42', '#55555e'],
  orbit: ['#0b0b12', '#2a2438', '#e7d3ff'],
  holloway: ['#3e5227', '#1d2812', '#d8e3a4'],
  mistral: ['#f2e6d0', '#c9a882', '#8a5a3c'],
  ep: ['#3b2a4a', '#6d4a5f', '#d19a6a'],
  live: ['#232323', '#3d3a36', '#6a655d'],
  lantern: ['#2a2320', '#14100e', '#f6c56b'],
  quiet: ['#d8d2c4', '#bfb6a3', '#a39a88'],
};

export const ART: Record<string, string> = Object.fromEntries(
  Object.entries(COLORS).map(([id, [a, b, c]]) => [
    id,
    `data:image/svg+xml,${encodeURIComponent(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient><radialGradient id="r" cx="72" cy="30" r="46" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="${c}" stop-opacity=".9"/><stop offset="1" stop-color="${c}" stop-opacity="0"/></radialGradient></defs><rect width="100" height="100" fill="url(#g)"/><rect width="100" height="100" fill="url(#r)"/></svg>`,
    )}`,
  ]),
);

const now = Math.floor(Date.now() / 1000);
const fmt = (f: string): Pick<AlbumRow, 'codec' | 'lossless' | 'sampleRate' | 'bitDepth' | 'bitrateKbps'> => {
  const [codec, spec] = f.split(' ');
  if (spec?.includes('/')) {
    const [bits, khz] = spec.split('/');
    return { codec: codec!, lossless: true, bitDepth: Number(bits), sampleRate: Math.round(Number(khz) * 1000), bitrateKbps: null };
  }
  if (codec === 'DSD64') return { codec: 'DSD', lossless: true, bitDepth: 1, sampleRate: 2822400, bitrateKbps: null };
  return { codec: codec!, lossless: false, bitDepth: null, sampleRate: 44100, bitrateKbps: spec === 'V0' ? 245 : Number(spec) };
};

const RAW_ALBUMS: [string, string, number, string, string][] = [
  ['Night Transit', 'Brass Meridian', 2017, 'FLAC 16/44.1', 'night'],
  ['Tidal Notation', 'Fieldwork', 2019, 'FLAC 16/44.1', 'tidal'],
  ['Low Frequency Weather', 'Halden Reach', 2021, 'FLAC 24/96', 'lfw'],
  ['Coastal Radar', 'Halden Reach', 2018, 'FLAC 24/48', 'radar'],
  ['Afterimage Suite', 'Ines Takahara', 2022, 'FLAC 24/192', 'after'],
  ['Paper Satellites', 'Kestrel Row', 2020, 'MP3 320', 'paper'],
  ['Salt Lines', 'Marisol Ekdahl', 2019, 'FLAC 16/44.1', 'salt'],
  ['Cinder', 'Mirela Dusk', 2021, 'WAV 24/48', 'cinder'],
  ['Glass Harbour', 'Oda Varga', 2018, 'ALAC 24/96', 'glass'],
  ['Contour', 'Pale Cartographer', 2015, 'OPUS 160', 'contour'],
  ['Rain Studies', 'Rainer Holm', 2020, 'FLAC 24/48', 'rain'],
  ['Live at Fjellstue', 'Solveig Aune Trio', 2014, 'DSD64', 'fjell'],
  ['Weathervane', 'Tarn & Vale', 2024, 'FLAC 24/96', 'vane'],
  ['Northbound Static', 'The Quiet Engines', 2023, 'FLAC 24/48', 'north'],
  ['Soft Machinery', 'Umbra Collective', 2012, 'MP3 V0', 'soft'],
  ['Low Orbit', 'Vesna Lark', 2022, 'FLAC 24/44.1', 'orbit'],
  ['Holloway', 'Wren Atlas', 2019, 'AAC 256', 'holloway'],
  ['Mistral', 'Yara Solé', 2023, 'FLAC 24/96', 'mistral'],
  ['Thermocline', 'Halden Reach', 2016, 'FLAC 16/44.1', 'ep'],
  ['Live at the Observatory', 'Halden Reach', 2023, 'MP3 320', 'live'],
  ['Lantern Hours', 'Grey Parish', 2020, 'FLAC 16/44.1', 'lantern'],
  ['Quiet Rooms', 'Hana Mori', 2022, 'FLAC 24/44.1', 'quiet'],
];

const sortKey = (s: string) => s.toLowerCase().replace(/^the /, '');
const artistIds = new Map<string, number>();
const artistId = (name: string) => {
  if (!artistIds.has(name)) artistIds.set(name, artistIds.size + 1);
  return artistIds.get(name)!;
};

export const ALBUMS: AlbumRow[] = RAW_ALBUMS.map(([title, artist, year, format, art], i) => ({
  id: i + 1,
  title,
  artistName: artist,
  artistId: artistId(artist),
  year,
  artworkId: art,
  trackCount: title === 'Thermocline' ? 5 : title === 'Low Frequency Weather' ? 10 : 8 + (i % 5),
  durationMs: (40 + (i % 17)) * 60_000,
  totalSize: (200 + i * 37) * 1e6,
  ...fmt(format),
  addedAt: now - i * 86400 * 9,
  sortKey: sortKey(artist),
  titleKey: sortKey(title),
  titleDerived: false,
  playCount: [0, 12, 612, 80, 40][i % 5]!,
  lastPlayedAt: i % 3 ? now - i * 3600 : null,
  unavailable: 0,
}));
export const LFW = ALBUMS[2]!;

const LFW_TRACKS: [string, number, number, number][] = [
  ['Pressure Front', 258_000, 3021, 97.4],
  ['Isobar', 342_186, 3104, 132.4],
  ['Cloud Ceiling', 236_000, 2988, 88.3],
  ['Doldrums', 370_000, 3150, 145.7],
  ['Barometer', 273_000, 3077, 105.1],
  ['Katabatic', 422_000, 3212, 169.5],
  ['Rain Shadow', 201_000, 2944, 74.0],
  ['Dew Point', 315_000, 3066, 120.8],
  ['Lee Wave', 287_000, 3010, 108.1],
  ['Clear Night, Still Air', 484_000, 2901, 175.6],
];

function row(id: number, title: string, album: AlbumRow, n: number, ms: number, over: Partial<TrackRow> = {}): TrackRow {
  return {
    id,
    title,
    artist: album.artistName,
    artistId: album.artistId,
    album: album.title,
    albumId: album.id,
    albumArtist: album.artistName,
    genre: 'Ambient',
    year: album.year,
    trackNumber: n,
    discNumber: 1,
    codec: album.codec!,
    lossless: album.lossless,
    sampleRate: album.sampleRate,
    bitDepth: album.bitDepth,
    channels: 2,
    bitrateKbps: album.bitrateKbps ?? 3000,
    durationMs: ms,
    fileSize: ms * 388,
    fileName: `${String(n).padStart(2, '0')} - ${title}.flac`,
    folderId: 10,
    artworkId: album.artworkId,
    favorite: false,
    playCount: 0,
    lastPlayedAt: null,
    addedAt: now - 86400 * 30,
    available: true,
    titleDerived: false,
    unreadable: false,
    ...over,
  };
}

export const TRACKS = new Map<number, TrackRow>();
LFW_TRACKS.forEach(([title, ms, kbps, mb], i) =>
  TRACKS.set(i + 1, row(i + 1, title, LFW, i + 1, ms, { bitrateKbps: kbps, fileSize: mb * 1e6, playCount: [22, 38, 9, 14, 3, 31, 5, 7, 2, 19][i]! })),
);
// A few thousand more rows so the Songs table scrolls like a real library.
let nextId = 100;
for (const album of ALBUMS) {
  if (album === LFW) continue;
  for (let n = 1; n <= album.trackCount; n++) {
    const id = nextId++;
    TRACKS.set(id, row(id, `${album.title.split(' ')[0]} ${['Study', 'Motif', 'Theme', 'Interlude', 'Reprise', 'Coda', 'Variation', 'Etude', 'Dance', 'Air', 'Hymn', 'Signal'][n % 12]} ${n}`, album, n, 180_000 + n * 17_000, { genre: ['Ambient', 'Electronic', 'Folk', 'Jazz', 'Classical'][album.id % 5]! }));
  }
}
for (let i = 0; i < 1200; i++) {
  const album = ALBUMS[i % ALBUMS.length]!;
  const id = nextId++;
  TRACKS.set(id, row(id, `Archive Take ${i + 1}`, album, (i % 12) + 1, 150_000 + (i % 40) * 6000, { genre: 'Field Recordings' }));
}
const ALL_IDS = [...TRACKS.keys()];
export const FAVORITES = [2, 6, 10, 104, 131];

export const STATS: LibraryStats = {
  tracks: 12408,
  albums: 486,
  albumArtists: 214,
  genres: 38,
  playlists: 3,
  favorites: 342,
  totalSize: 486.2e9,
  durationMs: (38 * 24 + 4) * 3_600_000,
  unavailable: 0,
  roots: 2,
  hiResAlbums: 142,
  losslessAlbums: 391,
};

export const ROOTS: RootInfo[] = [
  { id: 1, display: '~/Music', folderId: 1, online: true, trackCount: 11_170, totalSize: 486e9, lastScanAt: now - 120 },
  { id: 2, display: '/mnt/nas/archive', folderId: 2, online: true, trackCount: 1238, totalSize: 1.9e12, lastScanAt: now - 3600 },
];

export const PLAYLISTS: PlaylistRow[] = [
  { id: 1, name: 'Late-night lossless', pinned: true, trackCount: 64, durationMs: 4.2 * 3_600_000, artworkId: 'night', createdAt: now - 86400 * 90, updatedAt: now - 86400 },
  { id: 2, name: 'Headphone reference', pinned: true, trackCount: 28, durationMs: 2.1 * 3_600_000, artworkId: 'glass', createdAt: now - 86400 * 60, updatedAt: now - 86400 * 3 },
  { id: 3, name: 'Long drives', pinned: true, trackCount: 112, durationMs: 8 * 3_600_000, artworkId: 'vane', createdAt: now - 86400 * 30, updatedAt: now - 86400 * 7 },
];

export const IDLE_SCAN: ScanStatus = {
  phase: 'idle',
  root: null,
  current: null,
  processed: 0,
  total: 0,
  found: 0,
  added: 0,
  updated: 0,
  removed: 0,
  unreadable: 0,
  offline: [],
  finishedAt: now - 120,
};
export const SCANNING: ScanStatus = {
  ...IDLE_SCAN,
  phase: 'reading',
  root: '~/Music',
  current: '/home/me/Music/Jazz/Hana Mori/01 Door Left Open.flac',
  processed: 4212,
  total: 12408,
  found: 12408,
  added: 4212,
};

export function artistRows(): ArtistRow[] {
  return [...artistIds.entries()]
    .map(([name, id]) => {
      const albums = ALBUMS.filter((a) => a.artistId === id);
      return { id, name, sortKey: sortKey(name), albumCount: albums.length, trackCount: albums.reduce((t, a) => t + a.trackCount, 0), artworkId: albums[0]?.artworkId ?? null };
    })
    .sort((a, b) => a.sortKey.localeCompare(b.sortKey));
}

export function albumDetail(id: number): AlbumDetail {
  const album = ALBUMS.find((a) => a.id === id) ?? LFW;
  const tracks = [...TRACKS.values()].filter((t) => t.albumId === album.id).sort((a, b) => (a.trackNumber ?? 0) - (b.trackNumber ?? 0));
  return {
    album,
    artwork: { id: album.artworkId!, width: 1200, height: 1200, source: 'folder', palette: [[163, 64, 43], [60, 58, 85], [236, 138, 58]] },
    genres: ['Ambient', 'Electronic'],
    tracks,
    discCount: 1,
    channels: 2,
    replayGain: { trackGain: null, trackPeak: null, albumGain: -6.2, albumPeak: 0.991 },
    moreByArtist: ALBUMS.filter((a) => a.artistId === album.artistId && a.id !== album.id),
    folderId: 10,
    folderPath: '/home/me/Music/Electronic/Halden Reach/2021 - Low Frequency Weather',
  };
}

export function artistDetail(id: number): ArtistDetail {
  const albums = ALBUMS.filter((a) => a.artistId === id);
  const name = [...artistIds.entries()].find(([, v]) => v === id)?.[0] ?? 'Unknown';
  const tracks = [...TRACKS.values()].filter((t) => t.artistId === id).sort((a, b) => b.playCount - a.playCount);
  return {
    id,
    name,
    artwork: albums[0] ? { id: albums[0].artworkId!, width: 1200, height: 1200, source: 'embedded', palette: [[236, 138, 58], [27, 36, 51], [58, 43, 42]] } : null,
    genres: ['Ambient', 'Electronic', 'Drone'],
    trackCount: tracks.length || 47,
    composedCount: 0,
    durationMs: 3.85 * 3_600_000,
    totalSize: 5.9e9,
    playCount: 612,
    firstYear: 2014,
    lastYear: 2023,
    firstAdded: now - 86400 * 2000,
    lastPlayed: now - 1800,
    albums,
    appearsOn: [ALBUMS[1]!, ALBUMS[6]!],
    topTracks: tracks.slice(0, 5),
    formats: [
      { label: 'FLAC 24-bit', hiRes: true, lossless: true, tracks: 31 },
      { label: 'FLAC 16-bit', hiRes: false, lossless: true, tracks: 11 },
      { label: 'MP3 320', hiRes: false, lossless: false, tracks: 5 },
    ],
    folders: albums.map((a, i) => ({ id: 10 + i, name: a.title, displayPath: `/home/me/Music/Electronic/${name}/${a.year} - ${a.title}`, trackCount: a.trackCount })),
    homeFolder: { id: 9, name, displayPath: `/home/me/Music/Electronic/${name}`, trackCount: 47 },
  };
}

export const GENRES: GenreRow[] = ['Ambient', 'Classical', 'Electronic', 'Field Recordings', 'Folk', 'Jazz', 'Drone', 'Indie'].map((name, i) => ({
  id: i + 1,
  name,
  trackCount: 300 + i * 211,
  albumCount: 12 + i * 5,
  durationMs: (20 + i * 7) * 3_600_000,
  artworkIds: ALBUMS.slice(i * 2, i * 2 + 4).map((a) => a.artworkId!),
}));

const node = (id: number, name: string, trackCount: number, hasChildren: boolean, artworkId: string | null = null): FolderNode => ({
  id,
  name,
  hasChildren,
  trackCount,
  totalSize: trackCount * 30e6,
  artworkId,
});
export const FOLDER_CHILDREN: Record<number, FolderNode[]> = {
  1: [node(3, 'Ambient', 1204, true), node(4, 'Classical', 2310, true), node(5, 'Electronic', 3877, true), node(6, 'Field Recordings', 318, true), node(7, 'Folk', 1466, true), node(8, 'Jazz', 2105, true), node(20, '_Incoming', 86, false)],
  2: [node(21, 'Vinyl rips 24-96', 1142, true), node(22, 'DSD', 96, true)],
  5: [node(30, 'Brass Meridian', 41, true), node(31, 'Fieldwork', 28, true), node(9, 'Halden Reach', 47, true), node(32, 'Mirela Dusk', 22, true)],
  9: [node(33, '2016 - Thermocline [EP]', 5, false, 'ep'), node(34, '2018 - Coastal Radar', 12, false, 'radar'), node(10, '2021 - Low Frequency Weather', 10, false, 'lfw'), node(35, '2023 - Live at the Observatory', 20, false, 'live')],
};

export function folderDetail(id: number): FolderDetail {
  const lfw = id === 10;
  return {
    id,
    name: lfw ? '2021 - Low Frequency Weather' : 'Halden Reach',
    displayPath: lfw ? '/home/me/Music/Electronic/Halden Reach/2021 - Low Frequency Weather' : '/home/me/Music/Electronic/Halden Reach',
    rootId: 1,
    rootOnline: true,
    crumbs: lfw
      ? [{ id: 1, name: '/home/me/Music' }, { id: 5, name: 'Electronic' }, { id: 9, name: 'Halden Reach' }]
      : [{ id: 1, name: '/home/me/Music' }, { id: 5, name: 'Electronic' }],
    parentId: lfw ? 9 : 5,
    subfolders: lfw ? [] : FOLDER_CHILDREN[9]!,
    tracks: lfw ? albumDetail(3).tracks : [],
    otherFiles: lfw
      ? [
          { name: 'cover.jpg', kind: 'image', size: 612_000, modified: now - 86400 * 1000 },
          { name: 'Low Frequency Weather.cue', kind: 'cue', size: 2000, modified: now - 86400 * 1000 },
          { name: 'Low Frequency Weather.log', kind: 'log', size: 9000, modified: now - 86400 * 1000 },
        ]
      : [],
    artwork: lfw ? { id: 'lfw', width: 1200, height: 1200, source: 'folder', palette: [[163, 64, 43], [60, 58, 85], [236, 138, 58]] } : null,
    albumId: lfw ? 3 : null,
    albumCount: lfw ? 1 : 4,
    lastScanAt: now - 120,
    tagged: lfw ? 10 : 0,
    totalTracks: lfw ? 10 : 47,
    totalSize: lfw ? 10 * 30e6 : 47 * 30e6,
    totalDurationMs: lfw ? 10 * 300_000 : 47 * 300_000,
  };
}

export function trackList(query: TrackQuery): TrackList {
  let ids = ALL_IDS;
  const scope = query.scope;
  if (scope.kind === 'album') ids = ALL_IDS.filter((id) => TRACKS.get(id)!.albumId === scope.id);
  if (scope.kind === 'artist') ids = ALL_IDS.filter((id) => TRACKS.get(id)!.artistId === scope.id);
  if (scope.kind === 'favorites') ids = FAVORITES;
  if (scope.kind === 'playlist') ids = ALL_IDS.slice(0, 24);
  if (query.filter) {
    const f = query.filter.toLowerCase();
    ids = ids.filter((id) => `${TRACKS.get(id)!.title} ${TRACKS.get(id)!.artist}`.toLowerCase().includes(f));
  }
  if (query.sort === 'plays') ids = [...ids].sort((a, b) => TRACKS.get(b)!.playCount - TRACKS.get(a)!.playCount);
  const rows = ids.map((id) => TRACKS.get(id)!);
  return { ids, durationMs: rows.reduce((t, r) => t + r.durationMs, 0), totalSize: rows.reduce((t, r) => t + r.fileSize, 0) };
}

export function history(): HistoryEntry[] {
  return [2, 6, 1, 104, 131, 10, 4].map((id, i) => ({ playedAt: now - i * 2400 - (i > 3 ? 86400 : 0), listenedMs: 200_000, track: TRACKS.get(id)! }));
}

export function search(text: string): SearchResults {
  const q = text.toLowerCase();
  const tracks = [...TRACKS.values()].filter((t) => t.title.toLowerCase().includes(q)).slice(0, 40);
  const albums = ALBUMS.filter((a) => a.title.toLowerCase().includes(q) || a.artistName.toLowerCase().includes(q));
  const artists = artistRows().filter((a) => a.name.toLowerCase().includes(q));
  return {
    query: text,
    tookMs: 3,
    top: albums[0] ? { kind: 'album', album: albums[0] } : tracks[0] ? { kind: 'track', track: tracks[0] } : null,
    tracks,
    trackCount: tracks.length,
    albums,
    artists,
    genres: GENRES.filter((g) => g.name.toLowerCase().includes(q)),
    folders: q.startsWith('rain')
      ? [
          { id: 40, name: 'Rain', displayPath: '/home/me/Music/Field Recordings/Rain', trackCount: 48, totalSize: 2.1e9 },
          { id: 41, name: 'Rain on Skylight', displayPath: '/mnt/nas/archive/Field/Rain on Skylight', trackCount: 6, totalSize: 912e6 },
        ]
      : [],
    playlists: PLAYLISTS.filter((p) => p.name.toLowerCase().includes(q)),
  };
}
