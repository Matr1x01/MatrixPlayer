//! Read queries behind the library views.
//!
//! Small collections (albums, artists, genres) are returned whole and sorted
//! or filtered in the UI. Tracks can number in the hundreds of thousands, so
//! a track query returns only ids in order plus totals; the UI then fetches
//! rows for the part of the list it is showing with [`tracks_by_ids`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};

use super::paths::path_from_bytes;
use super::text::{fold, fts_query, like_escape, sort_key};
use crate::artwork::{ArtworkInfo, ArtworkSource};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::metadata::{AudioProperties, ReplayGain, TrackMetadata};
use crate::player::{LibraryRef, TrackInfo};

/// Columns of [`TrackRow`], for `SELECT … FROM tracks t`.
const TRACK_COLUMNS: &str = "t.id, t.title, t.artist, t.artist_id, t.album, t.album_id, t.album_artist, t.genre,
    t.year, t.track_number, t.disc_number,
    CASE WHEN t.codec = 'PCM' THEN t.container ELSE t.codec END AS codec, t.lossless, t.sample_rate, t.bit_depth, t.channels,
    t.bitrate_kbps, t.duration_ms, t.file_size, t.file_name, t.folder_id, t.artwork_id,
    t.favorite_at IS NOT NULL AS favorite, t.play_count, t.last_played_at, t.added_at, t.available,
    instr(t.derived_fields, '\"title\"') > 0 AS title_derived, t.probe_error IS NOT NULL AS unreadable";

/// One track as shown in lists.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub artist_id: Option<i64>,
    pub album: Option<String>,
    pub album_id: Option<i64>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub codec: String,
    pub lossless: bool,
    pub sample_rate: Option<i64>,
    pub bit_depth: Option<i64>,
    pub channels: Option<i64>,
    pub bitrate_kbps: Option<i64>,
    pub duration_ms: i64,
    pub file_size: i64,
    pub file_name: String,
    pub folder_id: i64,
    pub artwork_id: Option<String>,
    pub favorite: bool,
    pub play_count: i64,
    pub last_played_at: Option<i64>,
    pub added_at: i64,
    pub available: bool,
    pub title_derived: bool,
    pub unreadable: bool,
}

const ALBUM_COLUMNS: &str = "a.id, a.title, a.artist_name, a.artist_id, a.year, a.artwork_id, a.track_count,
    a.duration_ms, a.total_size, a.codec, a.lossless, a.sample_rate, a.bit_depth, a.bitrate_kbps, a.added_at,
    a.sort_key, a.title_key, a.title_derived,
    (SELECT COALESCE(SUM(play_count), 0) FROM tracks t WHERE t.album_id = a.id) AS play_count,
    (SELECT MAX(last_played_at) FROM tracks t WHERE t.album_id = a.id) AS last_played_at,
    (SELECT COUNT(*) FROM tracks t WHERE t.album_id = a.id AND t.available = 0) AS unavailable";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRow {
    pub id: i64,
    pub title: String,
    pub artist_name: String,
    pub artist_id: Option<i64>,
    pub year: Option<i64>,
    pub artwork_id: Option<String>,
    pub track_count: i64,
    pub duration_ms: i64,
    pub total_size: i64,
    pub codec: Option<String>,
    pub lossless: bool,
    pub sample_rate: Option<i64>,
    pub bit_depth: Option<i64>,
    pub bitrate_kbps: Option<i64>,
    pub added_at: i64,
    /// Artist sort key, then title sort key: what "Artist, then year" and
    /// "Title" sort on.
    pub sort_key: String,
    pub title_key: String,
    pub title_derived: bool,
    pub play_count: i64,
    pub last_played_at: Option<i64>,
    pub unavailable: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRow {
    pub id: i64,
    pub name: String,
    pub sort_key: String,
    pub album_count: i64,
    pub track_count: i64,
    pub artwork_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct GenreRow {
    pub id: i64,
    pub name: String,
    pub track_count: i64,
    pub album_count: i64,
    pub duration_ms: i64,
    /// Up to four covers for the genre tile, comma-separated in SQL.
    #[serde(skip)]
    pub covers: Option<String>,
    #[sqlx(skip)]
    pub artwork_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackList {
    pub ids: Vec<i64>,
    pub duration_ms: i64,
    pub total_size: i64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrackSort {
    #[default]
    Default,
    Title,
    Artist,
    Album,
    Year,
    Genre,
    Format,
    SampleRate,
    BitDepth,
    Size,
    Duration,
    Added,
    Plays,
    LastPlayed,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TrackScope {
    All,
    Favorites,
    Recent,
    Album { id: i64 },
    Artist { id: i64 },
    Composer { id: i64 },
    Genre { id: i64 },
    Folder { id: i64, recursive: bool },
    Playlist { id: i64 },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackQuery {
    pub scope: TrackScope,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub sort: TrackSort,
    #[serde(default)]
    pub descending: bool,
    #[serde(default)]
    pub hi_res_only: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub tracks: i64,
    pub albums: i64,
    pub album_artists: i64,
    pub genres: i64,
    pub playlists: i64,
    pub favorites: i64,
    pub total_size: i64,
    pub duration_ms: i64,
    pub unavailable: i64,
    pub roots: i64,
    pub hi_res_albums: i64,
    pub lossless_albums: i64,
}

pub async fn stats(db: &Database) -> AppResult<LibraryStats> {
    let (tracks, total_size, duration_ms, unavailable, favorites): (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(file_size), 0), COALESCE(SUM(duration_ms), 0),
                COALESCE(SUM(available = 0), 0), COALESCE(SUM(favorite_at IS NOT NULL), 0)
         FROM tracks",
    )
    .fetch_one(db.pool())
    .await?;
    let (albums, hi_res_albums, lossless_albums): (i64, i64, i64) = sqlx::query_as(&format!(
        "SELECT COUNT(*), COALESCE(SUM({HI_RES_ALBUM}), 0), COALESCE(SUM(lossless), 0) FROM albums"
    ))
    .fetch_one(db.pool())
    .await?;
    let album_artists: i64 =
        sqlx::query_scalar("SELECT COUNT(DISTINCT artist_id) FROM albums WHERE artist_id IS NOT NULL")
            .fetch_one(db.pool())
            .await?;
    let genres: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM genres").fetch_one(db.pool()).await?;
    let playlists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM playlists").fetch_one(db.pool()).await?;
    let roots: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM library_roots").fetch_one(db.pool()).await?;
    Ok(LibraryStats {
        tracks,
        albums,
        album_artists,
        genres,
        playlists,
        favorites,
        total_size,
        duration_ms,
        unavailable,
        roots,
        hi_res_albums,
        lossless_albums,
    })
}

/// "Hi-Res" in the design: lossless above CD quality.
const HI_RES_ALBUM: &str = "(lossless AND (COALESCE(bit_depth, 0) > 16 OR COALESCE(sample_rate, 0) > 48000))";
const HI_RES_TRACK: &str = "(t.lossless AND (COALESCE(t.bit_depth, 0) > 16 OR COALESCE(t.sample_rate, 0) > 48000))";

// ── albums ─────────────────────────────────────────────────────────────────

pub async fn albums(db: &Database) -> AppResult<Vec<AlbumRow>> {
    Ok(sqlx::query_as(&format!("SELECT {ALBUM_COLUMNS} FROM albums a ORDER BY a.sort_key, a.year, a.title_key"))
        .fetch_all(db.pool())
        .await?)
}

async fn albums_where(db: &Database, condition: &str, id: i64) -> AppResult<Vec<AlbumRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {ALBUM_COLUMNS} FROM albums a WHERE {condition} ORDER BY a.year DESC NULLS LAST, a.title_key"
    ))
    .bind(id)
    .fetch_all(db.pool())
    .await?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDetail {
    pub album: AlbumRow,
    pub artwork: Option<ArtworkInfo>,
    pub genres: Vec<String>,
    pub tracks: Vec<TrackRow>,
    pub disc_count: i64,
    pub channels: Option<i64>,
    pub replay_gain: ReplayGain,
    pub more_by_artist: Vec<AlbumRow>,
    pub folder_id: Option<i64>,
    pub folder_path: Option<String>,
}

pub async fn album(db: &Database, id: i64) -> AppResult<AlbumDetail> {
    let album: AlbumRow = sqlx::query_as(&format!("SELECT {ALBUM_COLUMNS} FROM albums a WHERE a.id = ?"))
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| AppError::NotFound("album".into()))?;
    let tracks: Vec<TrackRow> = sqlx::query_as(&format!(
        "SELECT {TRACK_COLUMNS} FROM tracks t WHERE t.album_id = ?
         ORDER BY t.disc_number NULLS FIRST, t.track_number NULLS LAST, t.file_name"
    ))
    .bind(id)
    .fetch_all(db.pool())
    .await?;
    let genres = genres_of(db, "t.album_id = ?1", id).await?;
    let artwork = match &album.artwork_id {
        Some(art) => artwork_info(db, art).await?,
        None => None,
    };
    let (gain, peak, channels, discs): (Option<f64>, Option<f64>, Option<i64>, i64) = sqlx::query_as(
        "SELECT MAX(rg_album_gain), MAX(rg_album_peak), MAX(channels), COUNT(DISTINCT COALESCE(disc_number, 1))
         FROM tracks WHERE album_id = ?",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await?;
    let more_by_artist = match album.artist_id {
        Some(artist) => {
            sqlx::query_as(&format!(
                "SELECT {ALBUM_COLUMNS} FROM albums a WHERE a.artist_id = ? AND a.id != ?
             ORDER BY a.year DESC NULLS LAST, a.title_key LIMIT 8"
            ))
            .bind(artist)
            .bind(id)
            .fetch_all(db.pool())
            .await?
        }
        None => Vec::new(),
    };
    // The folder the album lives in, when all its tracks share one.
    let folder: Option<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT f.id, f.path FROM folders f WHERE f.id = (SELECT folder_id FROM tracks WHERE album_id = ?1
         GROUP BY folder_id ORDER BY COUNT(*) DESC LIMIT 1)",
    )
    .bind(id)
    .fetch_optional(db.pool())
    .await?;
    Ok(AlbumDetail {
        album,
        artwork,
        genres,
        tracks,
        disc_count: discs,
        channels,
        replay_gain: ReplayGain {
            track_gain: None,
            track_peak: None,
            album_gain: gain.map(|g| g as f32),
            album_peak: peak.map(|p| p as f32),
        },
        more_by_artist,
        folder_id: folder.as_ref().map(|f| f.0),
        folder_path: folder.map(|f| path_from_bytes(&f.1).display().to_string()),
    })
}

/// The most common genres of the tracks matching `condition`, which refers to
/// `id` as `?1`.
async fn genres_of(db: &Database, condition: &str, id: i64) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT g.name FROM genres g JOIN track_genres tg ON tg.genre_id = g.id JOIN tracks t ON t.id = tg.track_id
         WHERE {condition} GROUP BY g.id ORDER BY COUNT(*) DESC, g.name LIMIT 4"
    ))
    .bind(id)
    .fetch_all(db.pool())
    .await?)
}

