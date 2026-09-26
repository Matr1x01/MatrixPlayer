-- The music library: folders the user added, and everything indexed in them.
--
-- Paths are stored as raw bytes (BLOB) because Linux file names need not be
-- UTF-8; `display` columns hold a lossy UTF-8 copy for showing and sorting.
-- Every `*_key` column is a case- and accent-folded form computed in Rust,
-- used for grouping and filtering.

CREATE TABLE library_roots (
    id           INTEGER PRIMARY KEY,
    path         BLOB    NOT NULL UNIQUE,
    display      TEXT    NOT NULL,
    added_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    last_scan_at INTEGER
) STRICT;

-- Every directory under a root that holds audio (or leads to some).
CREATE TABLE folders (
    id        INTEGER PRIMARY KEY,
    root_id   INTEGER NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
    parent_id INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    path      BLOB    NOT NULL UNIQUE,
    name      TEXT    NOT NULL,
    name_key  TEXT    NOT NULL
) STRICT;
CREATE INDEX folders_parent ON folders(parent_id);
CREATE INDEX folders_root ON folders(root_id);

-- Artists, album artists and composers share one namespace.
CREATE TABLE artists (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL,
    name_key  TEXT NOT NULL UNIQUE,
    sort_key  TEXT NOT NULL
) STRICT;

CREATE TABLE albums (
    id           INTEGER PRIMARY KEY,
    -- Identity: album artist + title, or folder + title when the files carry
    -- no album artist (so compilations and same-named albums stay apart).
    group_key    TEXT    NOT NULL UNIQUE,
    title        TEXT    NOT NULL,
    title_key    TEXT    NOT NULL,
    -- NULL for compilations ("Various artists") and unknown artists.
    artist_id    INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    artist_name  TEXT    NOT NULL,
    sort_key     TEXT    NOT NULL,
    -- True when the title was taken from the folder name.
    title_derived INTEGER NOT NULL DEFAULT 0,
    -- Aggregates, refreshed after every scan.
    year          INTEGER,
    artwork_id    TEXT,
    track_count   INTEGER NOT NULL DEFAULT 0,
    duration_ms   INTEGER NOT NULL DEFAULT 0,
    total_size    INTEGER NOT NULL DEFAULT 0,
    -- The format most of the album's tracks share, for its badge.
    codec         TEXT,
    lossless      INTEGER NOT NULL DEFAULT 0,
    sample_rate   INTEGER,
    bit_depth     INTEGER,
    bitrate_kbps  INTEGER,
    added_at      INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX albums_artist ON albums(artist_id);

CREATE TABLE genres (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL,
    name_key TEXT NOT NULL UNIQUE
) STRICT;

-- Decoded once per image; lets tracks be rebuilt from the database without
-- touching the artwork files.
CREATE TABLE artwork (
    id      TEXT    PRIMARY KEY,
    width   INTEGER NOT NULL,
    height  INTEGER NOT NULL,
    source  TEXT    NOT NULL,
    palette TEXT    NOT NULL CHECK (json_valid(palette))
) STRICT;

CREATE TABLE tracks (
    id              INTEGER PRIMARY KEY,
    root_id         INTEGER NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    path            BLOB    NOT NULL UNIQUE,
    display_path    TEXT    NOT NULL,
    file_name       TEXT    NOT NULL,
    file_size       INTEGER NOT NULL,
    mtime           INTEGER NOT NULL,
    -- False while the file's root is offline (an unmounted drive).
    available       INTEGER NOT NULL DEFAULT 1,

    title           TEXT    NOT NULL,
    artist          TEXT,
    album           TEXT,
    album_artist    TEXT,
    genre           TEXT,
    composer        TEXT,
    year            INTEGER,
    track_number    INTEGER,
    track_total     INTEGER,
    disc_number     INTEGER,
    disc_total      INTEGER,
    comment         TEXT,
    copyright       TEXT,
    tag_format      TEXT,
    tag_item_count  INTEGER NOT NULL DEFAULT 0,
    derived_fields  TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(derived_fields)),
    rg_track_gain   REAL,
    rg_track_peak   REAL,
    rg_album_gain   REAL,
    rg_album_peak   REAL,

    codec           TEXT    NOT NULL,
    container       TEXT    NOT NULL,
    lossless        INTEGER NOT NULL,
    sample_rate     INTEGER,
    bit_depth       INTEGER,
    channels        INTEGER,
    bitrate_kbps    INTEGER,
    duration_ms     INTEGER NOT NULL,
    probe_error     TEXT,

    artwork_id      TEXT REFERENCES artwork(id),
    artist_id       INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    album_id        INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    composer_id     INTEGER REFERENCES artists(id) ON DELETE SET NULL,

    -- Sort keys (folded, leading articles moved) for fast ORDER BY.
    title_key       TEXT    NOT NULL,
    artist_key      TEXT    NOT NULL,
    album_key       TEXT    NOT NULL,

    added_at        INTEGER NOT NULL DEFAULT (unixepoch()),
    -- Listening data, kept across rescans and renames.
    favorite_at     INTEGER,
    play_count      INTEGER NOT NULL DEFAULT 0,
    last_played_at  INTEGER
) STRICT;
CREATE INDEX tracks_album ON tracks(album_id, disc_number, track_number);
CREATE INDEX tracks_artist ON tracks(artist_id);
CREATE INDEX tracks_composer ON tracks(composer_id);
CREATE INDEX tracks_folder ON tracks(folder_id);
CREATE INDEX tracks_root ON tracks(root_id);
CREATE INDEX tracks_sort_artist ON tracks(artist_key, album_key, disc_number, track_number);
CREATE INDEX tracks_favorite ON tracks(favorite_at) WHERE favorite_at IS NOT NULL;
CREATE INDEX tracks_last_played ON tracks(last_played_at) WHERE last_played_at IS NOT NULL;
CREATE INDEX tracks_added ON tracks(added_at);

CREATE TABLE track_genres (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    genre_id INTEGER NOT NULL REFERENCES genres(id) ON DELETE CASCADE,
    PRIMARY KEY (track_id, genre_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX track_genres_genre ON track_genres(genre_id);

-- Word-prefix search over tracks. Rows are written by the scanner in the same
-- transaction as the track (rowid = tracks.id) and removed with it, including
-- when a whole root is removed.
CREATE VIRTUAL TABLE track_search USING fts5(
    title, artist, album, genre, composer, file_name,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER tracks_search_delete AFTER DELETE ON tracks BEGIN
    DELETE FROM track_search WHERE rowid = old.id;
END;

-- Every time a track was played for long enough to count.
CREATE TABLE play_history (
    id        INTEGER PRIMARY KEY,
    track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    played_at INTEGER NOT NULL,
    -- How much was heard, to tell full listens from skips.
    listened_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX play_history_time ON play_history(played_at);
CREATE INDEX play_history_track ON play_history(track_id);

CREATE TABLE playlists (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    pinned     INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    PRIMARY KEY (playlist_id, position)
) STRICT, WITHOUT ROWID;
CREATE INDEX playlist_tracks_track ON playlist_tracks(track_id);
