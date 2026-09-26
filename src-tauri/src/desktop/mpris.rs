//! MPRIS 2 on the session bus: desktop media controls, lock screens and
//! media keys (GNOME, KDE and most other desktops route them to MPRIS
//! players).
//!
//! The server mirrors the player's events into its own small state and
//! forwards method calls to the player as ordinary commands, so it has no
//! more power than the UI does.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::sync::broadcast::{self, error::RecvError};
use zbus::fdo;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::artwork::ArtworkStore;
use crate::player::{InsertMode, PlaybackStatus, PlayerCommand, PlayerEvent, PlayerHandle, RepeatMode, TrackInfo};
use crate::state::AppState;

const BUS_NAME: &str = "org.mpris.MediaPlayer2.matrixplayer";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const TRACK_PATH: &str = "/io/github/matr1x01/MatrixPlayer/Track";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
/// Basename of the installed desktop entry (`MatrixPlayer.desktop`).
pub const DESKTOP_ENTRY: &str = "MatrixPlayer";

/// MIME types the player opens, as listed in the desktop entry.
pub const MIME_TYPES: &[&str] = &[
    "audio/flac",
    "audio/x-flac",
    "audio/mpeg",
    "audio/mp4",
    "audio/x-m4a",
    "audio/aac",
    "audio/ogg",
    "audio/x-vorbis+ogg",
    "audio/x-opus+ogg",
    "audio/opus",
    "audio/wav",
    "audio/x-wav",
    "audio/x-aiff",
    "audio/x-ape",
    "audio/x-wavpack",
    "audio/x-musepack",
];

/// What the bus sees, updated from player events.
#[derive(Default)]
struct State {
    status: Option<PlaybackStatus>,
    track: Option<Arc<TrackInfo>>,
    /// Changes on every track change, so clients see a new track id.
    track_serial: u64,
    art: Option<PathBuf>,
    position_ms: u64,
    duration_ms: Option<u64>,
    volume: f64,
    repeat: RepeatMode,
    shuffle: bool,
    queue_len: usize,
    current: Option<usize>,
}

impl State {
    fn track_id(&self) -> OwnedObjectPath {
        let path =
            if self.track.is_some() { format!("{TRACK_PATH}/{}", self.track_serial) } else { NO_TRACK.to_owned() };
        OwnedObjectPath::try_from(path).expect("track paths are valid object paths")
    }

    fn can_next(&self) -> bool {
        self.current.is_some_and(|i| i + 1 < self.queue_len || self.repeat == RepeatMode::All)
    }

    fn can_previous(&self) -> bool {
        self.track.is_some()
    }
}

type Shared = Arc<Mutex<State>>;

fn value<'a>(v: impl Into<Value<'a>>) -> OwnedValue {
    v.into().try_to_owned().expect("plain values convert to owned values")
}

fn to_fdo(err: crate::error::AppError) -> fdo::Error {
    fdo::Error::Failed(err.to_string())
}

fn micros(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX / 1000) * 1000
}

struct Root {
    app: AppHandle,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        super::show_main_window(&self.app);
    }

    fn quit(&self) {
        self.app.exit(0);
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "MatrixPlayer"
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        DESKTOP_ENTRY
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["file".into()]
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        MIME_TYPES.iter().map(|&m| m.to_owned()).collect()
    }
}

struct Player {
    player: PlayerHandle,
    app: AppHandle,
    state: Shared,
}

