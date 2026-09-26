//! Writes scanned tracks into the library tables.
//!
//! An [`Indexer`] lives for one root's scan. It caches the ids of folders,
//! artists, genres and albums it has already resolved so each batch costs a
//! handful of statements per track, and remembers which albums it touched so
//! only their aggregates are recomputed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use sqlx::{Sqlite, Transaction};

use super::paths::path_bytes;
use super::text::{fold, sort_key};
use crate::artwork::ArtworkInfo;
use crate::database::Database;
use crate::error::AppResult;
use crate::player::TrackInfo;

/// Album-artist value that marks a compilation.
pub const VARIOUS_ARTISTS: &str = "Various artists";
pub const UNKNOWN_ARTIST: &str = "Unknown artist";

/// One file read by the scanner, ready to be written.
pub struct ScannedTrack {
    pub info: TrackInfo,
    pub size: u64,
    pub mtime: i64,
    /// The row this file updates: the same path, or a file that moved here.
    pub existing_id: Option<i64>,
}

pub struct Indexer {
    root_id: i64,
    root: PathBuf,
    folders: HashMap<PathBuf, i64>,
    artists: HashMap<String, i64>,
    genres: HashMap<String, i64>,
    albums: HashMap<String, i64>,
    artwork: HashSet<String>,
    pub touched_albums: HashSet<i64>,
}

impl Indexer {
    pub fn new(root_id: i64, root: PathBuf) -> Self {
        Self {
            root_id,
            root,
            folders: HashMap::new(),
            artists: HashMap::new(),
            genres: HashMap::new(),
            albums: HashMap::new(),
            artwork: HashSet::new(),
            touched_albums: HashSet::new(),
        }
    }

    /// Writes a batch of tracks in one transaction. Returns their ids.
    pub async fn write(&mut self, db: &Database, batch: &[ScannedTrack]) -> AppResult<Vec<i64>> {
        let mut tx = db.pool().begin().await?;
        let mut ids = Vec::with_capacity(batch.len());
        for track in batch {
            ids.push(self.write_one(&mut tx, track).await?);
        }
        tx.commit().await?;
        Ok(ids)
    }

