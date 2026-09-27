//! The player service: one task that owns the audio engine and the queue.
//!
//! Commands from the UI and events from the engine are handled on the same
//! task, one at a time, so playback state has exactly one writer. Callers talk
//! to it through a cloneable [`PlayerHandle`].

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};

use super::queue::Queue;
use super::{
    EntryId, InsertMode, PlaybackStatus, PlayerError, PlayerEvent, PlayerSnapshot, QueueSnapshot, QueueState,
    RepeatMode, SignalPath, TrackInfo, VolumeState,
};
use crate::audio::output::Output;
use crate::audio::{AudioEngine, EngineError, EngineErrorKind, EngineEvent, EngineMessage, EngineState, StreamFormat};
use crate::database::{Database, settings};
use crate::dsp::DspControl;
use crate::dsp::processor::FADE_S;
use crate::error::{AppError, AppResult};
use crate::filesystem;

/// "Previous" restarts the current track instead once it has played this long.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);
/// How often a changed volume is written to the database.
const PERSIST_INTERVAL: Duration = Duration::from_secs(2);
/// The longest a soft pause waits for its fade to become audible.
const MAX_FADE_WAIT: Duration = Duration::from_millis(900);
/// While skipping a run of files that cannot play, each of the first few is
/// reported on its own; the rest are summed up in one error.
const REPORTED_PER_RUN: usize = 3;

pub const SESSION_KEY: &str = "player.session";

/// The queue as saved between runs. Library tracks are found again by id;
/// other files by path.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub entries: Vec<SavedEntry>,
    pub current: Option<usize>,
    pub position_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedEntry {
    pub track_id: Option<i64>,
    pub path: String,
}

/// Persisted player preferences, restored at startup.
#[derive(Debug, Clone)]
pub struct PlayerSettings {
    pub volume: f64,
    pub muted: bool,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub gapless: bool,
    /// Output device id; `None` follows the system default.
    pub output_device: Option<String>,
}

pub const OUTPUT_DEVICE_KEY: &str = "player.output_device";

impl PlayerSettings {
    pub async fn load(db: &Database) -> Self {
        Self {
            // Unity: the system volume is the one people set; a lower
            // default made every track quieter than in other players.
            volume: settings::get(db, "player.volume").await.ok().flatten().unwrap_or(1.0),
            muted: settings::get(db, "player.muted").await.ok().flatten().unwrap_or(false),
            repeat: settings::get(db, "player.repeat").await.ok().flatten().unwrap_or_default(),
            shuffle: settings::get(db, "player.shuffle").await.ok().flatten().unwrap_or(false),
            gapless: settings::get(db, "player.gapless").await.ok().flatten().unwrap_or(true),
            output_device: settings::get(db, OUTPUT_DEVICE_KEY).await.ok().flatten(),
        }
    }
}

#[derive(Debug)]
pub enum PlayerCommand {
    Play,
    Pause,
    TogglePlayPause,
    Stop,
    Next,
    Previous,
    Seek(Duration),
    /// Slider position, 0–1.
    SetVolume(f64),
    SetMuted(bool),
    SetRepeat(RepeatMode),
    SetShuffle(bool),
    /// Continue into the next track without a gap (off: stop and reload).
    SetGapless(bool),
    /// Switches to another output; whatever was playing is reopened there.
    SetOutput(Box<Output>),
    Insert {
        tracks: Vec<Arc<TrackInfo>>,
        mode: InsertMode,
        /// With `Replace`: index of the track to start with.
        start: usize,
    },
    PlayEntry(EntryId),
    RemoveEntry(EntryId),
    /// Moves entries (in play order) before another entry, or to the end.
    MoveEntries {
        ids: Vec<EntryId>,
        before: Option<EntryId>,
    },
    /// Keeps only the current entry.
    ClearOthers,
    /// Puts back a saved session: the queue, the current entry and the
    /// position within it, paused.
    Restore {
        tracks: Vec<Arc<TrackInfo>>,
        start: usize,
        position: Duration,
    },
    ClearQueue,
}

/// Messages the service sends itself from timers.
#[derive(Debug)]
enum Internal {
    /// A soft pause's fade has reached the speakers.
    FadedOut(u64),
}

enum Request {
    Command(PlayerCommand, oneshot::Sender<AppResult<()>>),
    Snapshot(oneshot::Sender<PlayerSnapshot>),
    /// Write pending settings now (the app is exiting).
    Flush(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct PlayerHandle {
    requests: mpsc::Sender<Request>,
    events: broadcast::Sender<PlayerEvent>,
    dsp: Arc<DspControl>,
}

impl PlayerHandle {
    /// Creates the audio engine and the service. The returned future is the
    /// service loop; the caller spawns it on its async runtime.
    pub fn new(
        db: Database,
        settings: PlayerSettings,
        output: Output,
    ) -> AppResult<(Self, impl Future<Output = ()> + Send + 'static)> {
        let (engine_tx, engine_rx) = mpsc::unbounded_channel();
        let engine = AudioEngine::new(engine_tx, output)?;
        let dsp = engine.dsp();
        let (requests, requests_rx) = mpsc::channel(64);
        let (events, _) = broadcast::channel(512);
        let (internal, internal_rx) = mpsc::unbounded_channel();
        let player = Player::new(engine, db, events.clone(), settings, internal);
        Ok((Self { requests, events, dsp }, player.run(requests_rx, engine_rx, internal_rx)))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PlayerEvent> {
        self.events.subscribe()
    }

    /// The processing chain inside the player's pipeline.
    pub fn dsp(&self) -> Arc<DspControl> {
        self.dsp.clone()
    }

    pub async fn command(&self, command: PlayerCommand) -> AppResult<()> {
        let (reply, response) = oneshot::channel();
        self.requests.send(Request::Command(command, reply)).await.map_err(|_| AppError::PlayerUnavailable)?;
        response.await.map_err(|_| AppError::PlayerUnavailable)?
    }

    pub async fn snapshot(&self) -> AppResult<PlayerSnapshot> {
        let (reply, response) = oneshot::channel();
        self.requests.send(Request::Snapshot(reply)).await.map_err(|_| AppError::PlayerUnavailable)?;
        response.await.map_err(|_| AppError::PlayerUnavailable)
    }

    /// Saves settings that are normally written on a timer, such as volume.
    pub async fn flush(&self) {
        let (reply, done) = oneshot::channel();
        if self.requests.send(Request::Flush(reply)).await.is_ok() {
            let _ = done.await;
        }
    }
}

/// What the user last asked for, as opposed to what the pipeline is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    Play,
    Pause,
    Stop,
}

/// Which way playback last moved through the queue, so a file that cannot
/// play is skipped in the same direction ("previous" keeps going back).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Forward,
    Backward,
}

/// Failures met while skipping to something playable.
#[derive(Default)]
struct SkipRun {
    failed: usize,
    /// An entry's failed flag changed, so the queue needs re-sending.
    changed: bool,
}

struct Player {
    engine: AudioEngine,
    dsp: Arc<DspControl>,
    db: Database,
    events: broadcast::Sender<PlayerEvent>,
    internal: mpsc::UnboundedSender<Internal>,
    queue: Queue,
    gapless: bool,

