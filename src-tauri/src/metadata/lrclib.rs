//! LRCLIB (<https://lrclib.net>), a free database of synced lyrics.
//!
//! Its API asks clients to:
//! - name themselves, their version and homepage in `User-Agent`;
//! - send one request at a time, with a short pause (200–500 ms) between;
//! - honour `Retry-After` on 429 (rate limited) and 503 (overloaded), and
//!   check the status and content type before reading a body as JSON.
//!
//! [`Lrclib`] does all three: requests wait their turn, and after a 429 or
//! 503 none is sent until the server said to come back.
//!
//! A track is looked up by its tags: `/api/get` with title, artist, album
//! and length (LRCLIB matches within ±2 s), then, for titles tagged
//! differently from LRCLIB's ("… (Remastered 2011)"), `/api/search`, whose
//! results must match the title, an artist and the length.

use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::library::text::fold;

pub const BASE_URL: &str = "https://lrclib.net";
const USER_AGENT: &str =
    concat!("MatrixPlayer v", env!("CARGO_PKG_VERSION"), " (https://github.com/matr1x01/matrixplayer)");
/// Pause between two requests.
const SPACING: Duration = Duration::from_millis(300);
const TIMEOUT: Duration = Duration::from_secs(10);
/// How far a record's length may be from the track's (LRCLIB's own slack).
const LENGTH_SLACK_S: f64 = 2.0;
/// Waits when the server asks for one without saying how long.
const DEFAULT_BUSY: Duration = Duration::from_secs(30);
const DEFAULT_OVERLOADED: Duration = Duration::from_secs(1);

/// What identifies a track to LRCLIB.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_s: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    Lyrics {
        synced: Option<String>,
        plain: Option<String>,
    },
    /// LRCLIB knows the track, and it has no words.
    Instrumental,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LookupError {
    #[error("LRCLIB could not be reached ({0})")]
    Unreachable(String),
    #[error("LRCLIB is busy; try again in {} s", .0.as_secs().max(1))]
    Busy(Duration),
    #[error("LRCLIB could not answer ({0})")]
    Failed(String),
}

/// A lyrics record, as every endpoint returns it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    track_name: String,
    artist_name: String,
    duration: Option<f64>,
    #[serde(default)]
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

pub struct Lrclib {
    base: String,
    client: reqwest::Client,
    /// When the next request may go out. Held while a request is under way,
    /// so they go one at a time.
    next: Mutex<Instant>,
}

impl Lrclib {
    pub fn new(base: &str) -> Result<Self, String> {
        // Rustls on ring (reqwest leaves the choice to the application);
        // an error only means a provider is installed already.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { base: base.trim_end_matches('/').to_owned(), client, next: Mutex::new(Instant::now()) })
    }

    /// Finds lyrics for the track `query` describes.
    pub async fn lookup(&self, query: &Query) -> Result<Found, LookupError> {
        let mut params = vec![("track_name", query.title.clone()), ("artist_name", query.artist.clone())];
        if let Some(album) = &query.album {
            params.push(("album_name", album.clone()));
        }
        if let Some(seconds) = query.duration_s.filter(|s| (1.0..=3600.0).contains(s)) {
            params.push(("duration", format!("{}", seconds.round())));
        }
        if let Some(body) = self.get("/api/get", &params).await? {
            let record: Record = parse(&body)?;
            return Ok(found(&record));
        }
        let artist = artists(&query.artist).into_iter().next().unwrap_or_default();
        let params = [("track_name", plain_title(&query.title)), ("artist_name", artist)];
        let body = self.get("/api/search", &params).await?.unwrap_or_else(|| "[]".into());
        let records: Vec<Record> = parse(&body)?;
        Ok(best(&records, query).map_or(Found::Missing, found))
    }