    async fn write_one(&mut self, tx: &mut Transaction<'_, Sqlite>, scanned: &ScannedTrack) -> AppResult<i64> {
        let info = &scanned.info;
        let md = &info.metadata;
        let props = &info.properties;
        let dir = info.path.parent().unwrap_or(&self.root).to_path_buf();
        let folder_id = self.folder(tx, &dir).await?;

        let artist_id = match &md.artist {
            Some(name) => Some(self.artist(tx, name).await?),
            None => None,
        };
        let composer_id = match &md.composer {
            Some(name) => Some(self.artist(tx, name).await?),
            None => None,
        };
        let album_id = self.album(tx, info, &dir).await?;
        self.touched_albums.insert(album_id);
        if let Some(art) = &info.artwork {
            self.store_artwork(tx, art).await?;
        }

        let derived = serde_json::to_string(&md.derived_fields).expect("strings serialise");
        let artist_sort = sort_key(md.artist.as_deref().unwrap_or(""));
        let album_sort = sort_key(md.album.as_deref().unwrap_or(""));
        let query = if scanned.existing_id.is_some() {
            "UPDATE tracks SET
                root_id = ?1, folder_id = ?2, path = ?3, display_path = ?4, file_name = ?5, file_size = ?6,
                mtime = ?7, available = 1, title = ?8, artist = ?9, album = ?10, album_artist = ?11, genre = ?12,
                composer = ?13, year = ?14, track_number = ?15, track_total = ?16, disc_number = ?17,
                disc_total = ?18, comment = ?19, copyright = ?20, tag_format = ?21, tag_item_count = ?22,
                derived_fields = ?23, rg_track_gain = ?24, rg_track_peak = ?25, rg_album_gain = ?26,
                rg_album_peak = ?27, codec = ?28, container = ?29, lossless = ?30, sample_rate = ?31,
                bit_depth = ?32, channels = ?33, bitrate_kbps = ?34, duration_ms = ?35, probe_error = ?36,
                artwork_id = ?37, artist_id = ?38, album_id = ?39, composer_id = ?40, title_key = ?41,
                artist_key = ?42, album_key = ?43
             WHERE id = ?44
             RETURNING id"
        } else {
            "INSERT INTO tracks (
                root_id, folder_id, path, display_path, file_name, file_size, mtime, title, artist, album,
                album_artist, genre, composer, year, track_number, track_total, disc_number, disc_total, comment,
                copyright, tag_format, tag_item_count, derived_fields, rg_track_gain, rg_track_peak, rg_album_gain,
                rg_album_peak, codec, container, lossless, sample_rate, bit_depth, channels, bitrate_kbps,
                duration_ms, probe_error, artwork_id, artist_id, album_id, composer_id, title_key, artist_key,
                album_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40,
                ?41, ?42, ?43)
             RETURNING id"
        };
        let mut statement = sqlx::query_as(query)
            .bind(self.root_id)
            .bind(folder_id)
            .bind(path_bytes(&info.path))
            .bind(&info.display_path)
            .bind(&info.file_name)
            .bind(i64::try_from(scanned.size).unwrap_or(i64::MAX))
            .bind(scanned.mtime)
            .bind(&md.title)
            .bind(&md.artist)
            .bind(&md.album)
            .bind(&md.album_artist)
            .bind(&md.genre)
            .bind(&md.composer)
            .bind(md.year)
            .bind(md.track_number)
            .bind(md.track_total)
            .bind(md.disc_number)
            .bind(md.disc_total)
            .bind(&md.comment)
            .bind(&md.copyright)
            .bind(&md.tag_format)
            .bind(i64::try_from(md.tag_item_count).unwrap_or(0))
            .bind(derived)
            .bind(md.replay_gain.track_gain)
            .bind(md.replay_gain.track_peak)
            .bind(md.replay_gain.album_gain)
            .bind(md.replay_gain.album_peak)
            .bind(&props.codec)
            .bind(&props.container)
            .bind(props.lossless)
            .bind(props.sample_rate)
            .bind(props.bit_depth)
            .bind(props.channels)
            .bind(props.bitrate_kbps)
            .bind(i64::try_from(props.duration_ms).unwrap_or(i64::MAX))
            .bind(&info.probe_error)
            .bind(info.artwork.as_ref().map(|a| a.id.as_str()))
            .bind(artist_id)
            .bind(album_id)
            .bind(composer_id)
            .bind(sort_key(&md.title))
            .bind(artist_sort)
            .bind(album_sort);
        if let Some(existing) = scanned.existing_id {
            statement = statement.bind(existing);
        }
        let (id,): (i64,) = statement.fetch_one(&mut **tx).await?;

        // Search row and genres are rewritten wholesale.
        sqlx::query("DELETE FROM track_search WHERE rowid = ?").bind(id).execute(&mut **tx).await?;
        let artists = [md.artist.as_deref(), md.album_artist.as_deref()].into_iter().flatten().collect::<Vec<_>>();
        sqlx::query(
            "INSERT INTO track_search (rowid, title, artist, album, genre, composer, file_name)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(&md.title)
        .bind(artists.join(" "))
        .bind(&md.album)
        .bind(&md.genre)
        .bind(&md.composer)
        .bind(&info.file_name)
        .execute(&mut **tx)
        .await?;

        sqlx::query("DELETE FROM track_genres WHERE track_id = ?").bind(id).execute(&mut **tx).await?;
        for name in split_genres(md.genre.as_deref()) {
            let genre_id = self.genre(tx, name).await?;
            sqlx::query("INSERT OR IGNORE INTO track_genres (track_id, genre_id) VALUES (?, ?)")
                .bind(id)
                .bind(genre_id)
                .execute(&mut **tx)
                .await?;
        }
        Ok(id)
    }