async fn artwork_info(db: &Database, id: &str) -> AppResult<Option<ArtworkInfo>> {
    let row: Option<(String, i64, i64, String, String)> =
        sqlx::query_as("SELECT id, width, height, source, palette FROM artwork WHERE id = ?")
            .bind(id)
            .fetch_optional(db.pool())
            .await?;
    Ok(row.map(|(id, width, height, source, palette)| artwork_from_parts(id, width, height, &source, &palette)))
}

fn artwork_from_parts(id: String, width: i64, height: i64, source: &str, palette: &str) -> ArtworkInfo {
    ArtworkInfo {
        id,
        width: u32::try_from(width).unwrap_or(0),
        height: u32::try_from(height).unwrap_or(0),
        source: if source == "folder" { ArtworkSource::Folder } else { ArtworkSource::Embedded },
        palette: serde_json::from_str(palette).unwrap_or([[40, 40, 44]; 3]),
    }
}

// ── artists ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ArtistKind {
    /// Artists credited on albums (the default list).
    AlbumArtists,
    /// Anyone credited on a track or an album.
    All,
    Composers,
}

pub async fn artists(db: &Database, kind: ArtistKind) -> AppResult<Vec<ArtistRow>> {
    let (condition, tracks) = match kind {
        ArtistKind::AlbumArtists => (
            "EXISTS (SELECT 1 FROM albums al WHERE al.artist_id = ar.id)",
            "SELECT COUNT(*) FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
             WHERE t.artist_id = ar.id OR al.artist_id = ar.id",
        ),
        ArtistKind::All => (
            "EXISTS (SELECT 1 FROM tracks t WHERE t.artist_id = ar.id)
             OR EXISTS (SELECT 1 FROM albums al WHERE al.artist_id = ar.id)",
            "SELECT COUNT(*) FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
             WHERE t.artist_id = ar.id OR al.artist_id = ar.id",
        ),
        ArtistKind::Composers => (
            "EXISTS (SELECT 1 FROM tracks t WHERE t.composer_id = ar.id)",
            "SELECT COUNT(*) FROM tracks t WHERE t.composer_id = ar.id",
        ),
    };
    Ok(sqlx::query_as(&format!(
        "SELECT ar.id, ar.name, ar.sort_key,
            (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id) AS album_count,
            ({tracks}) AS track_count,
            COALESCE(
                (SELECT al.artwork_id FROM albums al WHERE al.artist_id = ar.id AND al.artwork_id IS NOT NULL
                 ORDER BY al.year DESC LIMIT 1),
                (SELECT t.artwork_id FROM tracks t WHERE (t.artist_id = ar.id OR t.composer_id = ar.id)
                 AND t.artwork_id IS NOT NULL LIMIT 1)) AS artwork_id
         FROM artists ar WHERE {condition} ORDER BY ar.sort_key"
    ))
    .fetch_all(db.pool())
    .await?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatShare {
    pub label: String,
    pub hi_res: bool,
    pub lossless: bool,
    pub tracks: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistDetail {
    pub id: i64,
    pub name: String,
    pub artwork: Option<ArtworkInfo>,
    pub genres: Vec<String>,
    pub track_count: i64,
    pub composed_count: i64,
    pub duration_ms: i64,
    pub total_size: i64,
    pub play_count: i64,
    pub first_year: Option<i64>,
    pub last_year: Option<i64>,
    pub first_added: Option<i64>,
    pub last_played: Option<i64>,
    pub albums: Vec<AlbumRow>,
    pub appears_on: Vec<AlbumRow>,
    pub top_tracks: Vec<TrackRow>,
    pub formats: Vec<FormatShare>,
    pub folders: Vec<FolderRef>,
    /// The folder that holds all of the artist's music, when there is one.
    pub home_folder: Option<FolderRef>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FolderRef {
    pub id: i64,
    pub name: String,
    #[serde(skip)]
    pub path: Vec<u8>,
    #[sqlx(skip)]
    pub display_path: String,
    pub track_count: i64,
}

/// Tracks credited to an artist: as track artist, or on their albums.
const BY_ARTIST: &str = "(t.artist_id = ?1 OR t.album_id IN (SELECT id FROM albums WHERE artist_id = ?1))";

pub async fn artist(db: &Database, id: i64) -> AppResult<ArtistDetail> {
    let name: String = sqlx::query_scalar("SELECT name FROM artists WHERE id = ?")
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| AppError::NotFound("artist".into()))?;
    type Totals = (i64, i64, i64, i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>);
    let (track_count, duration_ms, total_size, play_count, first_year, last_year, first_added, last_played): Totals =
        sqlx::query_as(&format!(
            "SELECT COUNT(*), COALESCE(SUM(duration_ms), 0), COALESCE(SUM(file_size), 0),
                COALESCE(SUM(play_count), 0), MIN(year), MAX(year), MIN(added_at), MAX(last_played_at)
             FROM tracks t WHERE {BY_ARTIST}"
        ))
        .bind(id)
        .fetch_one(db.pool())
        .await?;
    let composed_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE composer_id = ?").bind(id).fetch_one(db.pool()).await?;
    let albums = albums_where(db, "a.artist_id = ?1", id).await?;
    let appears_on = albums_where(
        db,
        "(a.artist_id IS NULL OR a.artist_id != ?1) AND a.id IN (SELECT album_id FROM tracks WHERE artist_id = ?1)",
        id,
    )
    .await?;
    let top_tracks = sqlx::query_as(&format!(
        "SELECT {TRACK_COLUMNS} FROM tracks t WHERE {BY_ARTIST}
         ORDER BY t.play_count DESC, t.last_played_at DESC NULLS LAST, t.year DESC, t.disc_number, t.track_number
         LIMIT 5"
    ))
    .bind(id)
    .fetch_all(db.pool())
    .await?;
    let genres = genres_of(db, BY_ARTIST, id).await?;
    let formats = format_shares(db, &format!("WHERE {BY_ARTIST}"), id).await?;
    let artwork = match albums.iter().chain(&appears_on).find_map(|a| a.artwork_id.clone()) {
        Some(art) => artwork_info(db, &art).await?,
        None => None,
    };
    let mut folders: Vec<FolderRef> = sqlx::query_as(&format!(
        "SELECT f.id, f.name, f.path, COUNT(*) AS track_count FROM tracks t JOIN folders f ON f.id = t.folder_id
         WHERE {BY_ARTIST} GROUP BY f.id ORDER BY f.path"
    ))
    .bind(id)
    .fetch_all(db.pool())
    .await?;
    for folder in &mut folders {
        folder.display_path = path_from_bytes(&folder.path).display().to_string();
    }
    let home_folder = common_folder(db, &folders).await?;
    Ok(ArtistDetail {
        id,
        name,
        artwork,
        genres,
        track_count,
        composed_count,
        duration_ms,
        total_size,
        play_count,
        first_year,
        last_year,
        first_added,
        last_played,
        albums,
        appears_on,
        top_tracks,
        formats,
        folders,
        home_folder,
    })
}

/// The deepest indexed folder containing every folder in `folders`.
async fn common_folder(db: &Database, folders: &[FolderRef]) -> AppResult<Option<FolderRef>> {
    let mut paths = folders.iter().map(|f| path_from_bytes(&f.path));
    let Some(mut common) = paths.next() else { return Ok(None) };
    for path in paths {
        while !path.starts_with(&common) {
            if !common.pop() {
                return Ok(None);
            }
        }
    }
    loop {
        let found: Option<FolderRef> =
            sqlx::query_as("SELECT f.id, f.name, f.path, 0 AS track_count FROM folders f WHERE f.path = ?")
                .bind(super::paths::path_bytes(&common))
                .fetch_optional(db.pool())
                .await?;
        if let Some(mut folder) = found {
            folder.display_path = common.display().to_string();
            folder.track_count = folders.iter().map(|f| f.track_count).sum();
            return Ok(Some(folder));
        }
        if !common.pop() {
            return Ok(None);
        }
    }
}

async fn format_shares(db: &Database, where_clause: &str, id: i64) -> AppResult<Vec<FormatShare>> {
    type Share = (String, bool, Option<i64>, Option<i64>, i64);
    let rows: Vec<Share> = sqlx::query_as(&format!(
        "SELECT CASE WHEN t.codec = 'PCM' THEN t.container ELSE t.codec END, t.lossless,
            CASE WHEN t.lossless THEN t.bit_depth END AS depth,
            CASE WHEN t.lossless THEN NULL ELSE (t.bitrate_kbps + 16) / 32 * 32 END AS rate, COUNT(*) AS n
         FROM tracks t {where_clause} GROUP BY 1, 2, 3, 4 ORDER BY n DESC"
    ))
    .bind(id)
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|(codec, lossless, depth, rate, tracks)| {
            let label = match (lossless, depth, rate) {
                (true, Some(bits), _) => format!("{codec} {bits}-bit"),
                (false, _, Some(kbps)) => format!("{codec} {kbps}"),
                _ => codec,
            };
            FormatShare { label, hi_res: depth.is_some_and(|d| d > 16), lossless, tracks }
        })
        .collect())
}

// ── genres ─────────────────────────────────────────────────────────────────

pub async fn genres(db: &Database) -> AppResult<Vec<GenreRow>> {
    let mut rows: Vec<GenreRow> = sqlx::query_as(
        "SELECT g.id, g.name,
            COUNT(t.id) AS track_count,
            COUNT(DISTINCT t.album_id) AS album_count,
            COALESCE(SUM(t.duration_ms), 0) AS duration_ms,
            (SELECT group_concat(artwork_id) FROM (
                SELECT a.artwork_id FROM albums a WHERE a.artwork_id IS NOT NULL AND a.id IN (
                    SELECT t2.album_id FROM tracks t2 JOIN track_genres tg2 ON tg2.track_id = t2.id
                    WHERE tg2.genre_id = g.id)
                ORDER BY a.track_count DESC LIMIT 4)) AS covers
         FROM genres g JOIN track_genres tg ON tg.genre_id = g.id JOIN tracks t ON t.id = tg.track_id
         GROUP BY g.id ORDER BY g.name_key",
    )
    .fetch_all(db.pool())
    .await?;
    for row in &mut rows {
        row.artwork_ids = row.covers.take().map(|c| c.split(',').map(str::to_owned).collect()).unwrap_or_default();
    }
    Ok(rows)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreDetail {
    pub genre: GenreRow,
    pub albums: Vec<AlbumRow>,
    pub artists: Vec<ArtistRow>,
}

pub async fn genre(db: &Database, id: i64) -> AppResult<GenreDetail> {
    let genre = genres(db).await?.into_iter().find(|g| g.id == id).ok_or_else(|| AppError::NotFound("genre".into()))?;
    let albums = sqlx::query_as(&format!(
        "SELECT {ALBUM_COLUMNS} FROM albums a WHERE a.id IN (
            SELECT t.album_id FROM tracks t JOIN track_genres tg ON tg.track_id = t.id WHERE tg.genre_id = ?)
         ORDER BY a.sort_key, a.year"
    ))
    .bind(id)
    .fetch_all(db.pool())
    .await?;
    let artists = sqlx::query_as(
        "SELECT ar.id, ar.name, ar.sort_key, 0 AS album_count, COUNT(t.id) AS track_count, MAX(t.artwork_id) AS artwork_id
         FROM artists ar JOIN tracks t ON t.artist_id = ar.id JOIN track_genres tg ON tg.track_id = t.id
         WHERE tg.genre_id = ? GROUP BY ar.id ORDER BY track_count DESC LIMIT 12",
    )
    .bind(id)
    .fetch_all(db.pool())
    .await?;
    Ok(GenreDetail { genre, albums, artists })
}

// ── folders ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderNode {
    pub id: i64,
    pub name: String,
    pub has_children: bool,
    /// Tracks in this folder and everything below it.
    pub track_count: i64,
    pub total_size: i64,
    pub artwork_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootInfo {
    pub id: i64,
    pub display: String,
    pub folder_id: Option<i64>,
    pub online: bool,
    pub track_count: i64,
    pub total_size: i64,
    pub last_scan_at: Option<i64>,
}

pub async fn roots(db: &Database) -> AppResult<Vec<RootInfo>> {
    type Row = (i64, Vec<u8>, String, Option<i64>, i64, i64, Option<i64>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT r.id, r.path, r.display,
            (SELECT f.id FROM folders f WHERE f.path = r.path),
            (SELECT COUNT(*) FROM tracks t WHERE t.root_id = r.id),
            (SELECT COALESCE(SUM(file_size), 0) FROM tracks t WHERE t.root_id = r.id),
            r.last_scan_at
         FROM library_roots r ORDER BY r.display",
    )
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, path, display, folder_id, track_count, total_size, last_scan_at)| RootInfo {
            id,
            online: path_from_bytes(&path).is_dir(),
            display,
            folder_id,
            track_count,
            total_size,
            last_scan_at,
        })
        .collect())
}

