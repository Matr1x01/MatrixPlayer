-- Application settings as JSON values keyed by a dotted name,
-- e.g. 'player.volume' = '0.8', 'ui.theme' = '"graphite"'.
-- Library tables (tracks, albums, artists, …) arrive with the library scanner.
CREATE TABLE settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL CHECK (json_valid(value)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