impl Player {
    async fn command(&self, command: PlayerCommand) -> fdo::Result<()> {
        self.player.command(command).await.map_err(to_fdo)
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    async fn next(&self) -> fdo::Result<()> {
        self.command(PlayerCommand::Next).await
    }

    async fn previous(&self) -> fdo::Result<()> {
        self.command(PlayerCommand::Previous).await
    }

    async fn pause(&self) -> fdo::Result<()> {
        self.command(PlayerCommand::Pause).await
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        self.command(PlayerCommand::TogglePlayPause).await
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.command(PlayerCommand::Stop).await
    }

    async fn play(&self) -> fdo::Result<()> {
        self.command(PlayerCommand::Play).await
    }

    /// Relative seek in microseconds. Past the end means "next track".
    async fn seek(&self, offset: i64) -> fdo::Result<()> {
        let (position, duration) = {
            let s = self.state.lock().unwrap();
            (s.position_ms as i64, s.duration_ms)
        };
        let target = position + offset / 1000;
        if duration.is_some_and(|d| target > d as i64) {
            return self.command(PlayerCommand::Next).await;
        }
        self.command(PlayerCommand::Seek(Duration::from_millis(target.max(0) as u64))).await
    }

    /// Absolute seek, ignored when `track_id` is no longer current.
    async fn set_position(&self, track_id: ObjectPath<'_>, position: i64) -> fdo::Result<()> {
        let (current, duration) = {
            let s = self.state.lock().unwrap();
            (s.track_id(), s.duration_ms)
        };
        if track_id.as_str() != current.as_str() || position < 0 || duration.is_some_and(|d| position / 1000 > d as i64)
        {
            return Ok(());
        }
        self.command(PlayerCommand::Seek(Duration::from_micros(position as u64))).await
    }

    async fn open_uri(&self, uri: String) -> fdo::Result<()> {
        let path = url::Url::parse(&uri)
            .ok()
            .filter(|u| u.scheme() == "file")
            .and_then(|u| u.to_file_path().ok())
            .ok_or_else(|| fdo::Error::InvalidArgs("only file:// URIs can be opened".into()))?;
        let state = self.app.state::<AppState>();
        state.open_paths(vec![path], InsertMode::Replace).await.map(|_| ()).map_err(to_fdo)
    }

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        match self.state.lock().unwrap().status {
            Some(PlaybackStatus::Playing | PlaybackStatus::Loading | PlaybackStatus::Seeking) => "Playing",
            Some(PlaybackStatus::Paused) => "Paused",
            _ => "Stopped",
        }
    }

    #[zbus(property)]
    fn loop_status(&self) -> &str {
        match self.state.lock().unwrap().repeat {
            RepeatMode::Off => "None",
            RepeatMode::One => "Track",
            RepeatMode::All => "Playlist",
        }
    }

    #[zbus(property)]
    async fn set_loop_status(&self, value: String) -> zbus::Result<()> {
        let mode = match value.as_str() {
            "None" => RepeatMode::Off,
            "Track" => RepeatMode::One,
            "Playlist" => RepeatMode::All,
            _ => return Err(fdo::Error::InvalidArgs(format!("unknown loop status {value}")).into()),
        };
        self.player.command(PlayerCommand::SetRepeat(mode)).await.map_err(|e| to_fdo(e).into())
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn set_rate(&self, _rate: f64) {}

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.state.lock().unwrap().shuffle
    }

    #[zbus(property)]
    async fn set_shuffle(&self, shuffle: bool) -> zbus::Result<()> {
        self.player.command(PlayerCommand::SetShuffle(shuffle)).await.map_err(|e| to_fdo(e).into())
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let s = self.state.lock().unwrap();
        let mut m = HashMap::new();
        m.insert("mpris:trackid".to_owned(), value(s.track_id()));
        let Some(track) = &s.track else { return m };
        let md = &track.metadata;
        if let Some(ms) = s.duration_ms {
            m.insert("mpris:length".to_owned(), value(micros(ms)));
        }
        if let Some(art) = s.art.as_ref().and_then(|p| url::Url::from_file_path(p).ok()) {
            m.insert("mpris:artUrl".to_owned(), value(art.to_string()));
        }
        m.insert("xesam:title".to_owned(), value(md.title.clone()));
        if let Some(artist) = &md.artist {
            m.insert("xesam:artist".to_owned(), value(vec![artist.clone()]));
        }
        if let Some(album) = &md.album {
            m.insert("xesam:album".to_owned(), value(album.clone()));
        }
        if let Some(album_artist) = md.album_artist.as_ref().or(md.artist.as_ref()) {
            m.insert("xesam:albumArtist".to_owned(), value(vec![album_artist.clone()]));
        }
        if let Some(genre) = &md.genre {
            m.insert("xesam:genre".to_owned(), value(vec![genre.clone()]));
        }
        if let Some(n) = md.track_number {
            m.insert("xesam:trackNumber".to_owned(), value(i32::try_from(n).unwrap_or(0)));
        }
        if let Some(n) = md.disc_number {
            m.insert("xesam:discNumber".to_owned(), value(i32::try_from(n).unwrap_or(0)));
        }
        if let Some(year) = md.year {
            m.insert("xesam:contentCreated".to_owned(), value(format!("{year:04}-01-01T00:00:00Z")));
        }
        if let Some(composer) = &md.composer {
            m.insert("xesam:composer".to_owned(), value(vec![composer.clone()]));
        }
        if let Ok(uri) = url::Url::from_file_path(&track.path) {
            m.insert("xesam:url".to_owned(), value(uri.to_string()));
        }
        m
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.state.lock().unwrap().volume
    }