/// Child folders of `parent`, with recursive track counts.
pub async fn folder_children(db: &Database, parent: i64) -> AppResult<Vec<FolderNode>> {
    let rows: Vec<(i64, String, i64, i64, i64, Option<String>)> = sqlx::query_as(
        "WITH RECURSIVE sub(top, id) AS (
            SELECT id, id FROM folders WHERE parent_id = ?1
            UNION ALL SELECT sub.top, f.id FROM folders f JOIN sub ON f.parent_id = sub.id
         )
         SELECT f.id, f.name,
            EXISTS (SELECT 1 FROM folders c WHERE c.parent_id = f.id),
            (SELECT COUNT(*) FROM sub JOIN tracks t ON t.folder_id = sub.id WHERE sub.top = f.id),
            (SELECT COALESCE(SUM(t.file_size), 0) FROM sub JOIN tracks t ON t.folder_id = sub.id WHERE sub.top = f.id),
            (SELECT t.artwork_id FROM tracks t WHERE t.folder_id = f.id AND t.artwork_id IS NOT NULL LIMIT 1)
         FROM folders f WHERE f.parent_id = ?1 ORDER BY f.name_key",
    )
    .bind(parent)
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, has_children, track_count, total_size, artwork_id)| FolderNode {
            id,
            name,
            has_children: has_children != 0,
            track_count,
            total_size,
            artwork_id,
        })
        .collect())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherFile {
    pub name: String,
    /// "image", "cue", "log", "playlist", "text" or "file".
    pub kind: &'static str,
    pub size: u64,
    pub modified: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderDetail {
    pub id: i64,
    pub name: String,
    pub display_path: String,
    pub root_id: i64,
    pub root_online: bool,
    /// From the root down to this folder's parent.
    pub crumbs: Vec<Crumb>,
    pub parent_id: Option<i64>,
    pub subfolders: Vec<FolderNode>,
    pub tracks: Vec<TrackRow>,
    pub other_files: Vec<OtherFile>,
    pub artwork: Option<ArtworkInfo>,
    /// The album most of this folder's tracks belong to.
    pub album_id: Option<i64>,
    pub album_count: i64,
    pub last_scan_at: Option<i64>,
    /// Tracks with a title, artist and album tag, out of all tracks here.
    pub tagged: i64,
}

pub async fn folder(db: &Database, id: i64) -> AppResult<FolderDetail> {
    let (name, path, root_id, parent_id): (String, Vec<u8>, i64, Option<i64>) =
        sqlx::query_as("SELECT name, path, root_id, parent_id FROM folders WHERE id = ?")
            .bind(id)
            .fetch_optional(db.pool())
            .await?
            .ok_or_else(|| AppError::NotFound("folder".into()))?;
    let path = path_from_bytes(&path);
    let crumbs: Vec<(i64, String)> = sqlx::query_as(
        "WITH RECURSIVE up(id, parent_id, name, depth) AS (
            SELECT id, parent_id, name, 0 FROM folders WHERE id = (SELECT parent_id FROM folders WHERE id = ?)
            UNION ALL SELECT f.id, f.parent_id, f.name, up.depth + 1 FROM folders f JOIN up ON f.id = up.parent_id
         )
         SELECT id, name FROM up ORDER BY depth DESC",
    )
    .bind(id)
    .fetch_all(db.pool())
    .await?;
    let tracks: Vec<TrackRow> =
        sqlx::query_as(&format!("SELECT {TRACK_COLUMNS} FROM tracks t WHERE t.folder_id = ? ORDER BY t.file_name"))
            .bind(id)
            .fetch_all(db.pool())
            .await?;
    let subfolders = folder_children(db, id).await?;
    let (album_id, album_count, tagged): (Option<i64>, i64, i64) = sqlx::query_as(
        "SELECT (SELECT album_id FROM tracks WHERE folder_id = ?1 GROUP BY album_id ORDER BY COUNT(*) DESC LIMIT 1),
            (SELECT COUNT(DISTINCT album_id) FROM tracks WHERE folder_id = ?1),
            (SELECT COUNT(*) FROM tracks WHERE folder_id = ?1 AND artist IS NOT NULL AND album IS NOT NULL
                AND instr(derived_fields, '\"title\"') = 0)",
    )
    .bind(id)
    .fetch_one(db.pool())
    .await?;
    let art = tracks.iter().find_map(|t| t.artwork_id.clone());
    let artwork = match art {
        Some(art) => artwork_info(db, &art).await?,
        None => None,
    };
    let last_scan_at: Option<i64> = sqlx::query_scalar("SELECT last_scan_at FROM library_roots WHERE id = ?")
        .bind(root_id)
        .fetch_one(db.pool())
        .await?;
    let listing_path = path.clone();
    let other_files = tokio::task::spawn_blocking(move || list_other_files(&listing_path))
        .await
        .map_err(|e| AppError::Io(std::io::Error::other(e)))?;
    let root_path: Vec<u8> =
        sqlx::query_scalar("SELECT path FROM library_roots WHERE id = ?").bind(root_id).fetch_one(db.pool()).await?;
    Ok(FolderDetail {
        id,
        name,
        display_path: path.display().to_string(),
        root_id,
        root_online: path_from_bytes(&root_path).is_dir(),
        crumbs: crumbs.into_iter().map(|(id, name)| Crumb { id, name }).collect(),
        parent_id,
        subfolders,
        tracks,
        other_files,
        artwork,
        album_id,
        album_count,
        last_scan_at,
        tagged,
    })
}

