// Mirrors of the Rust library types (src-tauri/src/library).

import type { ArtworkInfo, InsertMode, ReplayGain } from './player';

export type ScanPhase = 'idle' | 'discovering' | 'reading' | 'finishing';

export interface ScanStatus {
  phase: ScanPhase;
  root: string | null;
  current: string | null;
  processed: number;
  total: number;
  found: number;
  added: number;
  updated: number;
  removed: number;
  unreadable: number;
  offline: string[];
  /** Seconds since the epoch. */
  finishedAt: number | null;
}

export type LibraryChange = 'tracks' | 'roots' | 'favorites' | 'history' | 'playlists';

export interface LibraryStats {
  tracks: number;
  albums: number;
  albumArtists: number;
  genres: number;
  playlists: number;
  favorites: number;
  totalSize: number;
  durationMs: number;
  unavailable: number;
  roots: number;
  hiResAlbums: number;
  losslessAlbums: number;
}

export interface RootInfo {
  id: number;
  display: string;
  folderId: number | null;
  online: boolean;
  trackCount: number;
  totalSize: number;
  lastScanAt: number | null;
}

export interface DefaultRoot {
  display: string;
  files: number;
}

export interface TrackRow {
  id: number;
  title: string;
  artist: string | null;
  artistId: number | null;
  album: string | null;
  albumId: number | null;
  albumArtist: string | null;
  genre: string | null;
  year: number | null;
  trackNumber: number | null;
  discNumber: number | null;
  codec: string;
  lossless: boolean;
  sampleRate: number | null;
  bitDepth: number | null;
  channels: number | null;
  bitrateKbps: number | null;
  durationMs: number;
  fileSize: number;
  fileName: string;
  folderId: number;
  artworkId: string | null;
  favorite: boolean;
  playCount: number;
  lastPlayedAt: number | null;
  addedAt: number;
  available: boolean;
  titleDerived: boolean;
  unreadable: boolean;
}

export interface AlbumRow {
  id: number;
  title: string;
  artistName: string;
  artistId: number | null;
  year: number | null;
  artworkId: string | null;
  trackCount: number;
  durationMs: number;
  totalSize: number;
  codec: string | null;
  lossless: boolean;
  sampleRate: number | null;
  bitDepth: number | null;
  bitrateKbps: number | null;
  addedAt: number;
  sortKey: string;
  titleKey: string;
  titleDerived: boolean;
  playCount: number;
  lastPlayedAt: number | null;
  unavailable: number;
}

export interface AlbumDetail {
  album: AlbumRow;
  artwork: ArtworkInfo | null;
  genres: string[];
  tracks: TrackRow[];
  discCount: number;
  channels: number | null;
  replayGain: ReplayGain;
  moreByArtist: AlbumRow[];
  folderId: number | null;
  folderPath: string | null;
}

export type ArtistKind = 'albumArtists' | 'all' | 'composers';

export interface ArtistRow {
  id: number;
  name: string;
  sortKey: string;
  albumCount: number;
  trackCount: number;
  artworkId: string | null;
}

export interface FormatShare {
  label: string;
  hiRes: boolean;
  lossless: boolean;
  tracks: number;
}

export interface FolderRef {
  id: number;
  name: string;
  displayPath: string;
  trackCount: number;
}

export interface ArtistDetail {
  id: number;
  name: string;
  artwork: ArtworkInfo | null;
  genres: string[];
  trackCount: number;
  composedCount: number;
  durationMs: number;
  totalSize: number;
  playCount: number;
  firstYear: number | null;
  lastYear: number | null;
  firstAdded: number | null;
  lastPlayed: number | null;
  albums: AlbumRow[];
  appearsOn: AlbumRow[];
  topTracks: TrackRow[];
  formats: FormatShare[];
  folders: FolderRef[];
  homeFolder: FolderRef | null;
}

export interface GenreRow {
  id: number;
  name: string;
  trackCount: number;
  albumCount: number;
  durationMs: number;
  artworkIds: string[];
}

export interface GenreDetail {
  genre: GenreRow;
  albums: AlbumRow[];
  artists: ArtistRow[];
}

export interface FolderNode {
  id: number;
  name: string;
  hasChildren: boolean;
  trackCount: number;
  totalSize: number;
  artworkId: string | null;
}

export interface OtherFile {
  name: string;
  kind: 'image' | 'cue' | 'log' | 'playlist' | 'text' | 'file';
  size: number;
  modified: number | null;
}

export interface FolderDetail {
  id: number;
  name: string;
  displayPath: string;
  rootId: number;
  rootOnline: boolean;
  crumbs: { id: number; name: string }[];
  parentId: number | null;
  subfolders: FolderNode[];
  tracks: TrackRow[];
  otherFiles: OtherFile[];
  artwork: ArtworkInfo | null;
  albumId: number | null;
  albumCount: number;
  lastScanAt: number | null;
  tagged: number;
  /** Tracks, bytes and playing time here and in every folder below. */
  totalTracks: number;
  totalSize: number;
  totalDurationMs: number;
}

export type TrackSort =
  | 'default'
  | 'title'
  | 'artist'
  | 'album'
  | 'year'
  | 'genre'
  | 'format'
  | 'sampleRate'
  | 'bitDepth'
  | 'size'
  | 'duration'
  | 'added'
  | 'plays'
  | 'lastPlayed';

export type TrackScope =
  | { kind: 'all' }
  | { kind: 'favorites' }
  | { kind: 'recent' }
  | { kind: 'album'; id: number }
  | { kind: 'artist'; id: number }
  | { kind: 'composer'; id: number }
  | { kind: 'genre'; id: number }
  | { kind: 'folder'; id: number; recursive: boolean }
  | { kind: 'playlist'; id: number };

export interface TrackQuery {
  scope: TrackScope;
  filter?: string;
  sort?: TrackSort;
  descending?: boolean;
  hiResOnly?: boolean;
}

export interface TrackList {
  ids: number[];
  durationMs: number;
  totalSize: number;
}

export type PlaySource = { kind: 'tracks'; ids: number[] } | { kind: 'query'; query: TrackQuery };

export interface PlayOptions {
  mode?: InsertMode;
  start?: number;
  shuffle?: boolean;
}

export interface PlaylistRow {
  id: number;
  name: string;
  pinned: boolean;
  trackCount: number;
  durationMs: number;
  artworkId: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface FolderHit {
  id: number;
  name: string;
  displayPath: string;
  trackCount: number;
  totalSize: number;
}

export type TopResult =
  | { kind: 'album'; album: AlbumRow }
  | { kind: 'artist'; artist: ArtistRow }
  | { kind: 'track'; track: TrackRow };

export interface SearchResults {
  query: string;
  tookMs: number;
  top: TopResult | null;
  tracks: TrackRow[];
  trackCount: number;
  albums: AlbumRow[];
  artists: ArtistRow[];
  genres: GenreRow[];
  folders: FolderHit[];
  playlists: PlaylistRow[];
}

export interface HistoryEntry {
  playedAt: number;
  listenedMs: number;
  track: TrackRow;
}