    #[zbus(property)]
    async fn set_volume(&self, volume: f64) -> zbus::Result<()> {
        if !volume.is_finite() {
            return Err(fdo::Error::InvalidArgs("volume must be a number".into()).into());
        }
        self.player.command(PlayerCommand::SetVolume(volume.clamp(0.0, 1.0))).await.map_err(|e| to_fdo(e).into())
    }

    /// Read on demand by clients; changes are not signalled (per the spec).
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        micros(self.state.lock().unwrap().position_ms)
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.state.lock().unwrap().can_next()
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.state.lock().unwrap().can_previous()
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.state.lock().unwrap().track.is_some()
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.state.lock().unwrap().track.is_some()
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.track.is_some() && s.duration_ms.is_some_and(|d| d > 0)
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}

/// Registers the MPRIS server and keeps it in step with the player. Runs
/// until the app exits; failing to reach the session bus is not fatal.
pub fn spawn(app: AppHandle, player: PlayerHandle, artwork: Arc<ArtworkStore>) {
    let events = player.subscribe();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = serve(app, player, artwork, events).await {
            log::warn!("MPRIS is not available: {e}");
        }
    });
}

async fn serve(
    app: AppHandle,
    player: PlayerHandle,
    artwork: Arc<ArtworkStore>,
    mut events: broadcast::Receiver<PlayerEvent>,
) -> zbus::Result<()> {
    let state: Shared = Arc::default();
    seed(&state, &player).await;
    let root = Root { app: app.clone() };
    let iface = Player { player: player.clone(), app, state: state.clone() };

    let connection = zbus::connection::Builder::session()?
        .serve_at(OBJECT_PATH, root)?
        .serve_at(OBJECT_PATH, iface)?
        .build()
        .await?;
    // A second instance (or another build) may own the well-known name.
    if connection.request_name(BUS_NAME).await.is_err() {
        connection.request_name(format!("{BUS_NAME}.instance{}", std::process::id())).await?;
    }
    log::info!("MPRIS server registered");

    let player_ref: InterfaceRef<Player> = connection.object_server().interface(OBJECT_PATH).await?;
    let mut seek_done = false;
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(RecvError::Lagged(_)) => {
                seed(&state, &player).await;
                refresh_all(&player_ref).await;
                continue;
            }
            Err(RecvError::Closed) => break,
        };
        let emitter = player_ref.signal_emitter();
        let iface = player_ref.get().await;
        match event {
            PlayerEvent::Status { status, .. } => {
                let was = state.lock().unwrap().status.replace(status);
                seek_done |= was == Some(PlaybackStatus::Seeking) && status != PlaybackStatus::Seeking;
                if was.map(mpris_status) != Some(mpris_status(status)) {
                    let _ = iface.playback_status_changed(emitter).await;
                }
            }
            PlayerEvent::Track { track } => {
                let art = match track.as_ref().and_then(|t| t.artwork.as_ref()) {
                    Some(info) => {
                        let (store, id) = (artwork.clone(), info.id.clone());
                        tokio::task::spawn_blocking(move || store.thumbnail_for(&id)).await.ok().flatten()
                    }
                    None => None,
                };
                {
                    let mut s = state.lock().unwrap();
                    s.track_serial += 1;
                    s.duration_ms = track.as_ref().map(|t| t.properties.duration_ms).filter(|d| *d > 0);
                    s.position_ms = 0;
                    s.track = track;
                    s.art = art;
                }
                let _ = iface.metadata_changed(emitter).await;
                let _ = iface.can_play_changed(emitter).await;
                let _ = iface.can_pause_changed(emitter).await;
                let _ = iface.can_seek_changed(emitter).await;
                let _ = iface.can_go_previous_changed(emitter).await;
            }
            PlayerEvent::Position { position_ms, duration_ms } => {
                let length_changed = {
                    let mut s = state.lock().unwrap();
                    s.position_ms = position_ms;
                    let changed = duration_ms.is_some() && s.duration_ms != duration_ms;
                    if changed {
                        s.duration_ms = duration_ms;
                    }
                    changed
                };
                if length_changed {
                    let _ = iface.metadata_changed(emitter).await;
                }
                if std::mem::take(&mut seek_done) {
                    let _ = Player::seeked(emitter, micros(position_ms)).await;
                }
            }
            PlayerEvent::Volume(volume) => {
                state.lock().unwrap().volume = if volume.muted { 0.0 } else { volume.level };
                let _ = iface.volume_changed(emitter).await;
            }
            PlayerEvent::Queue(queue) => {
                {
                    let mut s = state.lock().unwrap();
                    s.queue_len = queue.entries.len();
                    s.current = queue.current_index;
                    s.repeat = queue.repeat;
                    s.shuffle = queue.shuffle;
                }
                let _ = iface.can_go_next_changed(emitter).await;
                let _ = iface.shuffle_changed(emitter).await;
                let _ = iface.loop_status_changed(emitter).await;
            }
            PlayerEvent::QueueState(queue) => {
                let (shuffle, repeat) = {
                    let mut s = state.lock().unwrap();
                    let changed = (s.shuffle != queue.shuffle, s.repeat != queue.repeat);
                    s.current = queue.current_index;
                    s.repeat = queue.repeat;
                    s.shuffle = queue.shuffle;
                    changed
                };
                let _ = iface.can_go_next_changed(emitter).await;
                if shuffle {
                    let _ = iface.shuffle_changed(emitter).await;
                }
                if repeat {
                    let _ = iface.loop_status_changed(emitter).await;
                }
            }
            PlayerEvent::Signal(_) | PlayerEvent::Error(_) => {}
        }
    }
    Ok(())
}