/// Files in `dir` that are not indexed audio: covers, cue sheets, rip logs.
fn list_other_files(dir: &std::path::Path) -> Vec<OtherFile> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<OtherFile> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let meta = entry.metadata().ok()?;
            if !meta.is_file() || crate::filesystem::has_audio_extension(&path) {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
            let kind = match ext.as_str() {
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" => "image",
                "cue" => "cue",
                "log" => "log",
                "m3u" | "m3u8" | "pls" => "playlist",
                "txt" | "nfo" | "md" => "text",
                _ => "file",
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            Some(OtherFile { name, kind, size: meta.len(), modified })
        })
        .collect();
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files
}

// ── tracks ─────────────────────────────────────────────────────────────────

/// Search qualifiers understood in filter and search text, e.g.
/// `format:flac rate:>48k bits:24 year:2010-2019`.
#[derive(Debug, Default, PartialEq)]
pub struct Filters {
    pub text: String,
    pub format: Option<String>,
    pub rate: Option<(Cmp, i64)>,
    pub bits: Option<(Cmp, i64)>,
    pub year: Option<(i64, i64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

impl Cmp {
    fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Lt => "<",
            Self::Le => "<=",
        }
    }
}

pub fn parse_filters(input: &str) -> Filters {
    let mut filters = Filters::default();
    let mut words = Vec::new();
    for word in input.split_whitespace() {
        let Some((key, value)) = word.split_once(':') else {
            words.push(word);
            continue;
        };
        let handled = match key.to_ascii_lowercase().as_str() {
            "format" | "codec" if !value.is_empty() => {
                filters.format = Some(value.to_ascii_lowercase());
                true
            }
            "rate" => parse_cmp(value, true).map(|c| filters.rate = Some(c)).is_some(),
            "bits" | "depth" => parse_cmp(value, false).map(|c| filters.bits = Some(c)).is_some(),
            "year" => parse_years(value).map(|y| filters.year = Some(y)).is_some(),
            _ => false,
        };
        if !handled {
            words.push(word);
        }
    }
    filters.text = words.join(" ");
    filters
}