    engine_state: EngineState,
    intent: Intent,
    /// A track was handed to the engine and has not reached `intent` yet.
    loading: bool,
    seeking: bool,
    /// Last status published, so unchanged statuses are not re-sent.
    status: PlaybackStatus,
    position: Duration,
    duration: Option<Duration>,

    volume: f64,
    muted: bool,
    saved_volume: (f64, bool),
    /// The queue or position changed since the session was last saved.
    session_dirty: bool,
    /// A position to seek to once a track has loaded, and whether to play
    /// from there (a restored session stays paused).
    pending_seek: Option<(Duration, bool)>,
    /// The pending seek is running; play once it lands.
    play_after_seek: bool,
    /// Invalidates fade timers that a later command made pointless.
    fade_token: u64,

    error: Option<PlayerError>,
    /// Failures since the last track that played, to stop skipping through
    /// a queue in which nothing is playable.
    failures: usize,
    direction: Direction,
    decoded: Option<StreamFormat>,
    output: Option<StreamFormat>,
}

impl Player {
    fn new(
        engine: AudioEngine,
        db: Database,
        events: broadcast::Sender<PlayerEvent>,
        settings: PlayerSettings,
        internal: mpsc::UnboundedSender<Internal>,
    ) -> Self {
        let mut queue = Queue::default();
        queue.set_repeat(settings.repeat);
        queue.set_shuffle(settings.shuffle);
        let dsp = engine.dsp();
        dsp.set_shuffle(settings.shuffle);
        let player = Self {
            engine,
            dsp,
            db,
            events,
            internal,
            queue,
            gapless: settings.gapless,
            engine_state: EngineState::Null,
            intent: Intent::Stop,
            loading: false,
            seeking: false,
            status: PlaybackStatus::Idle,
            position: Duration::ZERO,
            duration: None,
            volume: settings.volume.clamp(0.0, 1.0),
            muted: settings.muted,
            saved_volume: (settings.volume, settings.muted),
            session_dirty: false,
            pending_seek: None,
            play_after_seek: false,
            fade_token: 0,
            error: None,
            failures: 0,
            direction: Direction::Forward,
            decoded: None,
            output: None,
        };
        player.apply_volume();
        player
    }

