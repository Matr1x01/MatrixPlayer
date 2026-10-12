//! Lyrics from LRCLIB for tracks without their own, kept in the database so
//! that each track is looked up once (see [`super::lrclib`] for how).
//!
//! Only tags leave the computer: title, artist, album and length. A track
//! whose tags cannot identify it (no artist, or a title made up from the
//! file name) is not looked up at all.

use std::time::{SystemTime, UNIX_EPOCH};

use super::lrclib::{Found, Lrclib, Query};
use super::lyrics::{self, Lyrics, LyricsSource};
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::library::text::fold;
use crate::player::TrackInfo;

pub const PROVIDER: &str = "LRCLIB";
/// How long a miss is believed before LRCLIB is asked again: lyrics are
/// added to it all the time.
const MISS_KEPT_S: i64 = 7 * 24 * 60 * 60;

/// What to ask LRCLIB about `track`, if its tags identify it.
pub fn query(track: &TrackInfo) -> Option<Query> {
    let md = &track.metadata;
    let artist = md.artist.as_deref().or(md.album_artist.as_deref()).map(str::trim).filter(|a| !a.is_empty())?;
    let title = md.title.trim();
    if title.is_empty() || md.derived_fields.contains(&"title") {
        return None;
    }
    Some(Query {
        title: title.to_owned(),
        artist: artist.to_owned(),
        album: md.album.as_deref().map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned),
        duration_s: (track.properties.duration_ms > 0).then(|| track.properties.duration_ms as f64 / 1000.0),
    })
}

/// The cache's key for `query`: its fields folded, the length in seconds.
fn cache_key(query: &Query) -> String {
    let seconds = query.duration_s.map_or(0, |s| s.round() as u64);
    let album = query.album.as_deref().map(fold).unwrap_or_default();
    format!("{}\u{1f}{}\u{1f}{album}\u{1f}{seconds}", fold(&query.artist), fold(&query.title))
}

/// LRCLIB's lyrics for `track`, from the cache if it was asked before.
/// `Ok(None)` when there are none (or its tags cannot identify it); an
/// error when LRCLIB could not be asked, which is not remembered.
pub async fn find(db: &Database, lrclib: &Lrclib, track: &TrackInfo) -> AppResult<Option<Lyrics>> {
    let Some(query) = query(track) else {
        log::debug!("not looking up lyrics for {}: its tags do not say what it is", track.file_name);
        return Ok(None);
    };
    let key = cache_key(&query);
    let cached: Option<(String, Option<String>, Option<String>, i64)> =
        sqlx::query_as("SELECT status, synced, plain, fetched_at FROM online_lyrics WHERE key = ?")
            .bind(&key)
            .fetch_optional(db.pool())
            .await?;
    match cached {
        Some((status, _, _, at)) if status == "missing" && now() - at >= MISS_KEPT_S => {}
        Some((status, synced, plain, _)) => return Ok(to_lyrics(&from_row(&status, synced, plain))),
        None => {}
    }

    let found = lrclib.lookup(&query).await.map_err(|e| {
        log::warn!("could not look up lyrics for {}: {e}", track.file_name);
        AppError::Online(e.to_string())
    })?;
    let (status, synced, plain) = match &found {
        Found::Lyrics { synced, plain } => ("found", synced.clone(), plain.clone()),
        Found::Instrumental => ("instrumental", None, None),
        Found::Missing => ("missing", None, None),
    };
    log::info!(
        "lyrics for {} on {PROVIDER}: {}",
        track.file_name,
        match &found {
            Found::Lyrics { synced: Some(_), .. } => "synced",
            Found::Lyrics { .. } => "plain",
            Found::Instrumental => "instrumental",
            Found::Missing => "none",
        }
    );
    sqlx::query(
        "INSERT INTO online_lyrics (key, status, synced, plain, fetched_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET status = excluded.status, synced = excluded.synced,
             plain = excluded.plain, fetched_at = excluded.fetched_at",
    )
    .bind(&key)
    .bind(status)
    .bind(synced)
    .bind(plain)
    .bind(now())
    .execute(db.pool())
    .await?;
    Ok(to_lyrics(&found))
}

/// Forgets every lyric downloaded (and every miss). Returns how many.
pub async fn forget(db: &Database) -> AppResult<u64> {
    Ok(sqlx::query("DELETE FROM online_lyrics").execute(db.pool()).await?.rows_affected())
}

fn from_row(status: &str, synced: Option<String>, plain: Option<String>) -> Found {
    match status {
        "found" => Found::Lyrics { synced, plain },
        "instrumental" => Found::Instrumental,
        _ => Found::Missing,
    }
}