    /// GETs `path`, waiting its turn. `None` when LRCLIB has no such record.
    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Option<String>, LookupError> {
        let url = url::Url::parse_with_params(&format!("{}{path}", self.base), params)
            .map_err(|e| LookupError::Failed(e.to_string()))?;
        let mut next = self.next.lock().await;
        let now = Instant::now();
        if *next > now + SPACING {
            // Still within a wait the server asked for.
            return Err(LookupError::Busy(*next - now));
        }
        tokio::time::sleep_until((*next).into()).await;
        let sent = self.client.get(url).send().await;
        *next = Instant::now() + SPACING;
        let response = sent.map_err(|e| LookupError::Unreachable(describe(&e)))?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            let fallback =
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS { DEFAULT_BUSY } else { DEFAULT_OVERLOADED };
            let wait = retry_after(response.headers()).unwrap_or(fallback);
            log::warn!("LRCLIB answered {status}; waiting {wait:?} before asking again");
            *next = Instant::now() + wait.max(SPACING);
            return Err(LookupError::Busy(wait));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(LookupError::Failed(status.to_string()));
        }
        let json = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("json"));
        if !json {
            return Err(LookupError::Failed("the answer was not JSON".into()));
        }
        response.text().await.map(Some).map_err(|e| LookupError::Unreachable(describe(&e)))
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, LookupError> {
    serde_json::from_str(body).map_err(|e| LookupError::Failed(format!("unexpected answer: {e}")))
}

fn describe(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "it took too long to answer".into()
    } else if e.is_connect() {
        "no connection".into()
    } else {
        e.to_string()
    }
}

/// `Retry-After` in seconds (the HTTP-date form is not used by LRCLIB).
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let seconds: u64 = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds.min(3600)))
}

fn found(record: &Record) -> Found {
    let text = |s: &Option<String>| s.as_ref().filter(|s| !s.trim().is_empty()).cloned();
    let (synced, plain) = (text(&record.synced_lyrics), text(&record.plain_lyrics));
    match (synced, plain) {
        (None, None) if record.instrumental => Found::Instrumental,
        (None, None) => Found::Missing,
        (synced, plain) => Found::Lyrics { synced, plain },
    }
}

/// The search result that is this very track: the same title (annotations
/// aside), one of its artists, and its length. Synced lyrics first, then the
/// closest length.
fn best<'a>(records: &'a [Record], query: &Query) -> Option<&'a Record> {
    let title = key(&plain_title(&query.title));
    let wanted: Vec<String> = artists(&query.artist).iter().map(|a| key(a)).filter(|a| !a.is_empty()).collect();
    let length_gap = |r: &Record| match (query.duration_s, r.duration) {
        (Some(want), Some(have)) => (want - have).abs(),
        _ => 0.0,
    };
    records
        .iter()
        .filter(|r| key(&plain_title(&r.track_name)) == title)
        .filter(|r| {
            let have = key(&r.artist_name);
            wanted.iter().any(|a| have.contains(a.as_str()) || a.contains(have.as_str()))
        })
        .filter(|r| length_gap(r) <= LENGTH_SLACK_S)
        .filter(|r| found(r) != Found::Missing)
        .min_by(|a, b| {
            let rank = |r: &Record| match found(r) {
                Found::Lyrics { synced: Some(_), .. } => 0,
                Found::Lyrics { .. } => 1,
                _ => 2,
            };
            rank(a).cmp(&rank(b)).then(length_gap(a).total_cmp(&length_gap(b)))
        })
}

/// Words that mark a bracketed or dashed part of a title as an annotation
/// of the recording, not part of the song's name.
const ANNOTATIONS: &str = "remaster live version edit mix mono stereo deluxe explicit feat ft. bonus demo single acoustic anniversary original";

/// `title` without its annotations: "Master of Puppets (Remastered)" and
/// "Hey Jude - Remastered 2015" are found under their song's name.
pub(crate) fn plain_title(title: &str) -> String {
    let annotation = |s: &str| {
        let s = s.to_lowercase();
        ANNOTATIONS.split(' ').any(|a| s.contains(a))
    };
    let mut title = title.trim();
    loop {
        let before = title;
        if let Some(close) = title.chars().last().filter(|c| matches!(c, ')' | ']')) {
            let open = if close == ')' { '(' } else { '[' };
            if let Some(start) = title.rfind(open)
                && start > 0
                && annotation(&title[start..])
            {
                title = title[..start].trim_end();
            }
        }
        if let Some(dash) = title.rfind(" - ")
            && dash > 0
            && annotation(&title[dash..])
        {
            title = title[..dash].trim_end();
        }
        if title == before {
            return title.to_owned();
        }
    }
}