    async fn run(
        mut self,
        mut requests: mpsc::Receiver<Request>,
        mut engine_events: mpsc::UnboundedReceiver<EngineMessage>,
        mut internal: mpsc::UnboundedReceiver<Internal>,
    ) {
        let mut persist = tokio::time::interval(PERSIST_INTERVAL);
        persist.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                request = requests.recv() => match request {
                    Some(Request::Command(command, reply)) => {
                        let result = self.handle(command).await;
                        if let Err(e) = &result {
                            log::debug!("player command failed: {e}");
                        }
                        let _ = reply.send(result);
                    }
                    Some(Request::Snapshot(reply)) => {
                        let _ = reply.send(self.snapshot());
                    }
                    Some(Request::Flush(reply)) => {
                        self.persist_volume().await;
                        // Keep the exact position of the track being left.
                        self.session_dirty |= self.queue.current().is_some();
                        self.persist_session().await;
                        let _ = reply.send(());
                    }
                    None => break,
                },
                Some(message) = engine_events.recv() => {
                    // Events from a stream that has since been replaced or
                    // stopped describe nothing the user is hearing.
                    if message.epoch == self.engine.epoch() {
                        self.on_engine(message.event);
                    }
                }
                Some(message) = internal.recv() => self.on_internal(message),
                _ = persist.tick() => {
                    self.persist_volume().await;
                    self.persist_session().await;
                }
            }
        }
        self.persist_volume().await;
        self.persist_session().await;
    }

    // ── commands ────────────────────────────────────────────────────────────

    async fn handle(&mut self, command: PlayerCommand) -> AppResult<()> {
        match command {
            PlayerCommand::Play => self.play(),
            PlayerCommand::Pause => self.pause(),
            PlayerCommand::TogglePlayPause => {
                if self.wants_play() {
                    self.pause()
                } else {
                    self.play()
                }
            }
            PlayerCommand::Stop => self.stop(),
            PlayerCommand::Next => self.next(),
            PlayerCommand::Previous => self.previous(),
            PlayerCommand::Seek(position) => self.seek(position),
            PlayerCommand::SetVolume(level) => {
                self.volume = level.clamp(0.0, 1.0);
                self.apply_volume();
                self.emit(PlayerEvent::Volume(self.volume_state()));
                Ok(())
            }
            PlayerCommand::SetMuted(muted) => {
                self.muted = muted;
                self.apply_volume();
                self.emit(PlayerEvent::Volume(self.volume_state()));
                Ok(())
            }
            PlayerCommand::SetRepeat(repeat) => {
                self.queue.set_repeat(repeat);
                self.update_next();
                self.emit_queue_state();
                settings::set(&self.db, "player.repeat", &repeat).await
            }
            PlayerCommand::SetShuffle(shuffle) => {
                self.queue.set_shuffle(shuffle);
                self.dsp.set_shuffle(shuffle);
                self.update_next();
                self.emit_queue();
                settings::set(&self.db, "player.shuffle", &shuffle).await
            }
            PlayerCommand::SetGapless(gapless) => {
                self.gapless = gapless;
                self.update_next();
                self.emit_queue_state();
                settings::set(&self.db, "player.gapless", &gapless).await
            }
            PlayerCommand::SetOutput(output) => self.set_output(*output),
            PlayerCommand::Insert { tracks, mode, start } => self.insert(tracks, mode, start),
            PlayerCommand::PlayEntry(id) => {
                self.queue.jump_to(id).ok_or_else(|| AppError::InvalidArgument("no such queue entry".into()))?;
                self.failures = 0;
                self.direction = Direction::Forward;
                self.load_current(true)
            }
            PlayerCommand::RemoveEntry(id) => self.remove(id),
            PlayerCommand::MoveEntries { ids, before } => {
                if self.queue.move_entries(&ids, before) {
                    self.emit_queue();
                    self.update_next();
                }
                Ok(())
            }
            PlayerCommand::Restore { tracks, start, position } => {
                self.queue.insert(tracks, InsertMode::Replace, start);
                self.emit_queue();
                self.failures = 0;
                self.direction = Direction::Forward;
                let seek = (position > Duration::from_secs(1)).then_some((position, false));
                self.load_current_at(false, seek)
            }
            PlayerCommand::ClearOthers => {
                self.queue.clear_others();
                self.emit_queue();
                self.update_next();
                Ok(())
            }
            PlayerCommand::ClearQueue => {
                self.queue.clear();
                self.engine.stop()?;
                self.engine_state = EngineState::Ready;
                self.intent = Intent::Stop;
                self.loading = false;
                self.seeking = false;
                self.position = Duration::ZERO;
                self.duration = None;
                self.error = None;
                self.emit_track();
                self.emit_queue();
                self.emit_position();
                self.refresh_status();
                Ok(())
            }
        }
    }

    fn play(&mut self) -> AppResult<()> {
        if self.queue.current().is_none() {
            return Ok(());
        }
        // A track is opening at a saved position: play once it gets there.
        if let Some((_, play)) = &mut self.pending_seek {
            *play = true;
            self.refresh_status();
            return Ok(());
        }
        if self.seeking && self.play_after_seek {
            return Ok(());
        }
        let unloaded = !self.loading && matches!(self.engine_state, EngineState::Ready | EngineState::Null);
        if self.intent == Intent::Stop || unloaded {
            self.failures = 0;
            return self.load_current(true);
        }
        self.intent = Intent::Play;
        self.cancel_fade();
        // After a soft pause the chain is silent: bring the sound back in.
        self.dsp.fade(1.0, if self.soft_fades() { FADE_S } else { 0.0 });
        self.engine.play()?;
        self.refresh_status();
        Ok(())
    }

    fn pause(&mut self) -> AppResult<()> {
        if let Some((_, play)) = &mut self.pending_seek {
            *play = false;
        }
        self.play_after_seek = false;
        if self.intent != Intent::Play {
            self.refresh_status();
            return Ok(());
        }
        self.intent = Intent::Pause;
        let audible = self.engine_state == EngineState::Playing && !self.loading && !self.seeking;
        if self.soft_fades() && audible {
            self.fade_out_then_pause();
        } else {
            self.engine.pause()?;
        }
        self.refresh_status();
        Ok(())
    }

    /// Whether playback is, or is about to be, audible.
    fn wants_play(&self) -> bool {
        self.intent == Intent::Play || self.pending_seek.is_some_and(|(_, play)| play) || self.play_after_seek
    }

    fn soft_fades(&self) -> bool {
        self.dsp.settings().soft_fades
    }

    /// Fades to silence, then pauses once the silence is what the output
    /// plays: processing runs ahead of the speakers by the output buffer.
    fn fade_out_then_pause(&mut self) {
        self.fade_token += 1;
        self.dsp.fade(0.0, FADE_S);
        let ahead = match self.engine.running_time() {
            Some(now) => Duration::from_nanos(self.dsp.processed_running_time().saturating_sub(now)),
            None => Duration::ZERO,
        };
        let wait = (Duration::from_secs_f64(FADE_S) + ahead + Duration::from_millis(20)).min(MAX_FADE_WAIT);
        let (token, internal) = (self.fade_token, self.internal.clone());
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            let _ = internal.send(Internal::FadedOut(token));
        });
    }

    fn cancel_fade(&mut self) {
        self.fade_token += 1;
    }

    fn on_internal(&mut self, message: Internal) {
        match message {
            Internal::FadedOut(token) if token == self.fade_token && self.intent == Intent::Pause => {
                if let Err(e) = self.engine.pause() {
                    log::error!("could not pause after fading out: {e}");
                }
                self.refresh_status();
            }
            Internal::FadedOut(_) => {}
        }
    }

    /// Moves playback to another output, reopening the current track at the
    /// same position.
    fn set_output(&mut self, output: Output) -> AppResult<()> {
        let active = self.queue.current().is_some() && self.intent != Intent::Stop && self.error.is_none();
        let position = if self.loading { self.position } else { self.engine.position().unwrap_or(self.position) };
        let play = self.intent == Intent::Play;
        self.cancel_fade();
        self.engine.set_output(output)?;
        self.engine_state = EngineState::Ready;
        self.loading = false;
        self.seeking = false;
        self.decoded = None;
        self.output = None;
        self.emit_signal();
        if !active {
            self.refresh_status();
            return Ok(());
        }
        self.failures = 0;
        if position > Duration::from_secs(1) {
            // Open paused and seek first, so the start of the track is
            // never heard on the new output.
            return self.load_current_at(false, Some((position, play)));
        }
        self.load_current(play)
    }

    fn stop(&mut self) -> AppResult<()> {
        self.engine.stop()?;
        self.engine_state = EngineState::Ready;
        self.intent = Intent::Stop;
        self.loading = false;
        self.seeking = false;
        self.position = Duration::ZERO;
        self.emit_position();
        self.refresh_status();
        Ok(())
    }

    fn next(&mut self) -> AppResult<()> {
        if self.queue.advance().is_none() {
            return Ok(());
        }
        self.failures = 0;
        self.direction = Direction::Forward;
        self.switch_to_current()
    }

    fn previous(&mut self) -> AppResult<()> {
        let loaded = !self.loading && matches!(self.engine_state, EngineState::Paused | EngineState::Playing);
        if loaded && self.position > RESTART_THRESHOLD {
            return self.seek(Duration::ZERO);
        }
        if self.queue.retreat().is_none() {
            return if loaded { self.seek(Duration::ZERO) } else { Ok(()) };
        }
        self.failures = 0;
        self.direction = Direction::Backward;
        self.switch_to_current()
    }

    fn seek(&mut self, position: Duration) -> AppResult<()> {
        if self.loading || !matches!(self.engine_state, EngineState::Paused | EngineState::Playing) {
            return Err(AppError::InvalidArgument("nothing is playing".into()));
        }
        let position = self.duration.map_or(position, |d| position.min(d));
        self.engine.seek(position)?;
        self.seeking = true;
        self.position = position;
        self.emit_position();
        self.refresh_status();
        Ok(())
    }

    fn insert(&mut self, tracks: Vec<Arc<TrackInfo>>, mode: InsertMode, start: usize) -> AppResult<()> {
        let had_current = self.queue.current().is_some();
        self.queue.insert(tracks, mode, start);
        self.emit_queue();
        if mode == InsertMode::Replace || !had_current {
            self.failures = 0;
            self.direction = Direction::Forward;
            return self.load_current(true);
        }
        self.update_next();
        Ok(())
    }

    fn remove(&mut self, id: EntryId) -> AppResult<()> {
        let was_current =
            self.queue.remove(id).ok_or_else(|| AppError::InvalidArgument("no such queue entry".into()))?;
        self.emit_queue();
        if !was_current {
            self.update_next();
            return Ok(());
        }
        if self.queue.current().is_some() {
            return self.switch_to_current();
        }
        self.engine.stop()?;
        self.engine_state = EngineState::Ready;
        self.intent = Intent::Stop;
        self.position = Duration::ZERO;
        self.duration = None;
        self.emit_track();
        self.emit_position();
        self.refresh_status();
        Ok(())
    }

    /// After the current entry changed: load it if something was playing or
    /// paused, otherwise just select it and stay stopped.
    fn switch_to_current(&mut self) -> AppResult<()> {
        match self.intent {
            Intent::Stop => {
                self.position = Duration::ZERO;
                self.duration = self.queue.current().and_then(|e| known_duration(&e.track));
                self.update_next();
                self.emit_track();
                self.emit_queue_state();
                self.emit_position();
                self.refresh_status();
                Ok(())
            }
            intent => self.load_current(intent == Intent::Play),
        }
    }

    fn load_current(&mut self, play: bool) -> AppResult<()> {
        self.load_current_at(play, None)
    }

    /// Loads the current entry; with `seek`, it opens paused, seeks, and
    /// then plays if the seek says so. A file that cannot be opened is
    /// reported (and skipped, if playing) like any playback error.
    fn load_current_at(&mut self, play: bool, seek: Option<(Duration, bool)>) -> AppResult<()> {
        if let Err(err) = self.try_load_current(play, seek) {
            self.on_error(err);
        }
        Ok(())
    }

    /// [`Self::load_current_at`] without the error handling: returns why the
    /// current entry could not be opened.
    fn try_load_current(&mut self, play: bool, seek: Option<(Duration, bool)>) -> Result<(), EngineError> {
        self.pending_seek = seek;
        self.play_after_seek = false;
        let Some(track) = self.queue.current().map(|e| e.track.clone()) else {
            return Ok(());
        };
        let uri = filesystem::path_to_uri(&track.path).map_err(|e| EngineError {
            kind: EngineErrorKind::NotFound,
            message: e.to_string(),
            detail: None,
            uri: None,
        })?;
        self.intent = if play { Intent::Play } else { Intent::Pause };
        self.cancel_fade();
        self.dsp.fade(1.0, 0.0);
        self.loading = true;
        self.seeking = false;
        self.position = Duration::ZERO;
        self.duration = known_duration(&track);
        self.decoded = None;
        self.output = None;

        // Checked here rather than left to GStreamer: a missing file fails
        // the pipeline's state change, and costs a reset to recover from.
        let loaded = if track.path.exists() {
            self.engine.load(&uri, self.next_uri(), play)
        } else {
            Err(EngineError {
                kind: EngineErrorKind::NotFound,
                message: "The file could not be found".into(),
                detail: Some(track.display_path.clone()),
                uri: Some(uri),
            })
        };
        self.engine_state = EngineState::Ready;
        self.emit_track();
        self.emit_queue_state();
        self.emit_position();
        self.emit_signal();
        loaded?;
        self.refresh_status();
        Ok(())
    }

    /// The stream to continue with gaplessly after the current one.
    fn next_uri(&self) -> Option<String> {
        if !self.gapless {
            return None;
        }
        self.queue.peek_auto_next().and_then(|e| filesystem::path_to_uri(&e.track.path).ok())
    }

    /// Tells the engine what to continue with after the queue changed.
    fn update_next(&self) {
        self.engine.set_next(self.next_uri());
    }

    // ── engine events ───────────────────────────────────────────────────────

    fn on_engine(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::StateChanged(state) => {
                self.engine_state = state;
                let reached = matches!(
                    (self.intent, state),
                    (Intent::Play, EngineState::Playing) | (Intent::Pause, EngineState::Paused)
                );
                if self.loading && reached {
                    self.loading = false;
                    self.error = None;
                    if let Some(id) = self.queue.current().map(|e| e.id)
                        && self.queue.set_failed(id, false)
                    {
                        self.emit_queue();
                    }
                }
                if state == EngineState::Playing {
                    self.failures = 0;
                }
                self.refresh_status();
            }
            EngineEvent::StreamStarted { uri, gapless: true } => self.on_gapless_switch(&uri),
            EngineEvent::StreamStarted { .. } => {}
            EngineEvent::AsyncDone if self.pending_seek.is_some() && !self.loading => {
                if let Some((position, play)) = self.pending_seek.take() {
                    match self.seek(position) {
                        Ok(()) => self.play_after_seek = play,
                        Err(e) => {
                            log::debug!("could not restore the position: {e}");
                            if play && let Err(e) = self.play() {
                                log::error!("could not resume: {e}");
                            }
                        }
                    }
                }
                self.refresh_status();
            }
            EngineEvent::AsyncDone => {
                if self.seeking {
                    self.seeking = false;
                    if let Some(position) = self.engine.position() {
                        self.position = position;
                    }
                    self.emit_position();
                    if std::mem::take(&mut self.play_after_seek) {
                        // Still opening, as far as the user can tell, until it plays.
                        self.loading = true;
                        if let Err(e) = self.play() {
                            self.loading = false;
                            log::error!("could not resume after seeking: {e}");
                        }
                    }
                }
                self.refresh_status();
            }
            EngineEvent::EndOfStream => self.on_end_of_stream(),
            EngineEvent::Error(err) => self.on_error(err),
            EngineEvent::Position { position, duration } => {
                if self.seeking {
                    return;
                }
                self.position = position;
                if duration.is_some() {
                    self.duration = duration;
                }
                self.emit_position();
            }
            EngineEvent::DurationChanged => {
                if let Some(duration) = self.engine.duration() {
                    self.duration = Some(duration);
                    self.emit_position();
                }
            }
            EngineEvent::DecodedFormat(format) => {
                self.decoded = Some(format);
                self.emit_signal();
            }
            EngineEvent::OutputFormat(format) => {
                self.output = Some(format);
                self.emit_signal();
            }
        }
    }

    fn on_gapless_switch(&mut self, uri: &str) {
        let expected = self.queue.peek_auto_next().and_then(|e| filesystem::path_to_uri(&e.track.path).ok());
        if expected.as_deref() == Some(uri) {
            self.queue.advance_auto();
        } else {
            // The queue changed after the engine had already queued the next
            // stream; follow the engine so the UI shows what is audible.
            log::warn!("gapless switch to an unexpected stream: {uri}");
            let (entries, _) = self.queue.play_order();
            let id = entries
                .iter()
                .find(|e| filesystem::path_to_uri(&e.track.path).ok().as_deref() == Some(uri))
                .map(|e| e.id);
            if let Some(id) = id {
                self.queue.jump_to(id);
            }
        }
        self.position = Duration::ZERO;
        self.duration = self.queue.current().and_then(|e| known_duration(&e.track));
        self.error = None;
        self.update_next();
        self.emit_track();
        self.emit_queue_state();
        self.emit_position();
    }

    /// End of stream is only reached when no gapless continuation was queued.
    fn on_end_of_stream(&mut self) {
        if self.queue.peek_auto_next().is_some() {
            // Repeat or the queue changed after the continuation point.
            self.queue.advance_auto();
            if let Err(e) = self.load_current(true) {
                log::error!("could not continue after end of stream: {e}");
            }
            return;
        }
        if let Err(e) = self.stop() {
            log::error!("could not stop at end of queue: {e}");
        }
    }

    fn on_error(&mut self, err: EngineError) {
        self.loading = false;
        self.seeking = false;
        self.pending_seek = None;
        // Release the failed stream (and the output) entirely before trying
        // anything else; whatever it still posts is marked stale.
        self.engine.recover();
        self.engine_state = EngineState::Null;

        // A continuation queued gaplessly failed while the track before it
        // was still current: step onto the failed entry, so it is marked and
        // skipped rather than tried again.
        let current_uri = self.queue.current().and_then(|e| filesystem::path_to_uri(&e.track.path).ok());
        let next_uri = self.queue.peek_auto_next().and_then(|e| filesystem::path_to_uri(&e.track.path).ok());
        if err.uri.is_some() && err.uri != current_uri && err.uri == next_uri {
            self.queue.advance_auto();
            self.emit_track();
            self.emit_queue_state();
        }

        let recoverable = err.kind != EngineErrorKind::OutputUnavailable;
        let mut run = SkipRun::default();
        self.report_failure(err, &mut run);
        if recoverable && self.intent == Intent::Play && self.skip_to_playable(&mut run) {
            self.finish_run(&run);
            return;
        }
        self.finish_run(&run);
        self.intent = Intent::Stop;
        self.position = Duration::ZERO;
        self.emit_position();
        self.refresh_status();
    }

    /// Moves past entries that cannot play, in the direction playback was
    /// going, until one opens. Returns whether one did. Iterative, so a
    /// queue full of missing files (an unplugged drive) cannot overflow the
    /// stack.
    fn skip_to_playable(&mut self, run: &mut SkipRun) -> bool {
        while self.failures < self.queue.len() {
            let moved = match self.direction {
                Direction::Forward => self.queue.advance().is_some(),
                // Nothing playable further back: go forward from here.
                Direction::Backward => self.queue.retreat().is_some() || {
                    self.direction = Direction::Forward;
                    self.queue.advance().is_some()
                },
            };
            if !moved {
                return false;
            }
            match self.try_load_current(true, None) {
                Ok(()) => return true,
                Err(err) => {
                    // A failed load has already taken the pipeline down; a
                    // missing file never reached it.
                    self.engine_state = EngineState::Null;
                    self.report_failure(err, run);
                }
            }
        }
        false
    }

    /// Counts a failure, flags the entry, and reports it unless this run
    /// has already reported enough.
    fn report_failure(&mut self, err: EngineError, run: &mut SkipRun) {
        self.failures += 1;
        if let Some(id) = self.queue.current().map(|e| e.id) {
            run.changed |= self.queue.set_failed(id, true);
        }
        let error = PlayerError {
            kind: err.kind,
            message: err.message,
            detail: err.detail,
            file_name: err.uri.as_deref().and_then(file_name_from_uri),
        };
        run.failed += 1;
        if run.failed <= REPORTED_PER_RUN {
            self.emit(PlayerEvent::Error(error.clone()));
        }
        self.error = Some(error);
    }

    fn finish_run(&mut self, run: &SkipRun) {
        if run.changed {
            self.emit_queue();
        }
        let unreported = run.failed.saturating_sub(REPORTED_PER_RUN);
        if unreported > 0 {
            self.emit(PlayerEvent::Error(PlayerError {
                kind: EngineErrorKind::Other,
                message: format!("{unreported} more files could not be played"),
                detail: None,
                file_name: None,
            }));
        }
    }

    // ── state & events ──────────────────────────────────────────────────────

    fn compute_status(&self) -> PlaybackStatus {
        if self.queue.current().is_none() {
            return if self.error.is_some() { PlaybackStatus::Error } else { PlaybackStatus::Idle };
        }
        match self.intent {
            Intent::Stop if self.error.is_some() => PlaybackStatus::Error,
            Intent::Stop => PlaybackStatus::Stopped,
            _ if self.loading || self.pending_seek.is_some_and(|(_, play)| play) => PlaybackStatus::Loading,
            _ if self.seeking => PlaybackStatus::Seeking,
            // Once a track is loaded, the pipeline's own PLAYING ⇄ PAUSED
            // transitions (a seek briefly prerolls, a soft pause fades out
            // first) are not states the user should see flicker through.
            _ if matches!(self.engine_state, EngineState::Ready | EngineState::Null) => PlaybackStatus::Loading,
            Intent::Play => PlaybackStatus::Playing,
            Intent::Pause => PlaybackStatus::Paused,
        }
    }

    fn refresh_status(&mut self) {
        let status = self.compute_status();
        if status != self.status {
            self.status = status;
            self.emit(PlayerEvent::Status { status, error: self.error.clone() });
        }
    }

    fn apply_volume(&self) {
        // A cubic curve matches loudness perception far better than a linear
        // one; it is the same mapping PulseAudio and PipeWire mixers use.
        self.engine.set_volume(self.volume.powi(3));
        self.engine.set_muted(self.muted);
    }

    fn volume_state(&self) -> VolumeState {
        let gain_db = (!self.muted && self.volume > 0.0).then(|| 60.0 * self.volume.log10());
        VolumeState { level: self.volume, muted: self.muted, gain_db }
    }

    async fn persist_volume(&mut self) {
        let current = (self.volume, self.muted);
        if current == self.saved_volume {
            return;
        }
        let saved = async {
            settings::set(&self.db, "player.volume", &self.volume).await?;
            settings::set(&self.db, "player.muted", &self.muted).await
        };
        match saved.await {
            Ok(()) => self.saved_volume = current,
            Err(e) => log::warn!("could not save volume: {e}"),
        }
    }

    /// Saves the queue, the current entry and the position, so the session
    /// can be restored after a restart.
    async fn persist_session(&mut self) {
        if !self.session_dirty {
            return;
        }
        let (entries, current) = self.queue.play_order();
        let session = SavedSession {
            entries: entries
                .iter()
                .map(|e| SavedEntry {
                    track_id: e.track.library.map(|l| l.track_id),
                    path: e.track.path.to_string_lossy().into_owned(),
                })
                .collect(),
            current,
            position_ms: millis(self.position),
        };
        match settings::set(&self.db, SESSION_KEY, &session).await {
            Ok(()) => self.session_dirty = false,
            Err(e) => log::warn!("could not save the session: {e}"),
        }
    }

    fn queue_snapshot(&self) -> QueueSnapshot {
        let (entries, current_index) = self.queue.play_order();
        QueueSnapshot {
            entries,
            current_index,
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
            gapless: self.gapless,
        }
    }

    fn signal_path(&self) -> SignalPath {
        SignalPath {
            backend: self.engine.backend(),
            device: self.engine.device().cloned(),
            buffer_ms: self.engine.buffer_ms(),
            decoded: self.decoded.clone(),
            output: self.output.clone(),
        }
    }

    fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            status: self.status,
            track: self.queue.current().map(|e| e.track.clone()),
            position_ms: millis(self.position),
            duration_ms: self.duration.map(millis),
            volume: self.volume_state(),
            queue: self.queue_snapshot(),
            signal: self.signal_path(),
            error: self.error.clone(),
        }
    }

    fn emit(&self, event: PlayerEvent) {
        // No subscribers is fine: the frontend may not have loaded yet.
        let _ = self.events.send(event);
    }

    fn emit_track(&mut self) {
        self.session_dirty = true;
        self.emit(PlayerEvent::Track { track: self.queue.current().map(|e| e.track.clone()) });
    }

    /// The whole queue: only after its contents or order changed.
    fn emit_queue(&mut self) {
        self.session_dirty = true;
        self.emit(PlayerEvent::Queue(self.queue_snapshot()));
    }

    /// Cursor and modes only, which is all that changes on a track change.
    fn emit_queue_state(&mut self) {
        self.session_dirty = true;
        self.emit(PlayerEvent::QueueState(QueueState {
            current_index: self.queue.current_index(),
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
            gapless: self.gapless,
        }));
    }

    fn emit_position(&self) {
        self.emit(PlayerEvent::Position { position_ms: millis(self.position), duration_ms: self.duration.map(millis) });
    }

    fn emit_signal(&self) {
        self.emit(PlayerEvent::Signal(self.signal_path()));
    }
}