fn mpris_status(status: PlaybackStatus) -> &'static str {
    match status {
        PlaybackStatus::Playing | PlaybackStatus::Loading | PlaybackStatus::Seeking => "Playing",
        PlaybackStatus::Paused => "Paused",
        _ => "Stopped",
    }
}

/// Fills the state from a snapshot (at start, and after missed events).
async fn seed(state: &Shared, player: &PlayerHandle) {
    let Ok(snap) = player.snapshot().await else { return };
    let mut s = state.lock().unwrap();
    s.status = Some(snap.status);
    if s.track.as_ref().map(|t| &t.path) != snap.track.as_ref().map(|t| &t.path) {
        s.track_serial += 1;
    }
    s.track = snap.track;
    s.position_ms = snap.position_ms;
    s.duration_ms = snap.duration_ms;
    s.volume = if snap.volume.muted { 0.0 } else { snap.volume.level };
    s.repeat = snap.queue.repeat;
    s.shuffle = snap.queue.shuffle;
    s.queue_len = snap.queue.entries.len();
    s.current = snap.queue.current_index;
}

async fn refresh_all(player_ref: &InterfaceRef<Player>) {
    let emitter = player_ref.signal_emitter();
    let iface = player_ref.get().await;
    let _ = iface.playback_status_changed(emitter).await;
    let _ = iface.metadata_changed(emitter).await;
    let _ = iface.volume_changed(emitter).await;
    let _ = iface.shuffle_changed(emitter).await;
    let _ = iface.loop_status_changed(emitter).await;
    let _ = iface.can_go_next_changed(emitter).await;
}