    /// The folder row for `dir`, creating it and any missing ancestors up to
    /// the root.
    async fn folder(&mut self, tx: &mut Transaction<'_, Sqlite>, dir: &Path) -> AppResult<i64> {
        if let Some(&id) = self.folders.get(dir) {
            return Ok(id);
        }
        let parent_id = match dir.parent() {
            Some(parent) if dir != self.root && parent.starts_with(&self.root) => {
                Some(Box::pin(self.folder(tx, parent)).await?)
            }
            _ => None,
        };
        let name = if dir == self.root {
            dir.display().to_string()
        } else {
            dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned())
        };
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO folders (root_id, parent_id, path, name, name_key) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET root_id = excluded.root_id, parent_id = excluded.parent_id,
                name = excluded.name
             RETURNING id",
        )
        .bind(self.root_id)
        .bind(parent_id)
        .bind(path_bytes(dir))
        .bind(&name)
        .bind(sort_key(&name))
        .fetch_one(&mut **tx)
        .await?;
        self.folders.insert(dir.to_path_buf(), id);
        Ok(id)
    }

    async fn artist(&mut self, tx: &mut Transaction<'_, Sqlite>, name: &str) -> AppResult<i64> {
        let key = fold(name);
        if let Some(&id) = self.artists.get(&key) {
            return Ok(id);
        }
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO artists (name, name_key, sort_key) VALUES (?, ?, ?)
             ON CONFLICT(name_key) DO UPDATE SET name = name
             RETURNING id",
        )
        .bind(name)
        .bind(&key)
        .bind(sort_key(name))
        .fetch_one(&mut **tx)
        .await?;
        self.artists.insert(key, id);
        Ok(id)
    }

    async fn genre(&mut self, tx: &mut Transaction<'_, Sqlite>, name: &str) -> AppResult<i64> {
        let key = fold(name);
        if let Some(&id) = self.genres.get(&key) {
            return Ok(id);
        }
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO genres (name, name_key) VALUES (?, ?)
             ON CONFLICT(name_key) DO UPDATE SET name = name
             RETURNING id",
        )
        .bind(name)
        .bind(&key)
        .fetch_one(&mut **tx)
        .await?;
        self.genres.insert(key, id);
        Ok(id)
    }

    async fn album(&mut self, tx: &mut Transaction<'_, Sqlite>, info: &TrackInfo, dir: &Path) -> AppResult<i64> {
        let md = &info.metadata;
        let album_dir = album_folder(dir);
        let (title, title_derived) = match &md.album {
            Some(title) => (title.clone(), false),
            None => (
                album_dir.file_name().map_or_else(|| "Unknown album".to_owned(), |n| n.to_string_lossy().into_owned()),
                true,
            ),
        };
        let group_key = match (&md.album_artist, &md.album) {
            (Some(album_artist), Some(album)) => format!("aa:{}\u{1f}{}", fold(album_artist), fold(album)),
            _ => format!("dir:{}\u{1f}{}", album_dir.display(), md.album.as_deref().map(fold).unwrap_or_default()),
        };
        if let Some(&id) = self.albums.get(&group_key) {
            return Ok(id);
        }
        let (artist_id, artist_name) = match &md.album_artist {
            Some(name) => (Some(self.artist(tx, name).await?), name.clone()),
            // Settled from all of the album's tracks in `refresh_albums`.
            None => (None, md.artist.clone().unwrap_or_else(|| UNKNOWN_ARTIST.to_owned())),
        };
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO albums (group_key, title, title_key, artist_id, artist_name, sort_key, title_derived)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(group_key) DO UPDATE SET title = excluded.title
             RETURNING id",
        )
        .bind(&group_key)
        .bind(&title)
        .bind(sort_key(&title))
        .bind(artist_id)
        .bind(&artist_name)
        .bind(sort_key(&artist_name))
        .bind(title_derived)
        .fetch_one(&mut **tx)
        .await?;
        self.albums.insert(group_key, id);
        Ok(id)
    }

    async fn store_artwork(&mut self, tx: &mut Transaction<'_, Sqlite>, art: &ArtworkInfo) -> AppResult<()> {
        if self.artwork.contains(&art.id) {
            return Ok(());
        }
        let source = serde_json::to_value(art.source).expect("enum serialises");
        sqlx::query("INSERT OR IGNORE INTO artwork (id, width, height, source, palette) VALUES (?, ?, ?, ?, ?)")
            .bind(&art.id)
            .bind(art.width)
            .bind(art.height)
            .bind(source.as_str().unwrap_or("embedded"))
            .bind(serde_json::to_string(&art.palette).expect("palette serialises"))
            .execute(&mut **tx)
            .await?;
        self.artwork.insert(art.id.clone());
        Ok(())
    }
}

