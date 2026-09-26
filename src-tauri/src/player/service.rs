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
use crate::error::{AppError, AppResult};
use crate::filesystem;

/// "Previous" restarts the current track instead once it has played this long.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);
/// How often a changed volume is written to the database.
const PERSIST_INTERVAL: Duration = Duration::from_secs(2);

/// Persisted player preferences, restored at startup.
#[derive(Debug, Clone)]
pub struct PlayerSettings {
    pub volume: f64,
    pub muted: bool,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

impl PlayerSettings {
    pub async fn load(db: &Database) -> Self {
        Self {
            volume: settings::get(db, "player.volume").await.ok().flatten().unwrap_or(0.8),
            muted: settings::get(db, "player.muted").await.ok().flatten().unwrap_or(false),
            repeat: settings::get(db, "player.repeat").await.ok().flatten().unwrap_or_default(),
            shuffle: settings::get(db, "player.shuffle").await.ok().flatten().unwrap_or(false),
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
    ClearQueue,
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
        let (requests, requests_rx) = mpsc::channel(64);
        let (events, _) = broadcast::channel(512);
        let player = Player::new(engine, db, events.clone(), settings);
        Ok((Self { requests, events }, player.run(requests_rx, engine_rx)))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PlayerEvent> {
        self.events.subscribe()
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

struct Player {
    engine: AudioEngine,
    db: Database,
    events: broadcast::Sender<PlayerEvent>,
    queue: Queue,

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

    error: Option<PlayerError>,
    /// Failures since the last track that played, to stop skipping through
    /// a queue in which nothing is playable.
    failures: usize,
    decoded: Option<StreamFormat>,
    output: Option<StreamFormat>,
}

impl Player {
    fn new(
        engine: AudioEngine,
        db: Database,
        events: broadcast::Sender<PlayerEvent>,
        settings: PlayerSettings,
    ) -> Self {
        let mut queue = Queue::default();
        queue.set_repeat(settings.repeat);
        queue.set_shuffle(settings.shuffle);
        let player = Self {
            engine,
            db,
            events,
            queue,
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
            error: None,
            failures: 0,
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
                _ = persist.tick() => self.persist_volume().await,
            }
        }
        self.persist_volume().await;
    }

    // ── commands ────────────────────────────────────────────────────────────

    async fn handle(&mut self, command: PlayerCommand) -> AppResult<()> {
        match command {
            PlayerCommand::Play => self.play(),
            PlayerCommand::Pause => self.pause(),
            PlayerCommand::TogglePlayPause => {
                if self.intent == Intent::Play {
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
                self.update_next();
                self.emit_queue();
                settings::set(&self.db, "player.shuffle", &shuffle).await
            }
            PlayerCommand::Insert { tracks, mode, start } => self.insert(tracks, mode, start),
            PlayerCommand::PlayEntry(id) => {
                self.queue.jump_to(id).ok_or_else(|| AppError::InvalidArgument("no such queue entry".into()))?;
                self.failures = 0;
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
        let unloaded = !self.loading && matches!(self.engine_state, EngineState::Ready | EngineState::Null);
        if self.intent == Intent::Stop || unloaded {
            self.failures = 0;
            return self.load_current(true);
        }
        self.intent = Intent::Play;
        self.engine.play()?;
        self.refresh_status();
        Ok(())
    }

    fn pause(&mut self) -> AppResult<()> {
        if self.intent != Intent::Play {
            return Ok(());
        }
        self.intent = Intent::Pause;
        self.engine.pause()?;
        self.refresh_status();
        Ok(())
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
        let Some(track) = self.queue.current().map(|e| e.track.clone()) else {
            return Ok(());
        };
        let uri = filesystem::path_to_uri(&track.path)?;
        self.intent = if play { Intent::Play } else { Intent::Pause };
        self.loading = true;
        self.seeking = false;
        self.position = Duration::ZERO;
        self.duration = known_duration(&track);
        self.decoded = None;
        self.output = None;

        let loaded = self.engine.load(&uri, self.next_uri(), play);
        self.engine_state = EngineState::Ready;
        self.emit_track();
        self.emit_queue_state();
        self.emit_position();
        self.emit_signal();
        if let Err(err) = loaded {
            self.on_error(EngineError {
                kind: EngineErrorKind::Other,
                message: err.to_string(),
                detail: None,
                uri: Some(uri),
            });
            return Err(err);
        }
        self.refresh_status();
        Ok(())
    }

    /// The stream to continue with gaplessly after the current one.
    fn next_uri(&self) -> Option<String> {
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
                }
                if state == EngineState::Playing {
                    self.failures = 0;
                }
                self.refresh_status();
            }
            EngineEvent::StreamStarted { uri, gapless: true } => self.on_gapless_switch(&uri),
            EngineEvent::StreamStarted { .. } => {}
            EngineEvent::AsyncDone => {
                if self.seeking {
                    self.seeking = false;
                    if let Some(position) = self.engine.position() {
                        self.position = position;
                    }
                    self.emit_position();
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
        let error = PlayerError {
            kind: err.kind,
            message: err.message,
            detail: err.detail,
            file_name: err.uri.as_deref().and_then(file_name_from_uri),
        };
        self.emit(PlayerEvent::Error(error.clone()));
        self.error = Some(error);
        self.loading = false;
        self.seeking = false;
        if let Err(e) = self.engine.stop() {
            log::error!("could not stop after a playback error: {e}");
        }
        self.engine_state = EngineState::Ready;
        self.failures += 1;

        // Skip past a broken file, as long as some other file might play.
        let recoverable = err.kind != EngineErrorKind::OutputUnavailable;
        if recoverable
            && self.intent == Intent::Play
            && self.failures < self.queue.len()
            && self.queue.advance().is_some()
        {
            if let Err(e) = self.load_current(true) {
                log::error!("could not skip to the next track: {e}");
            }
            return;
        }
        self.intent = Intent::Stop;
        self.position = Duration::ZERO;
        self.emit_position();
        self.refresh_status();
    }

    // ── state & events ──────────────────────────────────────────────────────

    fn compute_status(&self) -> PlaybackStatus {
        if self.queue.current().is_none() {
            return if self.error.is_some() { PlaybackStatus::Error } else { PlaybackStatus::Idle };
        }
        match self.intent {
            Intent::Stop if self.error.is_some() => PlaybackStatus::Error,
            Intent::Stop => PlaybackStatus::Stopped,
            _ if self.loading => PlaybackStatus::Loading,
            _ if self.seeking => PlaybackStatus::Seeking,
            _ => match self.engine_state {
                EngineState::Playing => PlaybackStatus::Playing,
                EngineState::Paused => PlaybackStatus::Paused,
                EngineState::Ready | EngineState::Null => PlaybackStatus::Loading,
            },
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

    fn queue_snapshot(&self) -> QueueSnapshot {
        let (entries, current_index) = self.queue.play_order();
        QueueSnapshot { entries, current_index, shuffle: self.queue.shuffle(), repeat: self.queue.repeat() }
    }

    fn signal_path(&self) -> SignalPath {
        SignalPath { backend: self.engine.backend(), decoded: self.decoded.clone(), output: self.output.clone() }
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

    fn emit_track(&self) {
        self.emit(PlayerEvent::Track { track: self.queue.current().map(|e| e.track.clone()) });
    }

    /// The whole queue: only after its contents or order changed.
    fn emit_queue(&self) {
        self.emit(PlayerEvent::Queue(self.queue_snapshot()));
    }

    /// Cursor and modes only, which is all that changes on a track change.
    fn emit_queue_state(&self) {
        self.emit(PlayerEvent::QueueState(QueueState {
            current_index: self.queue.current_index(),
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
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
        let settings = PlayerSettings { volume: 0.8, muted: false, repeat: RepeatMode::Off, shuffle: false };
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
}