/// The artists in an artist tag: "A/ B", "A; B", "A & B", "A feat. B".
fn artists(tag: &str) -> Vec<String> {
    let mut tag = tag.to_owned();
    for joiner in [" feat. ", " feat ", " ft. ", " featuring ", " & ", ";", ",", "/"] {
        tag = tag.replace(joiner, "\u{1f}");
    }
    tag.split('\u{1f}').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// Letters and digits only, folded: how names are compared.
fn key(s: &str) -> String {
    fold(s).chars().filter(|c| c.is_alphanumeric()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(title: &str, artist: &str, duration: f64, synced: Option<&str>, plain: Option<&str>) -> Record {
        Record {
            track_name: title.into(),
            artist_name: artist.into(),
            duration: Some(duration),
            instrumental: false,
            plain_lyrics: plain.map(Into::into),
            synced_lyrics: synced.map(Into::into),
        }
    }

    fn query(title: &str, artist: &str, duration: f64) -> Query {
        Query { title: title.into(), artist: artist.into(), album: None, duration_s: Some(duration) }
    }

    #[test]
    fn titles_lose_their_recording_annotations_only() {
        assert_eq!(plain_title("Master of Puppets (Remastered)"), "Master of Puppets");
        assert_eq!(plain_title("Hey Jude - Remastered 2015"), "Hey Jude");
        assert_eq!(plain_title("Closer [Explicit] (feat. Halsey)"), "Closer");
        assert_eq!(plain_title("Song 2 - Live at Wembley (2009 Remaster)"), "Song 2");
        assert_eq!(plain_title("(Don't Fear) The Reaper"), "(Don't Fear) The Reaper");
        assert_eq!(plain_title("Hello (Goodbye)"), "Hello (Goodbye)", "a part of the name stays");
        assert_eq!(plain_title("Live"), "Live");
    }

    #[test]
    fn artist_tags_are_split_into_artists() {
        assert_eq!(artists("The Chainsmokers/ Halsey"), ["The Chainsmokers", "Halsey"]);
        assert_eq!(artists("A feat. B & C"), ["A", "B", "C"]);
        assert_eq!(artists("Simon & Garfunkel"), ["Simon", "Garfunkel"]);
    }

    #[test]
    fn the_best_search_result_is_this_track_with_synced_lyrics() {
        let records = [
            record("Closer", "The Chainsmokers", 300.0, Some("[00:01.00]wrong length"), None),
            record("Closer", "Nine Inch Nails", 244.0, Some("[00:01.00]wrong artist"), None),
            record("Closer to the Edge", "The Chainsmokers", 244.0, Some("[00:01.00]another song"), None),
            record("Closer", "The Chainsmokers feat. Halsey", 245.0, None, Some("plain only")),
            record("closer", "The Chainsmokers & Halsey", 243.0, Some("[00:01.00]this one"), Some("this one")),
        ];
        let best = best(&records, &query("Closer (feat. Halsey)", "The Chainsmokers/ Halsey", 244.4)).unwrap();
        assert_eq!(best.synced_lyrics.as_deref(), Some("[00:01.00]this one"));
        assert!(super::best(&records, &query("Closer", "Someone Else", 244.0)).is_none());
    }

    use crate::test_support::{HttpMock, HttpRequest};

    const JSON: &str = "application/json";

    fn closer() -> Query {
        Query {
            title: "Closer (feat. Halsey)".into(),
            artist: "The Chainsmokers/ Halsey".into(),
            album: Some("Collage".into()),
            duration_s: Some(244.4),
        }
    }

    fn record_json(title: &str, artist: &str, duration: f64, synced: &str) -> String {
        serde_json::json!({
            "id": 1, "name": title, "trackName": title, "artistName": artist, "albumName": "Collage",
            "duration": duration, "instrumental": false, "plainLyrics": "plain", "syncedLyrics": synced,
        })
        .to_string()
    }

    #[tokio::test]
    async fn asks_with_the_tracks_signature_and_names_itself() {
        let server =
            HttpMock::start(|_| (200, JSON, record_json("Closer", "The Chainsmokers", 244.0, "[00:01.00]Hey"), vec![]))
                .await;
        let lrclib = Lrclib::new(&server.base).unwrap();
        let found = lrclib.lookup(&closer()).await.unwrap();
        assert_eq!(found, Found::Lyrics { synced: Some("[00:01.00]Hey".into()), plain: Some("plain".into()) });
        let [request] = &server.requests()[..] else { panic!("{:?}", server.requests()) };
        assert_eq!(request.path, "/api/get");
        assert_eq!(request.param("track_name"), Some("Closer (feat. Halsey)"));
        assert_eq!(request.param("artist_name"), Some("The Chainsmokers/ Halsey"));
        assert_eq!(request.param("album_name"), Some("Collage"));
        assert_eq!(request.param("duration"), Some("244"));
        let agent = request.user_agent.as_deref().unwrap();
        assert!(agent.starts_with("MatrixPlayer v") && agent.contains("(https://"), "{agent}");
    }

    #[tokio::test]
    async fn searches_when_the_signature_finds_nothing_and_spaces_requests() {
        let server = HttpMock::start(|r: &HttpRequest| match r.path.as_str() {
            "/api/get" => (404, JSON, r#"{"name":"TrackNotFound","statusCode":404}"#.into(), vec![]),
            _ => {
                let records = [
                    record_json("Closer", "Nine Inch Nails", 373.0, "[00:01.00]wrong"),
                    record_json("Closer", "The Chainsmokers", 245.0, "[00:01.00]right"),
                ];
                (200, JSON, format!("[{}]", records.join(",")), vec![])
            }
        })
        .await;
        let lrclib = Lrclib::new(&server.base).unwrap();
        let found = lrclib.lookup(&closer()).await.unwrap();
        assert!(matches!(found, Found::Lyrics { synced: Some(s), .. } if s.ends_with("right")));
        let requests = server.requests();
        assert_eq!(requests.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/api/get", "/api/search"]);
        assert_eq!(requests[1].param("track_name"), Some("Closer"), "searched without the annotation");
        assert_eq!(requests[1].param("artist_name"), Some("The Chainsmokers"));
        assert!(requests[1].at - requests[0].at >= SPACING - Duration::from_millis(5), "requests were not spaced");
        let none = HttpMock::start(|r: &HttpRequest| match r.path.as_str() {
            "/api/get" => (404, JSON, "{}".into(), vec![]),
            _ => (200, JSON, "[]".into(), vec![]),
        })
        .await;
        assert_eq!(Lrclib::new(&none.base).unwrap().lookup(&closer()).await.unwrap(), Found::Missing);
    }

    #[tokio::test]
    async fn honours_retry_after_and_sends_nothing_meanwhile() {
        let server =
            HttpMock::start(|_| (429, "text/plain", "slow down".into(), vec![("Retry-After", "2".into())])).await;
        let lrclib = Lrclib::new(&server.base).unwrap();
        assert_eq!(lrclib.lookup(&closer()).await, Err(LookupError::Busy(Duration::from_secs(2))));
        assert!(matches!(lrclib.lookup(&closer()).await, Err(LookupError::Busy(_))));
        assert_eq!(server.requests().len(), 1, "no request before the wait is over");
    }

    #[tokio::test]
    async fn reports_answers_it_cannot_use_and_no_connection() {
        let server = HttpMock::start(|_| (200, "text/html", "<html>maintenance</html>".into(), vec![])).await;
        let lrclib = Lrclib::new(&server.base).unwrap();
        assert!(matches!(lrclib.lookup(&closer()).await, Err(LookupError::Failed(_))));
        let server = HttpMock::start(|_| (500, JSON, r#"{"name":"UnknownError"}"#.into(), vec![])).await;
        assert!(matches!(Lrclib::new(&server.base).unwrap().lookup(&closer()).await, Err(LookupError::Failed(_))));
        // Nothing listens on the port of a listener just closed.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let offline = Lrclib::new(&format!("http://127.0.0.1:{port}")).unwrap();
        assert!(matches!(offline.lookup(&closer()).await, Err(LookupError::Unreachable(_))));
    }

    /// Asks the real LRCLIB about a well-known track, tagged the way taggers
    /// often do it. Opt in with `cargo test -- --ignored` (needs internet).
    #[tokio::test]
    #[ignore = "asks lrclib.net"]
    async fn finds_a_well_known_track_on_lrclib() {
        let lrclib = Lrclib::new(BASE_URL).unwrap();
        let found = lrclib.lookup(&Query { album: None, ..closer() }).await.unwrap();
        let Found::Lyrics { synced: Some(synced), .. } = found else { panic!("{found:?}") };
        eprintln!("{}", synced.lines().take(3).collect::<Vec<_>>().join("\n"));
        assert!(synced.starts_with('['), "LRC timestamps");
    }

    #[test]
    fn instrumental_and_empty_records() {
        let mut r = record("Intro", "A", 60.0, None, Some("  "));
        assert_eq!(found(&r), Found::Missing);
        r.instrumental = true;
        assert_eq!(found(&r), Found::Instrumental);
    }
}