fn parse_cmp(value: &str, rate: bool) -> Option<(Cmp, i64)> {
    let (cmp, rest) = [(">=", Cmp::Ge), ("<=", Cmp::Le), (">", Cmp::Gt), ("<", Cmp::Lt), ("=", Cmp::Eq)]
        .iter()
        .find_map(|(p, c)| value.strip_prefix(p).map(|r| (*c, r)))
        .unwrap_or((Cmp::Eq, value));
    let lower = rest.to_ascii_lowercase();
    let (number, scale) = match lower.strip_suffix('k') {
        Some(n) if rate => (n, 1000.0),
        _ => (lower.strip_suffix("hz").unwrap_or(&lower), 1.0),
    };
    let n: f64 = number.parse().ok()?;
    // "44.1" or "96" without a unit are kHz; bare Hz values are large.
    let n = if rate && scale == 1.0 && n < 1000.0 { n * 1000.0 } else { n * scale };
    Some((cmp, n.round() as i64))
}

fn parse_years(value: &str) -> Option<(i64, i64)> {
    match value.split_once(['-', '–']) {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => {
            // "199x" or "1990s" mean the decade.
            let decade = value.strip_suffix('x').or_else(|| value.strip_suffix("0s").map(|_| &value[..3]));
            match decade {
                Some(prefix) if prefix.len() == 3 => {
                    let start: i64 = format!("{prefix}0").parse().ok()?;
                    Some((start, start + 9))
                }
                _ => {
                    let y: i64 = value.parse().ok()?;
                    Some((y, y))
                }
            }
        }
    }
}

fn push_filters(q: &mut QueryBuilder<'_, Sqlite>, filters: &Filters) {
    if let Some(fts) = fts_query(&filters.text) {
        q.push(" AND t.id IN (SELECT rowid FROM track_search WHERE track_search MATCH ").push_bind(fts).push(")");
    }
    if let Some(format) = &filters.format {
        match format.as_str() {
            "lossless" => {
                q.push(" AND t.lossless");
            }
            "lossy" => {
                q.push(" AND NOT t.lossless");
            }
            "hires" | "hi-res" => {
                q.push(format!(" AND {HI_RES_TRACK}"));
            }
            "dsd" => {
                q.push(" AND lower(t.codec) LIKE 'dsd%'");
            }
            other => {
                q.push(" AND (lower(t.codec) = ").push_bind(other.to_owned());
                q.push(" OR lower(t.container) = ").push_bind(other.to_owned()).push(")");
            }
        }
    }
    if let Some((cmp, rate)) = filters.rate {
        q.push(format!(" AND t.sample_rate {} ", cmp.sql())).push_bind(rate);
    }
    if let Some((cmp, bits)) = filters.bits {
        q.push(format!(" AND t.bit_depth {} ", cmp.sql())).push_bind(bits);
    }
    if let Some((from, to)) = filters.year {
        q.push(" AND t.year BETWEEN ").push_bind(from).push(" AND ").push_bind(to);
    }
}