/// Synced lyrics when there are any that parse, else the plain text.
fn to_lyrics(found: &Found) -> Option<Lyrics> {
    let mut lyrics = match found {
        Found::Lyrics { synced, plain } => synced
            .as_deref()
            .and_then(|text| lyrics::parse(text, LyricsSource::Online))
            .filter(|l| l.synced)
            .or_else(|| plain.as_deref().and_then(|text| lyrics::parse(text, LyricsSource::Online)))?,
        Found::Instrumental => Lyrics::instrumental(LyricsSource::Online),
        Found::Missing => return None,
    };
    lyrics.provider = Some(PROVIDER.to_owned());
    Some(lyrics)
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::artwork::ArtworkStore;
    use crate::test_support::{HttpMock, write_flac};

    const JSON: &str = "application/json";

    /// A tagged one-second track ("Isobar" by "Test Artist").
    fn track(dir: &tempfile::TempDir) -> TrackInfo {
        gstreamer::init().unwrap();
        let path = dir.path().join("a.flac");
        write_flac(&path, 1.0, "Isobar");
        TrackInfo::load(&path, &ArtworkStore::new(dir.path().join("art")).unwrap())
    }

    fn record(synced: Option<&str>, plain: Option<&str>, instrumental: bool) -> String {
        serde_json::json!({
            "id": 7, "trackName": "Isobar", "artistName": "Test Artist", "albumName": "Test Album",
            "duration": 1.0, "instrumental": instrumental, "plainLyrics": plain, "syncedLyrics": synced,
        })
        .to_string()
    }

    #[tokio::test]
    async fn found_lyrics_are_kept_and_follow_the_music() {
        let dir = tempfile::tempdir().unwrap();
        let track = track(&dir);
        let db = Database::in_memory().await.unwrap();
        let synced = "[00:01.00]First line\n[00:03.50]Second line";
        let body = record(Some(synced), Some("First line\nSecond line"), false);
        let server = HttpMock::start(move |_| (200, JSON, body.clone(), vec![])).await;
        let lrclib = Lrclib::new(&server.base).unwrap();

        let lyrics = find(&db, &lrclib, &track).await.unwrap().expect("found");
        assert!(lyrics.synced, "synced lyrics are preferred");
        assert_eq!((lyrics.source, lyrics.provider.as_deref()), (LyricsSource::Online, Some(PROVIDER)));
        let times: Vec<_> = lyrics.lines.iter().map(|l| l.time_ms).collect();
        assert_eq!(times, [Some(1000), Some(3500)]);
        assert_eq!(server.requests()[0].param("track_name"), Some("Isobar"));

        let again = find(&db, &lrclib, &track).await.unwrap();
        assert_eq!(again, Some(lyrics), "the same lyrics, from the cache");
        assert_eq!(server.requests().len(), 1, "asked once");
    }

    #[tokio::test]
    async fn plain_lyrics_and_instrumentals() {
        let dir = tempfile::tempdir().unwrap();
        let track = track(&dir);
        let db = Database::in_memory().await.unwrap();
        let server =
            HttpMock::start(|_| (200, JSON, record(None, Some("Only words\n\nNo times"), false), vec![])).await;
        let lyrics = find(&db, &Lrclib::new(&server.base).unwrap(), &track).await.unwrap().unwrap();
        assert!(!lyrics.synced && !lyrics.instrumental);
        assert_eq!(lyrics.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Only words", "", "No times"]);

        let db = Database::in_memory().await.unwrap();
        let server = HttpMock::start(|_| (200, JSON, record(None, None, true), vec![])).await;
        let lyrics = find(&db, &Lrclib::new(&server.base).unwrap(), &track).await.unwrap().unwrap();
        assert!(lyrics.instrumental && lyrics.lines.is_empty());
    }

    #[tokio::test]
    async fn misses_are_kept_for_a_while_and_failures_not_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let track = track(&dir);
        let db = Database::in_memory().await.unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let server = HttpMock::start(move |r| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                return (503, JSON, r#"{"name":"ServerOverloaded"}"#.into(), vec![("Retry-After", "1".into())]);
            }
            match r.path.as_str() {
                "/api/get" => (404, JSON, r#"{"name":"TrackNotFound"}"#.into(), vec![]),
                _ => (200, JSON, "[]".into(), vec![]),
            }
        })
        .await;

        let busy = find(&db, &Lrclib::new(&server.base).unwrap(), &track).await;
        assert!(matches!(busy, Err(AppError::Online(ref m)) if m.contains("busy")), "{busy:?}");
        let lrclib = Lrclib::new(&server.base).unwrap();
        assert_eq!(find(&db, &lrclib, &track).await.unwrap(), None, "asked again after the failure");
        let asked = server.requests().len();
        assert_eq!(asked, 3, "the failed request, then /api/get and /api/search");
        assert_eq!(find(&db, &lrclib, &track).await.unwrap(), None);
        assert_eq!(server.requests().len(), asked, "a miss is not asked about again at once");

        sqlx::query("UPDATE online_lyrics SET fetched_at = fetched_at - ?")
            .bind(MISS_KEPT_S)
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(find(&db, &lrclib, &track).await.unwrap(), None);
        assert!(server.requests().len() > asked, "an old miss is asked about again");
        assert_eq!(forget(&db).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn tracks_their_tags_do_not_identify_are_not_looked_up() {
        let dir = tempfile::tempdir().unwrap();
        let mut track = track(&dir);
        let db = Database::in_memory().await.unwrap();
        let server = HttpMock::start(|_| (200, JSON, record(Some("[00:01.00]x"), None, false), vec![])).await;
        let lrclib = Lrclib::new(&server.base).unwrap();
        track.metadata.artist = None;
        assert_eq!(find(&db, &lrclib, &track).await.unwrap(), None);
        track.metadata.artist = Some("Test Artist".into());
        track.metadata.derived_fields.push("title");
        assert_eq!(find(&db, &lrclib, &track).await.unwrap(), None);
        assert!(server.requests().is_empty(), "nothing was sent");
    }
}
