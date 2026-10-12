-- Lyrics looked up online (LRCLIB), so each track is asked about once.
-- Misses are kept too, with when they were looked up, and asked about again
-- after a while: lyrics are added to LRCLIB all the time.
CREATE TABLE online_lyrics (
    -- The track as it was asked about: artist, title, album and length,
    -- normalised (see metadata::online_lyrics::cache_key).
    key        TEXT    PRIMARY KEY NOT NULL,
    -- 'found', 'instrumental' or 'missing'.
    status     TEXT    NOT NULL CHECK (status IN ('found', 'instrumental', 'missing')),
    synced     TEXT,
    plain      TEXT,
    fetched_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