fn known_duration(track: &TrackInfo) -> Option<Duration> {
    (track.properties.duration_ms > 0).then(|| Duration::from_millis(track.properties.duration_ms))
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn file_name_from_uri(uri: &str) -> Option<String> {
    let path = url::Url::parse(uri).ok()?.to_file_path().ok()?;
    Some(path.file_name()?.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::artwork::ArtworkStore;
    use crate::test_support::write_flac;

    struct Harness {
        player: PlayerHandle,
        events: broadcast::Receiver<PlayerEvent>,
        artwork: Arc<ArtworkStore>,
        dir: tempfile::TempDir,
    }

    impl Harness {
        /// Reads the files and replaces the queue with them.
        async fn open(&self, paths: Vec<PathBuf>) -> usize {
            let tracks: Vec<_> = paths.iter().map(|p| Arc::new(TrackInfo::load(p, &self.artwork))).collect();
            let count = tracks.len();
            self.player.command(PlayerCommand::Insert { tracks, mode: InsertMode::Replace, start: 0 }).await.unwrap();
            count
        }
    }

    async fn harness() -> Harness {
        gstreamer::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = Database::in_memory().await.unwrap();
        let artwork = Arc::new(ArtworkStore::new(dir.path().join("art")).unwrap());
        let settings = PlayerSettings {
            volume: 0.8,
            muted: false,
            repeat: RepeatMode::Off,
            shuffle: false,
            gapless: true,
            output_device: None,
        };
        let (player, service) = PlayerHandle::new(db, settings, Output::fake(true)).unwrap();
        tokio::spawn(service);
        let events = player.subscribe();
        Harness { player, events, artwork, dir }
    }

    /// Collects events up to and including the first one matching `done`.
    async fn wait_for(
        events: &mut broadcast::Receiver<PlayerEvent>,
        done: impl Fn(&PlayerEvent) -> bool,
    ) -> Vec<PlayerEvent> {
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            match tokio::time::timeout_at(deadline, events.recv()).await {
                Ok(Ok(event)) => {
                    let finished = done(&event);
                    seen.push(event);
                    if finished {
                        return seen;
                    }
                }
                Ok(Err(e)) => panic!("player event stream failed: {e}"),
                Err(_) => panic!("timed out waiting for player event; saw {seen:#?}"),
            }
        }
    }

    fn is_status(status: PlaybackStatus) -> impl Fn(&PlayerEvent) -> bool {
        move |e| matches!(e, PlayerEvent::Status { status: s, .. } if *s == status)
    }

    fn statuses(events: &[PlayerEvent]) -> Vec<PlaybackStatus> {
        events
            .iter()
            .filter_map(|e| if let PlayerEvent::Status { status, .. } = e { Some(*status) } else { None })
            .collect()
    }

    fn track_titles(events: &[PlayerEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                PlayerEvent::Track { track: Some(t) } => Some(t.metadata.title.clone()),
                _ => None,
            })
            .collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plays_the_queue_gaplessly_and_stops_at_the_end() {
        let mut h = harness().await;
        let (a, b) = (h.dir.path().join("a.flac"), h.dir.path().join("b.flac"));
        write_flac(&a, 0.4, "First");
        write_flac(&b, 0.4, "Second");

        assert_eq!(h.open(vec![a, b]).await, 2);
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Stopped)).await;

        assert_eq!(track_titles(&events), ["First", "Second"]);
        assert_eq!(statuses(&events), [PlaybackStatus::Loading, PlaybackStatus::Playing, PlaybackStatus::Stopped]);

        let snapshot = h.player.snapshot().await.unwrap();
        let track = snapshot.track.expect("last track stays selected");
        assert_eq!(track.metadata.title, "Second");
        assert_eq!(track.metadata.artist.as_deref(), Some("Test Artist"));
        assert_eq!(track.properties.codec, "FLAC");
        assert_eq!((track.properties.sample_rate, track.properties.bit_depth), (Some(44_100), Some(16)));
        assert_eq!(snapshot.position_ms, 0);
        assert_eq!(snapshot.queue.current_index, Some(1));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn skips_files_that_cannot_play() {
        let mut h = harness().await;
        let bad = h.dir.path().join("broken.flac");
        std::fs::write(&bad, b"not audio").unwrap();
        let good = h.dir.path().join("good.flac");
        write_flac(&good, 2.0, "Good");

        h.open(vec![bad, good]).await;
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        let error = events.iter().find_map(|e| if let PlayerEvent::Error(err) = e { Some(err) } else { None });
        assert_eq!(error.expect("the broken file is reported").file_name.as_deref(), Some("broken.flac"));
        assert_eq!(track_titles(&events).last().map(String::as_str), Some("Good"));
    }

    fn errors(events: &[PlayerEvent]) -> Vec<&PlayerError> {
        events.iter().filter_map(|e| if let PlayerEvent::Error(err) = e { Some(err) } else { None }).collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_file_gone_from_disk_is_skipped_and_the_next_one_plays() {
        let mut h = harness().await;
        let (a, gone, c) = (h.dir.path().join("a.flac"), h.dir.path().join("gone.flac"), h.dir.path().join("c.flac"));
        write_flac(&a, 4.0, "First");
        write_flac(&gone, 1.0, "Gone");
        write_flac(&c, 4.0, "Third");
        h.open(vec![a, gone.clone(), c]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        std::fs::remove_file(&gone).unwrap();
        h.player.command(PlayerCommand::Next).await.unwrap();
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        let errors = errors(&events);
        assert_eq!(errors.len(), 1, "only the missing file is reported: {errors:#?}");
        assert_eq!(errors[0].file_name.as_deref(), Some("gone.flac"));
        assert_eq!(errors[0].kind, EngineErrorKind::NotFound);
        let snapshot = h.player.snapshot().await.unwrap();
        assert_eq!(snapshot.track.unwrap().metadata.title, "Third", "the track after the missing one plays");
        assert!(snapshot.queue.entries[1].failed, "the missing entry is flagged");
        assert!(!snapshot.queue.entries[2].failed);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_gapless_continuation_gone_from_disk_is_skipped() {
        let mut h = harness().await;
        let (a, gone, c) = (h.dir.path().join("a.flac"), h.dir.path().join("gone.flac"), h.dir.path().join("c.flac"));
        write_flac(&a, 1.0, "First");
        write_flac(&gone, 1.0, "Gone");
        write_flac(&c, 0.5, "Third");
        h.open(vec![a, gone.clone(), c]).await;
        // Queued as the continuation already; gone before it is reached.
        std::fs::remove_file(&gone).unwrap();
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Stopped)).await;

        let errors = errors(&events);
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].file_name.as_deref(), Some("gone.flac"), "the error names the file that is gone");
        assert_eq!(track_titles(&events).last().map(String::as_str), Some("Third"), "the track after it played");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_file_that_fails_to_open_does_not_break_the_next_one() {
        use std::os::unix::fs::PermissionsExt;
        let mut h = harness().await;
        let (a, locked, c) = (h.dir.path().join("a.flac"), h.dir.path().join("locked.flac"), h.dir.path().join("c.flac"));
        write_flac(&a, 4.0, "First");
        write_flac(&locked, 1.0, "Locked");
        write_flac(&c, 4.0, "Third");
        h.open(vec![a, locked.clone(), c]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        // Unreadable: GStreamer fails the state change itself, which used to
        // leave the pipeline unable to start the following track too.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        h.player.command(PlayerCommand::Next).await.unwrap();
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

        let errors = errors(&events);
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|e| e.file_name.as_deref() == Some("locked.flac")), "{errors:#?}");
        assert_eq!(h.player.snapshot().await.unwrap().track.unwrap().metadata.title, "Third");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn previous_skips_back_past_a_broken_file() {
        let mut h = harness().await;
        let (a, bad, c) = (h.dir.path().join("a.flac"), h.dir.path().join("bad.flac"), h.dir.path().join("c.flac"));
        write_flac(&a, 4.0, "First");
        std::fs::write(&bad, b"not audio").unwrap();
        write_flac(&c, 4.0, "Third");
        let tracks = [a, bad, c].iter().map(|p| Arc::new(TrackInfo::load(p, &h.artwork))).collect();
        h.player.command(PlayerCommand::Insert { tracks, mode: InsertMode::Replace, start: 2 }).await.unwrap();
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        h.player.command(PlayerCommand::Previous).await.unwrap();
        wait_for(&mut h.events, |e| matches!(e, PlayerEvent::Error(_))).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        let snapshot = h.player.snapshot().await.unwrap();
        assert_eq!(snapshot.track.unwrap().metadata.title, "First", "previous keeps going back");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pause_seek_and_resume() {
        let mut h = harness().await;
        let a = h.dir.path().join("a.flac");
        write_flac(&a, 4.0, "Long");
        h.open(vec![a]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        h.player.command(PlayerCommand::Pause).await.unwrap();
        wait_for(&mut h.events, is_status(PlaybackStatus::Paused)).await;

        h.player.command(PlayerCommand::Seek(Duration::from_millis(2500))).await.unwrap();
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Paused)).await;
        assert_eq!(statuses(&events), [PlaybackStatus::Seeking, PlaybackStatus::Paused]);
        let position = h.player.snapshot().await.unwrap().position_ms;
        assert!((2450..=2550).contains(&position), "position after seek: {position}");

        h.player.command(PlayerCommand::TogglePlayPause).await.unwrap();
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        h.player.command(PlayerCommand::Seek(Duration::from_secs(60))).await.unwrap();
        let clamped = h.player.snapshot().await.unwrap();
        assert!(clamped.position_ms <= clamped.duration_ms.unwrap(), "seeks are clamped to the duration");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn restores_a_session_paused_at_its_position() {
        let mut h = harness().await;
        let (a, b) = (h.dir.path().join("a.flac"), h.dir.path().join("b.flac"));
        write_flac(&a, 1.0, "First");
        write_flac(&b, 4.0, "Second");
        let tracks = [a, b].iter().map(|p| Arc::new(TrackInfo::load(p, &h.artwork))).collect();
        h.player
            .command(PlayerCommand::Restore { tracks, start: 1, position: Duration::from_millis(2500) })
            .await
            .unwrap();
        wait_for(&mut h.events, |e| matches!(e, PlayerEvent::Position { position_ms, .. } if *position_ms >= 2000))
            .await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Paused)).await;

        let snapshot = h.player.snapshot().await.unwrap();
        assert_eq!(snapshot.track.unwrap().metadata.title, "Second");
        assert_eq!(snapshot.queue.current_index, Some(1));
        assert!((2400..=2600).contains(&snapshot.position_ms), "position: {}", snapshot.position_ms);
        assert!(snapshot.queue.entries.iter().all(|e| !e.added), "a restored queue is not 'added by you'");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn seeking_while_playing_never_reports_paused() {
        let mut h = harness().await;
        let a = h.dir.path().join("a.flac");
        write_flac(&a, 4.0, "Long");
        h.open(vec![a]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        h.player.command(PlayerCommand::Seek(Duration::from_millis(2000))).await.unwrap();
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        assert_eq!(statuses(&events), [PlaybackStatus::Seeking, PlaybackStatus::Playing]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_soft_pause_really_stops_the_pipeline() {
        let mut h = harness().await;
        let a = h.dir.path().join("a.flac");
        write_flac(&a, 4.0, "Long");
        h.open(vec![a]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        h.player.command(PlayerCommand::Pause).await.unwrap();
        wait_for(&mut h.events, is_status(PlaybackStatus::Paused)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let first = h.player.snapshot().await.unwrap().position_ms;
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(h.player.snapshot().await.unwrap().position_ms, first, "position holds once the fade is done");

        h.player.command(PlayerCommand::Play).await.unwrap();
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
    }

    /// When each buffer reached the output, and its peak level.
    type Heard = Arc<std::sync::Mutex<Vec<(std::time::Instant, f64)>>>;

    /// A fake output that records what it renders and when.
    fn listening_output() -> (Output, Heard) {
        use gstreamer::prelude::*;
        let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = gstreamer::ElementFactory::make("fakesink")
            .name("matrixplayer-output")
            .property("sync", true)
            .property("signal-handoffs", true)
            .build()
            .unwrap();
        let log = heard.clone();
        sink.connect("handoff", false, move |args| {
            let buffer = args[1].get::<gstreamer::Buffer>().ok()?;
            let map = buffer.map_readable().ok()?;
            let peak = map
                .as_slice()
                .chunks_exact(8)
                .map(|b| f64::from_ne_bytes(b.try_into().unwrap()).abs())
                .fold(0.0, f64::max);
            log.lock().unwrap().push((std::time::Instant::now(), peak));
            None
        });
        let output = Output { sink, backend: crate::audio::OutputBackend::Auto, device: None };
        (output, heard)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resuming_after_a_soft_pause_is_heard_at_once() {
        gstreamer::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (output, heard) = listening_output();
        let settings = PlayerSettings {
            volume: 1.0,
            muted: false,
            repeat: RepeatMode::Off,
            shuffle: false,
            gapless: true,
            output_device: None,
        };
        let (player, service) = PlayerHandle::new(Database::in_memory().await.unwrap(), settings, output).unwrap();
        tokio::spawn(service);
        player.dsp().apply(&crate::dsp::settings::DspSettings::default());
        let mut events = player.subscribe();
        let artwork = ArtworkStore::new(dir.path().join("art")).unwrap();
        let a = dir.path().join("a.flac");
        write_flac(&a, 6.0, "Long");
        let tracks = vec![Arc::new(TrackInfo::load(&a, &artwork))];
        player.command(PlayerCommand::Insert { tracks, mode: InsertMode::Replace, start: 0 }).await.unwrap();
        wait_for(&mut events, is_status(PlaybackStatus::Playing)).await;
        tokio::time::sleep(Duration::from_millis(800)).await;

        let steady = heard.lock().unwrap().iter().map(|(_, p)| *p).fold(0.0, f64::max);
        player.command(PlayerCommand::Pause).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1600)).await;
        {
            // The fade must have been heard before the output stopped: the
            // last buffer out is the tail of the fade, not full-level music
            // cut off (with nothing but silence left waiting behind it).
            let heard = heard.lock().unwrap();
            let last = heard.last().expect("something was rendered").1;
            assert!(last < steady * 0.25, "the output stopped at {last:.3} of {steady:.3}: the fade was never heard");
        }

        let resumed = std::time::Instant::now();
        player.command(PlayerCommand::Play).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let heard = heard.lock().unwrap();
        let after: Vec<_> = heard.iter().filter(|(t, _)| *t > resumed).collect();
        let first = after.iter().position(|(_, p)| *p > 0.05).expect("the music comes back after resuming");
        let delay = after[first].0 - resumed;
        assert!(delay < Duration::from_millis(350), "music came back after {delay:?}");
        // Once back, it keeps playing: no stretch of silence behind it.
        let mut quiet_since = None;
        for (t, p) in &after[first..] {
            if *p > 0.05 {
                quiet_since = None;
            } else {
                let since = *quiet_since.get_or_insert(*t);
                assert!(*t - since < Duration::from_millis(120), "silent for {:?} after resuming", *t - since);
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn switching_outputs_resumes_at_the_same_position() {
        let mut h = harness().await;
        let a = h.dir.path().join("a.flac");
        write_flac(&a, 5.0, "Long");
        h.open(vec![a]).await;
        wait_for(&mut h.events, |e| matches!(e, PlayerEvent::Position { position_ms, .. } if *position_ms >= 1500))
            .await;

        h.player.command(PlayerCommand::SetOutput(Box::new(Output::fake(true)))).await.unwrap();
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        assert!(!statuses(&events).contains(&PlaybackStatus::Paused), "{:?}", statuses(&events));
        let position = h.player.snapshot().await.unwrap().position_ms;
        assert!(position >= 1400, "resumed at {position} ms");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn without_gapless_the_next_track_is_loaded_after_the_end() {
        let mut h = harness().await;
        h.player.command(PlayerCommand::SetGapless(false)).await.unwrap();
        let (a, b) = (h.dir.path().join("a.flac"), h.dir.path().join("b.flac"));
        write_flac(&a, 0.4, "First");
        write_flac(&b, 0.4, "Second");
        h.open(vec![a, b]).await;
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Stopped)).await;
        assert_eq!(track_titles(&events), ["First", "Second"]);
        let loads = statuses(&events).iter().filter(|s| **s == PlaybackStatus::Loading).count();
        assert_eq!(loads, 2, "each track is opened on its own");
        assert!(!h.player.snapshot().await.unwrap().queue.gapless);
    }
}