/// Multi-valued genre tags ("Ambient; Electronic") become separate genres.
/// Slashes are left alone: "R&B/Soul" is usually one genre.
pub fn split_genres(genre: Option<&str>) -> impl Iterator<Item = &str> {
    genre.into_iter().flat_map(|g| g.split([';', '\0'])).map(str::trim).filter(|g| !g.is_empty())
}

/// The folder an album lives in: the track's folder, or its parent when the
/// track sits in a per-disc subfolder ("CD 2", "Disc 1").
pub fn album_folder(dir: &Path) -> &Path {
    let is_disc = dir.file_name().and_then(|n| n.to_str()).is_some_and(|name| {
        let lower = name.trim().to_ascii_lowercase();
        ["cd", "disc", "disk"].iter().any(|prefix| {
            lower.strip_prefix(prefix).is_some_and(|rest| {
                let rest = rest.trim_start_matches([' ', '-', '_', '.']);
                !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
            })
        })
    });
    match dir.parent() {
        Some(parent) if is_disc => parent,
        _ => dir,
    }
}

/// Recomputes the stored aggregates of `albums` (or of every album) from
/// their tracks, settles the artist of albums without an album-artist tag,
/// and removes albums, artists, genres and folders nothing refers to.
pub async fn refresh_albums(db: &Database, albums: Option<&[i64]>) -> AppResult<()> {
    let mut tx = db.pool().begin().await?;
    let ids: Vec<i64> = match albums {
        Some(ids) => ids.to_vec(),
        None => sqlx::query_scalar("SELECT id FROM albums").fetch_all(&mut *tx).await?,
    };
    for chunk in ids.chunks(400) {
        let list = chunk.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        sqlx::query(&format!(
            "UPDATE albums SET
                track_count = (SELECT COUNT(*) FROM tracks t WHERE t.album_id = albums.id),
                duration_ms = (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks t WHERE t.album_id = albums.id),
                total_size  = (SELECT COALESCE(SUM(file_size), 0) FROM tracks t WHERE t.album_id = albums.id),
                year = (SELECT year FROM tracks t WHERE t.album_id = albums.id AND year IS NOT NULL
                        GROUP BY year ORDER BY COUNT(*) DESC, year DESC LIMIT 1),
                artwork_id = (SELECT artwork_id FROM tracks t WHERE t.album_id = albums.id AND artwork_id IS NOT NULL
                        ORDER BY disc_number, track_number, title_key LIMIT 1),
                (codec, lossless, sample_rate, bit_depth, bitrate_kbps) = (
                    SELECT CASE WHEN codec = 'PCM' THEN container ELSE codec END, lossless, sample_rate, bit_depth,
                        CAST(AVG(bitrate_kbps) AS INTEGER)
                    FROM tracks t WHERE t.album_id = albums.id
                    GROUP BY codec, sample_rate, bit_depth ORDER BY COUNT(*) DESC LIMIT 1)
             WHERE id IN ({list}) AND EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = albums.id)"
        ))
        .execute(&mut *tx)
        .await?;

        // Albums without an album-artist tag take the artist most of their
        // tracks share, or become a compilation.
        let untagged: Vec<(i64,)> =
            sqlx::query_as(&format!("SELECT id FROM albums WHERE id IN ({list}) AND group_key LIKE 'dir:%'"))
                .fetch_all(&mut *tx)
                .await?;
        for (album_id,) in untagged {
            let top: Option<(Option<i64>, Option<String>, i64, i64)> = sqlx::query_as(
                "SELECT artist_id, artist, COUNT(*) AS n, (SELECT COUNT(*) FROM tracks WHERE album_id = ?1)
                 FROM tracks WHERE album_id = ?1 GROUP BY artist_key ORDER BY n DESC LIMIT 1",
            )
            .bind(album_id)
            .fetch_optional(&mut *tx)
            .await?;
            let (artist_id, name) = match top {
                Some((id, Some(name), n, total)) if n * 2 > total => (id, name),
                Some((_, None, n, total)) if n * 2 > total => (None, UNKNOWN_ARTIST.to_owned()),
                Some(_) => (None, VARIOUS_ARTISTS.to_owned()),
                None => continue,
            };
            sqlx::query("UPDATE albums SET artist_id = ?, artist_name = ?, sort_key = ? WHERE id = ?")
                .bind(artist_id)
                .bind(&name)
                .bind(sort_key(&name))
                .bind(album_id)
                .execute(&mut *tx)
                .await?;
        }
    }
    prune(&mut tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Deletes rows that no track refers to any more.
async fn prune(tx: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    sqlx::query("DELETE FROM albums WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE album_id = albums.id)")
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "DELETE FROM artists WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE artist_id = artists.id)
            AND NOT EXISTS (SELECT 1 FROM tracks WHERE composer_id = artists.id)
            AND NOT EXISTS (SELECT 1 FROM albums WHERE artist_id = artists.id)",
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM genres WHERE NOT EXISTS (SELECT 1 FROM track_genres WHERE genre_id = genres.id)")
        .execute(&mut **tx)
        .await?;
    // Keep every folder that holds a track or leads to one, and the roots.
    sqlx::query(
        "WITH RECURSIVE keep(id) AS (
            SELECT DISTINCT folder_id FROM tracks
            UNION SELECT f.parent_id FROM folders f JOIN keep ON f.id = keep.id WHERE f.parent_id IS NOT NULL
         )
         DELETE FROM folders WHERE parent_id IS NOT NULL AND id NOT IN (SELECT id FROM keep)",
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM artwork WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE artwork_id = artwork.id)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_folders_belong_to_their_parent_album() {
        assert_eq!(album_folder(Path::new("/m/Album/CD 2")), Path::new("/m/Album"));
        assert_eq!(album_folder(Path::new("/m/Album/Disc1")), Path::new("/m/Album"));
        assert_eq!(album_folder(Path::new("/m/Album/disk-03")), Path::new("/m/Album"));
        assert_eq!(album_folder(Path::new("/m/Album/Discography")), Path::new("/m/Album/Discography"));
        assert_eq!(album_folder(Path::new("/m/CD")), Path::new("/m/CD"));
    }

    #[test]
    fn splits_multi_valued_genres_only() {
        let genres: Vec<_> = split_genres(Some("Ambient; Electronic\0Drone")).collect();
        assert_eq!(genres, ["Ambient", "Electronic", "Drone"]);
        assert_eq!(split_genres(Some("R&B/Soul")).collect::<Vec<_>>(), ["R&B/Soul"]);
        assert_eq!(split_genres(Some(" ; ")).count(), 0);
        assert_eq!(split_genres(None).count(), 0);
    }
}