/// Ids (in display order) and totals of the tracks matching `query`.
pub async fn track_list(db: &Database, query: &TrackQuery) -> AppResult<TrackList> {
    let mut q = QueryBuilder::<Sqlite>::new("SELECT t.id, t.duration_ms, t.file_size FROM tracks t");
    if let TrackScope::Playlist { .. } = query.scope {
        q.push(" JOIN playlist_tracks pt ON pt.track_id = t.id");
    }
    q.push(" WHERE 1");
    match &query.scope {
        TrackScope::All => {}
        TrackScope::Favorites => {
            q.push(" AND t.favorite_at IS NOT NULL");
        }
        TrackScope::Recent => {
            q.push(" AND t.last_played_at IS NOT NULL");
        }
        TrackScope::Album { id } => {
            q.push(" AND t.album_id = ").push_bind(*id);
        }
        TrackScope::Artist { id } => {
            q.push(" AND (t.artist_id = ").push_bind(*id);
            q.push(" OR t.album_id IN (SELECT id FROM albums WHERE artist_id = ").push_bind(*id).push("))");
        }
        TrackScope::Composer { id } => {
            q.push(" AND t.composer_id = ").push_bind(*id);
        }
        TrackScope::Genre { id } => {
            q.push(" AND t.id IN (SELECT track_id FROM track_genres WHERE genre_id = ").push_bind(*id).push(")");
        }
        TrackScope::Folder { id, recursive: false } => {
            q.push(" AND t.folder_id = ").push_bind(*id);
        }
        TrackScope::Folder { id, recursive: true } => {
            q.push(" AND t.folder_id IN (WITH RECURSIVE sub(id) AS (SELECT ")
                .push_bind(*id)
                .push(" UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id) SELECT id FROM sub)");
        }
        TrackScope::Playlist { id } => {
            q.push(" AND pt.playlist_id = ").push_bind(*id);
        }
    }
    push_filters(&mut q, &parse_filters(&query.filter));
    if query.hi_res_only {
        q.push(format!(" AND {HI_RES_TRACK}"));
    }

    let dir = if query.descending { "DESC" } else { "ASC" };
    let album_order = "t.album_key, t.album_id, t.disc_number NULLS FIRST, t.track_number NULLS LAST, t.file_name";
    let order = match (query.sort, &query.scope) {
        (TrackSort::Default, TrackScope::Album { .. }) => {
            "t.disc_number NULLS FIRST, t.track_number NULLS LAST, t.file_name".to_owned()
        }
        (TrackSort::Default, TrackScope::Artist { .. } | TrackScope::Composer { .. }) => {
            format!("t.year NULLS LAST, {album_order}")
        }
        (TrackSort::Default, TrackScope::Folder { .. }) => "t.display_path".to_owned(),
        (TrackSort::Default, TrackScope::Favorites) => "t.favorite_at DESC".to_owned(),
        (TrackSort::Default, TrackScope::Recent) => "t.last_played_at DESC".to_owned(),
        (TrackSort::Default, TrackScope::Playlist { .. }) => "pt.position".to_owned(),
        (TrackSort::Default | TrackSort::Artist, _) => format!("t.artist_key {dir}, {album_order}"),
        (TrackSort::Title, _) => format!("t.title_key {dir}, t.artist_key"),
        (TrackSort::Album, _) => format!("t.album_key {dir}, t.album_id, t.disc_number, t.track_number"),
        (TrackSort::Year, _) => format!("t.year {dir} NULLS LAST, {album_order}"),
        (TrackSort::Genre, _) => format!("t.genre {dir} NULLS LAST, t.artist_key, {album_order}"),
        (TrackSort::Format, _) => format!("t.codec {dir}, t.bit_depth {dir}, t.sample_rate {dir}, t.artist_key"),
        (TrackSort::SampleRate, _) => format!("t.sample_rate {dir} NULLS LAST, t.artist_key, {album_order}"),
        (TrackSort::BitDepth, _) => format!("t.bit_depth {dir} NULLS LAST, t.artist_key, {album_order}"),
        (TrackSort::Size, _) => format!("t.file_size {dir}"),
        (TrackSort::Duration, _) => format!("t.duration_ms {dir}"),
        (TrackSort::Added, _) => format!("t.added_at {dir}, {album_order}"),
        (TrackSort::Plays, _) => format!("t.play_count {dir}, t.last_played_at {dir}"),
        (TrackSort::LastPlayed, _) => format!("t.last_played_at {dir} NULLS LAST"),
    };
    q.push(" ORDER BY ").push(order);

    let rows: Vec<(i64, i64, i64)> = q.build_query_as().fetch_all(db.pool()).await?;
    let (duration_ms, total_size) = rows.iter().fold((0, 0), |(d, s), r| (d + r.1, s + r.2));
    Ok(TrackList { ids: rows.into_iter().map(|r| r.0).collect(), duration_ms, total_size })
}

/// Rows for `ids`, in the same order. Unknown ids are skipped; an id that
/// appears more than once (a playlist entry repeated, a track in the history
/// twice) yields a row each time.
pub async fn tracks_by_ids(db: &Database, ids: &[i64]) -> AppResult<Vec<TrackRow>> {
    let mut by_id: HashMap<i64, TrackRow> = HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let list = chunk.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let rows: Vec<TrackRow> =
            sqlx::query_as(&format!("SELECT {TRACK_COLUMNS} FROM tracks t WHERE t.id IN ({list})"))
                .fetch_all(db.pool())
                .await?;
        by_id.extend(rows.into_iter().map(|r| (r.id, r)));
    }
    Ok(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect())
}

#[derive(sqlx::FromRow)]
struct FullTrack {
    id: i64,
    album_id: Option<i64>,
    artist_id: Option<i64>,
    folder_id: i64,
    path: Vec<u8>,
    display_path: String,
    file_name: String,
    file_size: i64,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    genre: Option<String>,
    composer: Option<String>,
    year: Option<i64>,
    track_number: Option<i64>,
    track_total: Option<i64>,
    disc_number: Option<i64>,
    disc_total: Option<i64>,
    comment: Option<String>,
    copyright: Option<String>,
    tag_format: Option<String>,
    tag_item_count: i64,
    derived_fields: String,
    rg_track_gain: Option<f64>,
    rg_track_peak: Option<f64>,
    rg_album_gain: Option<f64>,
    rg_album_peak: Option<f64>,
    codec: String,
    container: String,
    lossless: bool,
    sample_rate: Option<i64>,
    bit_depth: Option<i64>,
    channels: Option<i64>,
    bitrate_kbps: Option<i64>,
    duration_ms: i64,
    probe_error: Option<String>,
    art_id: Option<String>,
    art_width: Option<i64>,
    art_height: Option<i64>,
    art_source: Option<String>,
    art_palette: Option<String>,
}

/// Field names the metadata layer may list as derived.
const DERIVABLE: &[&str] = &["title", "artist", "album"];

impl FullTrack {
    fn into_info(self) -> TrackInfo {
        let narrow = |v: Option<i64>| v.and_then(|v| u32::try_from(v).ok());
        let derived: Vec<String> = serde_json::from_str(&self.derived_fields).unwrap_or_default();
        TrackInfo {
            library: Some(LibraryRef {
                track_id: self.id,
                album_id: self.album_id,
                artist_id: self.artist_id,
                folder_id: self.folder_id,
            }),
            path: path_from_bytes(&self.path),
            display_path: self.display_path,
            file_name: self.file_name,
            metadata: TrackMetadata {
                title: self.title,
                artist: self.artist,
                album: self.album,
                album_artist: self.album_artist,
                genre: self.genre,
                year: self.year.and_then(|y| u16::try_from(y).ok()),
                track_number: narrow(self.track_number),
                track_total: narrow(self.track_total),
                disc_number: narrow(self.disc_number),
                disc_total: narrow(self.disc_total),
                composer: self.composer,
                comment: self.comment,
                copyright: self.copyright,
                replay_gain: ReplayGain {
                    track_gain: self.rg_track_gain.map(|v| v as f32),
                    track_peak: self.rg_track_peak.map(|v| v as f32),
                    album_gain: self.rg_album_gain.map(|v| v as f32),
                    album_peak: self.rg_album_peak.map(|v| v as f32),
                },
                tag_format: self.tag_format,
                tag_item_count: usize::try_from(self.tag_item_count).unwrap_or(0),
                derived_fields: DERIVABLE.iter().copied().filter(|f| derived.iter().any(|d| d == f)).collect(),
            },
            properties: AudioProperties {
                codec: self.codec,
                container: self.container,
                lossless: self.lossless,
                sample_rate: narrow(self.sample_rate),
                bit_depth: self.bit_depth.and_then(|v| u8::try_from(v).ok()),
                channels: self.channels.and_then(|v| u8::try_from(v).ok()),
                bitrate_kbps: narrow(self.bitrate_kbps),
                duration_ms: u64::try_from(self.duration_ms).unwrap_or(0),
                file_size: u64::try_from(self.file_size).unwrap_or(0),
            },
            artwork: match (self.art_id, self.art_width, self.art_height, self.art_source, self.art_palette) {
                (Some(id), Some(w), Some(h), Some(source), Some(palette)) => {
                    Some(artwork_from_parts(id, w, h, &source, &palette))
                }
                _ => None,
            },
            probe_error: self.probe_error,
        }
    }
}

/// Full player tracks for `ids`, in order, built from the database without
/// reading the files. Tracks on offline drives are left out. Repeated ids
/// are kept, so positions into `ids` still line up with the result.
pub async fn track_infos(db: &Database, ids: &[i64]) -> AppResult<Vec<Arc<TrackInfo>>> {
    Ok(track_infos_aligned(db, ids).await?.into_iter().flatten().collect())
}

/// [`track_infos`], position for position: `None` where a track is unknown
/// or offline, so callers can map an index into `ids` onto the result.
pub async fn track_infos_aligned(db: &Database, ids: &[i64]) -> AppResult<Vec<Option<Arc<TrackInfo>>>> {
    let mut by_id: HashMap<i64, Arc<TrackInfo>> = HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let list = chunk.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let rows: Vec<FullTrack> = sqlx::query_as(&format!(
            "SELECT t.id, t.album_id, t.artist_id, t.folder_id, t.path, t.display_path, t.file_name, t.file_size,
                t.title, t.artist, t.album,
                t.album_artist, t.genre, t.composer, t.year, t.track_number, t.track_total, t.disc_number,
                t.disc_total, t.comment, t.copyright, t.tag_format, t.tag_item_count, t.derived_fields,
                t.rg_track_gain, t.rg_track_peak, t.rg_album_gain, t.rg_album_peak, t.codec, t.container,
                t.lossless, t.sample_rate, t.bit_depth, t.channels, t.bitrate_kbps, t.duration_ms, t.probe_error,
                a.id AS art_id, a.width AS art_width, a.height AS art_height, a.source AS art_source,
                a.palette AS art_palette
             FROM tracks t LEFT JOIN artwork a ON a.id = t.artwork_id
             WHERE t.id IN ({list}) AND t.available"
        ))
        .fetch_all(db.pool())
        .await?;
        by_id.extend(rows.into_iter().map(|r| (r.id, Arc::new(r.into_info()))));
    }
    Ok(ids.iter().map(|id| by_id.get(id).cloned()).collect())
}

/// Library rows for files, keyed by path, so files opened from the file
/// manager still count as library tracks.
pub async fn track_infos_for_paths(db: &Database, paths: &[PathBuf]) -> AppResult<HashMap<PathBuf, Arc<TrackInfo>>> {
    let mut ids = Vec::new();
    for chunk in paths.chunks(200) {
        let mut q = QueryBuilder::<Sqlite>::new("SELECT id FROM tracks WHERE path IN (");
        let mut list = q.separated(", ");
        for path in chunk {
            list.push_bind(super::paths::path_bytes(path).to_vec());
        }
        q.push(")");
        let found: Vec<(i64,)> = q.build_query_as().fetch_all(db.pool()).await?;
        ids.extend(found.into_iter().map(|r| r.0));
    }
    Ok(track_infos(db, &ids).await?.into_iter().map(|t| (t.path.clone(), t)).collect())
}

// ── search ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderHit {
    pub id: i64,
    pub name: String,
    pub display_path: String,
    pub track_count: i64,
    pub total_size: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistRow {
    pub id: i64,
    pub name: String,
    pub pinned: bool,
    pub track_count: i64,
    pub duration_ms: i64,
    pub artwork_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub const PLAYLIST_COLUMNS: &str = "p.id, p.name, p.pinned,
    (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id) AS track_count,
    (SELECT COALESCE(SUM(t.duration_ms), 0) FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
        WHERE pt.playlist_id = p.id) AS duration_ms,
    (SELECT t.artwork_id FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
        WHERE pt.playlist_id = p.id AND t.artwork_id IS NOT NULL ORDER BY pt.position LIMIT 1) AS artwork_id,
    p.created_at, p.updated_at";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TopResult {
    Album { album: AlbumRow },
    Artist { artist: ArtistRow },
    Track { track: TrackRow },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub query: String,
    pub took_ms: f64,
    pub top: Option<TopResult>,
    pub tracks: Vec<TrackRow>,
    pub track_count: i64,
    pub albums: Vec<AlbumRow>,
    pub artists: Vec<ArtistRow>,
    pub genres: Vec<GenreRow>,
    pub folders: Vec<FolderHit>,
    pub playlists: Vec<PlaylistRow>,
}

/// Searches everything at once, the way the search dialog shows it.
pub async fn search(db: &Database, input: &str, limit: usize) -> AppResult<SearchResults> {
    let started = std::time::Instant::now();
    let filters = parse_filters(input);
    let has_filters =
        filters.format.is_some() || filters.rate.is_some() || filters.bits.is_some() || filters.year.is_some();
    let text = fold(&filters.text);
    let empty = SearchResults {
        query: input.to_owned(),
        took_ms: 0.0,
        top: None,
        tracks: Vec::new(),
        track_count: 0,
        albums: Vec::new(),
        artists: Vec::new(),
        genres: Vec::new(),
        folders: Vec::new(),
        playlists: Vec::new(),
    };
    if text.is_empty() && !has_filters {
        return Ok(empty);
    }

    // Tracks: every word must match some field.
    let list = track_list(
        db,
        &TrackQuery {
            scope: TrackScope::All,
            filter: input.to_owned(),
            sort: TrackSort::Default,
            descending: false,
            hi_res_only: false,
        },
    )
    .await?;
    let mut tracks = tracks_by_ids(db, &list.ids[..list.ids.len().min(200)]).await?;
    // Titles that start with the query first, then the rest in library order.
    tracks.sort_by_key(|t| !fold(&t.title).starts_with(&text));
    tracks.truncate(limit);

    let (albums, artists, genres, folders, playlists) = if text.is_empty() {
        Default::default()
    } else {
        let pattern = format!("%{}%", like_escape(&text));
        let word = format!("% {}%", like_escape(&text));
        let starts = format!("{}%", like_escape(&text));
        let key = sort_key(&filters.text);
        let key_starts = format!("{}%", like_escape(&key));
        let albums: Vec<AlbumRow> = sqlx::query_as(&format!(
            "SELECT {ALBUM_COLUMNS} FROM albums a
             WHERE a.title_key LIKE ?1 ESCAPE '\\' OR a.title_key LIKE ?2 ESCAPE '\\'
                OR a.title_key LIKE ?3 ESCAPE '\\' OR a.title_key LIKE ?4 ESCAPE '\\'
             ORDER BY a.title_key LIKE ?3 ESCAPE '\\' OR a.title_key LIKE ?4 ESCAPE '\\' DESC, a.sort_key LIMIT ?5"
        ))
        .bind(&starts)
        .bind(&word)
        .bind(&key_starts)
        .bind(&pattern)
        .bind(limit as i64)
        .fetch_all(db.pool())
        .await?;
        let mut artists: Vec<ArtistRow> = sqlx::query_as(
            "SELECT ar.id, ar.name, ar.sort_key,
                (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id) AS album_count,
                (SELECT COUNT(*) FROM tracks t WHERE t.artist_id = ar.id OR t.composer_id = ar.id) AS track_count,
                (SELECT al.artwork_id FROM albums al WHERE al.artist_id = ar.id AND al.artwork_id IS NOT NULL
                    LIMIT 1) AS artwork_id
             FROM artists ar
             WHERE ar.name_key LIKE ?1 ESCAPE '\\' OR ar.name_key LIKE ?2 ESCAPE '\\' OR ar.sort_key LIKE ?3 ESCAPE '\\'
             ORDER BY ar.name_key LIKE ?1 ESCAPE '\\' DESC, ar.sort_key LIMIT ?4",
        )
        .bind(&starts)
        .bind(&word)
        .bind(&key_starts)
        .bind(limit as i64)
        .fetch_all(db.pool())
        .await?;
        artists.retain(|a| a.track_count > 0 || a.album_count > 0);
        let mut genres = self::genres(db).await?;
        genres.retain(|g| fold(&g.name).contains(&text));
        genres.truncate(limit);
        let folder_rows: Vec<(i64, String, Vec<u8>)> = sqlx::query_as(
            "SELECT f.id, f.name, f.path FROM folders f WHERE f.name_key LIKE ?1 ESCAPE '\\' OR f.name_key LIKE ?2 ESCAPE '\\'
             ORDER BY length(f.path) LIMIT ?3",
        )
        .bind(&pattern)
        .bind(&key_starts)
        .bind(limit as i64)
        .fetch_all(db.pool())
        .await?;
        let mut folders = Vec::with_capacity(folder_rows.len());
        for (id, name, path) in folder_rows {
            let (track_count, total_size): (i64, i64) = sqlx::query_as(
                "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id)
                 SELECT COUNT(*), COALESCE(SUM(file_size), 0) FROM tracks WHERE folder_id IN (SELECT id FROM sub)",
            )
            .bind(id)
            .fetch_one(db.pool())
            .await?;
            folders.push(FolderHit {
                id,
                name,
                display_path: path_from_bytes(&path).display().to_string(),
                track_count,
                total_size,
            });
        }
        let playlists: Vec<PlaylistRow> =
            sqlx::query_as(&format!("SELECT {PLAYLIST_COLUMNS} FROM playlists p ORDER BY p.name"))
                .fetch_all(db.pool())
                .await?;
        let playlists = playlists.into_iter().filter(|p| fold(&p.name).contains(&text)).take(limit).collect();
        (albums, artists, genres, folders, playlists)
    };

    // The top result is whatever matches the query most closely by name.
    let closeness = |name: &str| -> f64 {
        let name = fold(name);
        if name == text {
            2.0
        } else if name.starts_with(&text) {
            1.0 + text.len() as f64 / name.len().max(1) as f64
        } else {
            0.0
        }
    };
    let best_album = albums.first().map(|a| (closeness(&a.title), a));
    let best_artist = artists.first().map(|a| (closeness(&a.name), a));
    let best_track = tracks.first().map(|t| (closeness(&t.title) - 0.01, t));
    let mut top: Option<(f64, TopResult)> = None;
    let mut consider = |score: f64, result: TopResult| {
        if score > 0.0 && top.as_ref().is_none_or(|(s, _)| score > *s) {
            top = Some((score, result));
        }
    };
    if let Some((s, a)) = best_album {
        consider(s + 0.02, TopResult::Album { album: a.clone() });
    }
    if let Some((s, a)) = best_artist {
        consider(s, TopResult::Artist { artist: a.clone() });
    }
    if let Some((s, t)) = best_track {
        consider(s, TopResult::Track { track: t.clone() });
    }
    let top = top.map(|(_, r)| r).or_else(|| tracks.first().map(|t| TopResult::Track { track: t.clone() }));

    Ok(SearchResults {
        query: input.to_owned(),
        took_ms: started.elapsed().as_secs_f64() * 1000.0,
        top,
        track_count: list.ids.len() as i64,
        tracks,
        albums,
        artists,
        genres,
        folders,
        playlists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_qualifiers() {
        let f = parse_filters("rain format:FLAC rate:>48k bits:24 year:2010-2019 studies");
        assert_eq!(f.text, "rain studies");
        assert_eq!(f.format.as_deref(), Some("flac"));
        assert_eq!(f.rate, Some((Cmp::Gt, 48_000)));
        assert_eq!(f.bits, Some((Cmp::Eq, 24)));
        assert_eq!(f.year, Some((2010, 2019)));

        assert_eq!(parse_filters("rate:44.1").rate, Some((Cmp::Eq, 44_100)));
        assert_eq!(parse_filters("rate:>=96000").rate, Some((Cmp::Ge, 96_000)));
        assert_eq!(parse_filters("year:199x").year, Some((1990, 1999)));
        assert_eq!(parse_filters("year:1980s").year, Some((1980, 1989)));
        assert_eq!(parse_filters("note: this").text, "note: this", "unknown qualifiers stay text");
        assert_eq!(parse_filters("rate:fast").text, "rate:fast");
    }
}
