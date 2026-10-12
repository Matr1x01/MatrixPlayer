//! The player service: one task that owns the audio engine and the queue.
//!
//! Commands from the UI and events from the engine are handled on the same
//! task, one at a time, so playback state has exactly one writer. Callers talk
//! to it through a cloneable [`PlayerHandle`].

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};

use super::bit_perfect::{Mode, verdict};
use super::queue::Queue;
use super::{
    EntryId, InsertMode, PlaybackStatus, PlayerError, PlayerEvent, PlayerSnapshot, QueueSnapshot, QueueState,
    RepeatMode, SignalPath, TrackInfo, VolumeState,
};
use crate::audio::output::Output;
use crate::audio::{
    AudioEngine, EngineError, EngineErrorKind, EngineEvent, EngineMessage, EngineState, OutputBackend, StreamFormat,
};
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
/// Fade before a seek, a skip or a stop: short, so the command still feels
/// immediate, but it never cuts the waveform off mid-cycle.
const SKIP_FADE_S: f64 = 0.04;
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
    /// Reopens the current track where it is, so an output setting that is
    /// fixed when the format is negotiated (dither) takes effect. Only done
    /// when the output takes integer samples: float outputs never dither.
    RenegotiateOutput,
    /// Reopens the current track where it is, after bit-perfect playback
    /// was switched on or off (the stream's layout is negotiated anew).
    Reopen,
    /// Tells the user about something that happened to the output (another
    /// program took the device), without changing playback.
    Notice(PlayerError),
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
    /// Puts back the queue as it was before the last time it was replaced
    /// or cleared, at the same track and position.
    UndoQueue,
}

/// Messages the service sends itself from timers.
#[derive(Debug)]
enum Internal {
    /// A fade to silence has reached the speakers.
    FadedOut(u64),
}

/// What a fade to silence leads to, once the output has played it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterFade {
    Pause,
    Seek(Duration),
    /// Open the current entry, which the queue and the UI have already
    /// moved to. It plays if the intent is still to play by then.
    Load,
    Stop,
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

/// The queue before it was replaced or cleared, for [`PlayerCommand::UndoQueue`].
struct Undo {
    queue: Queue,
    position: Duration,
    intent: Intent,
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
    /// What the fade to silence under way is for.
    after_fade: Option<AfterFade>,

    error: Option<PlayerError>,
    /// Failures since the last track that played, to stop skipping through
    /// a queue in which nothing is playable.
    failures: usize,
    direction: Direction,
    undo: Option<Undo>,
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
            after_fade: None,
            error: None,
            failures: 0,
            direction: Direction::Forward,
            undo: None,
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
            PlayerCommand::RenegotiateOutput => self.renegotiate_output(),
            PlayerCommand::Reopen => {
                self.emit(PlayerEvent::Volume(self.volume_state()));
                self.emit_signal();
                match self.resume_point() {
                    Some(point) => self.resume_at(point),
                    None => Ok(()),
                }
            }
            PlayerCommand::Notice(error) => {
                self.emit(PlayerEvent::Error(error));
                Ok(())
            }
            PlayerCommand::Insert { tracks, mode, start } => self.insert(tracks, mode, start),
            PlayerCommand::PlayEntry(id) => {
                self.queue.jump_to(id).ok_or_else(|| AppError::InvalidArgument("no such queue entry".into()))?;
                self.failures = 0;
                self.direction = Direction::Forward;
                self.switch_track(true, None)
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
                self.save_undo();
                self.queue.insert(tracks, InsertMode::Replace, start);
                self.emit_queue();
                self.failures = 0;
                self.direction = Direction::Forward;
                let seek = (position > Duration::from_secs(1)).then_some((position, false));
                self.switch_track(false, seek)
            }
            PlayerCommand::ClearOthers => {
                self.queue.clear_others();
                self.emit_queue();
                self.update_next();
                Ok(())
            }
            PlayerCommand::ClearQueue => {
                self.save_undo();
                self.queue.clear();
                self.duration = None;
                self.error = None;
                self.emit_track();
                self.emit_queue();
                self.stop()
            }
            PlayerCommand::UndoQueue => self.undo(),
        }
    }

    /// Keeps the queue as it is now, so a replace or clear can be undone.
    fn save_undo(&mut self) {
        if self.queue.len() == 0 {
            return;
        }
        let position = self.heard_position();
        self.undo = Some(Undo { queue: self.queue.clone(), position, intent: self.intent });
    }

    fn undo(&mut self) -> AppResult<()> {
        let Undo { mut queue, position, intent } =
            self.undo.take().ok_or_else(|| AppError::InvalidArgument("there is nothing to undo".into()))?;
        // Modes are the user's current choice, not part of what is undone.
        queue.set_repeat(self.queue.repeat());
        queue.set_shuffle(self.queue.shuffle());
        self.queue = queue;
        self.emit_queue();
        self.failures = 0;
        self.direction = Direction::Forward;
        if self.queue.current().is_none() {
            return Ok(());
        }
        let resume_at = (position > Duration::from_secs(1)).then_some(position);
        match intent {
            Intent::Stop => {
                self.stop()?;
                self.switch_to_current()
            }
            Intent::Pause => self.switch_track(false, resume_at.map(|p| (p, false))),
            Intent::Play => match resume_at {
                Some(p) => self.switch_track(false, Some((p, true))),
                None => self.switch_track(true, None),
            },
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
        // A seek or a skip is fading out: it goes on to play.
        if matches!(self.after_fade, Some(AfterFade::Seek(_) | AfterFade::Load)) {
            self.intent = Intent::Play;
            self.refresh_status();
            return Ok(());
        }
        let unloaded = !self.loading && matches!(self.engine_state, EngineState::Ready | EngineState::Null);
        if self.intent == Intent::Stop || unloaded {
            self.failures = 0;
            return self.switch_track(true, None);
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
        let audible = self.audible();
        self.intent = Intent::Pause;
        if self.after_fade.is_some() {
            // Already fading out for a seek or a skip, which now ends paused.
        } else if self.soft_fades() && audible {
            self.fade_out_then(AfterFade::Pause);
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

    /// Whether the track is being heard right now: playing, and neither
    /// opening nor seeking.
    fn audible(&self) -> bool {
        self.intent == Intent::Play && self.engine_state == EngineState::Playing && !self.loading && !self.seeking
    }

    fn soft_fades(&self) -> bool {
        self.dsp.settings().soft_fades
    }

    /// Whether a seek, skip or stop the user asked for fades out first: when
    /// the track is being heard and soft fades are on, or when a fade to
    /// silence is already under way (the new action takes it over).
    fn fade_first(&self) -> bool {
        self.after_fade.is_some() || (self.audible() && self.soft_fades())
    }

    /// Fades to silence, then does `action` once the silence is what the
    /// output plays: processing runs ahead of the speakers by the output
    /// buffer. A fade already under way ends in `action` instead of starting
    /// over, so a burst of seeks or skips performs only the last one.
    fn fade_out_then(&mut self, action: AfterFade) {
        if self.after_fade.replace(action).is_some() {
            return;
        }
        let seconds = if action == AfterFade::Pause { FADE_S } else { SKIP_FADE_S };
        self.fade_token += 1;
        self.dsp.fade(0.0, seconds);
        let ahead = match self.engine.running_time() {
            Some(now) => Duration::from_nanos(self.dsp.processed_running_time().saturating_sub(now)),
            None => Duration::ZERO,
        };
        let wait = (Duration::from_secs_f64(seconds) + ahead + Duration::from_millis(20)).min(MAX_FADE_WAIT);
        let (token, internal) = (self.fade_token, self.internal.clone());
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            let _ = internal.send(Internal::FadedOut(token));
        });
    }

    /// Drops the fade under way and what it was for: a newer command took over.
    fn cancel_fade(&mut self) {
        self.fade_token += 1;
        self.after_fade = None;
    }

    fn on_internal(&mut self, message: Internal) {
        match message {
            Internal::FadedOut(token) if token == self.fade_token => self.finish_fade(),
            Internal::FadedOut(_) => {}
        }
    }

    /// Does what the fade to silence was for, now that it has been heard.
    fn finish_fade(&mut self) {
        let Some(action) = self.after_fade.take() else { return };
        self.fade_token += 1;
        match action {
            AfterFade::Pause => {
                if self.intent == Intent::Pause
                    && let Err(e) = self.engine.pause()
                {
                    log::error!("could not pause after fading out: {e}");
                }
            }
            AfterFade::Seek(position) => self.finish_seek(position),
            AfterFade::Load => {
                if let Err(err) = self.open_current(self.intent == Intent::Play) {
                    self.on_error(err);
                }
            }
            AfterFade::Stop => {
                if let Err(e) = self.engine.stop() {
                    log::error!("could not stop after fading out: {e}");
                }
                self.engine_state = EngineState::Ready;
            }
        }
        self.refresh_status();
    }

    /// The seek a fade was waiting for.
    fn finish_seek(&mut self, position: Duration) {
        if self.intent == Intent::Play {
            // The seek's flush restarts the fade from silence; this brings
            // the sound back up (the chain is at the fade-out's zero).
            self.dsp.fade(1.0, FADE_S);
        } else if let Err(e) = self.engine.pause() {
            log::error!("could not pause before seeking: {e}");
        }
        if let Err(e) = self.engine.seek(position) {
            log::debug!("could not seek after fading out: {e}");
            self.seeking = false;
            self.position = self.engine.position().unwrap_or(self.position);
            self.emit_position();
        }
    }

    /// Moves playback to another output, reopening the current track at the
    /// same position.
    fn set_output(&mut self, output: Output) -> AppResult<()> {
        let resume = self.resume_point();
        self.cancel_fade();
        self.engine.set_output(output)?;
        self.engine_state = EngineState::Ready;
        self.loading = false;
        self.seeking = false;
        self.decoded = None;
        self.output = None;
        self.emit_signal();
        match resume {
            Some(point) => self.resume_at(point),
            None => {
                self.refresh_status();
                Ok(())
            }
        }
    }

    fn renegotiate_output(&mut self) -> AppResult<()> {
        let integer = self.output.as_ref().is_some_and(|f| !f.float);
        match self.resume_point() {
            Some(point) if integer => self.resume_at(point),
            _ => Ok(()),
        }
    }

    /// Where to pick up after the pipeline is rebuilt (position, and whether
    /// to play); `None` when nothing is loaded.
    fn resume_point(&self) -> Option<(Duration, bool)> {
        let active = self.queue.current().is_some() && self.intent != Intent::Stop && self.error.is_none();
        active.then_some((self.heard_position(), self.intent == Intent::Play))
    }

    /// The position in the current track: the engine's, unless a track is
    /// opening (perhaps to seek to a position once open) or a seek is under
    /// way (it may still be fading out, with the engine at the old
    /// position), when it is the one asked for.
    fn heard_position(&self) -> Duration {
        if let Some((position, _)) = self.pending_seek {
            return position;
        }
        if self.loading || self.seeking { self.position } else { self.engine.position().unwrap_or(self.position) }
    }

    /// Reopens the current track at a point from [`Self::resume_point`].
    fn resume_at(&mut self, (position, play): (Duration, bool)) -> AppResult<()> {
        self.failures = 0;
        if position > Duration::from_secs(1) {
            // Open paused and seek first, so the start of the track is
            // never heard.
            return self.switch_track(false, Some((position, play)));
        }
        self.switch_track(play, None)
    }

    /// Stops for the user: a track being heard fades out first.
    fn stop(&mut self) -> AppResult<()> {
        if !self.fade_first() {
            return self.stop_now();
        }
        self.fade_out_then(AfterFade::Stop);
        self.show_stopped();
        Ok(())
    }

    /// Stops right away (the queue ran out, or nothing is audible).
    fn stop_now(&mut self) -> AppResult<()> {
        self.cancel_fade();
        self.engine.stop()?;
        self.engine_state = EngineState::Ready;
        self.show_stopped();
        Ok(())
    }

    fn show_stopped(&mut self) {
        self.intent = Intent::Stop;
        self.loading = false;
        self.seeking = false;
        self.pending_seek = None;
        self.play_after_seek = false;
        self.position = Duration::ZERO;
        self.emit_position();
        self.refresh_status();
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
        // Stopped (perhaps still fading out) counts as not loaded.
        let loaded = self.intent != Intent::Stop
            && !self.loading
            && matches!(self.engine_state, EngineState::Paused | EngineState::Playing);
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

    /// Seeks; while the track is being heard it fades out first. The UI
    /// shows the new position at once.
    fn seek(&mut self, position: Duration) -> AppResult<()> {
        let loaded = matches!(self.engine_state, EngineState::Paused | EngineState::Playing);
        if self.loading || !loaded || self.intent == Intent::Stop {
            return Err(AppError::InvalidArgument("nothing is playing".into()));
        }
        let position = self.duration.map_or(position, |d| position.min(d));
        if self.fade_first() {
            self.fade_out_then(AfterFade::Seek(position));
        } else {
            self.engine.seek(position)?;
        }
        self.seeking = true;
        self.position = position;
        self.emit_position();
        self.refresh_status();
        Ok(())
    }

    fn insert(&mut self, tracks: Vec<Arc<TrackInfo>>, mode: InsertMode, start: usize) -> AppResult<()> {
        let had_current = self.queue.current().is_some();
        if mode == InsertMode::Replace && !tracks.is_empty() {
            self.save_undo();
        }
        self.queue.insert(tracks, mode, start);
        self.emit_queue();
        if mode == InsertMode::Replace || !had_current {
            self.failures = 0;
            self.direction = Direction::Forward;
            return self.switch_track(true, None);
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
        self.duration = None;
        self.emit_track();
        self.stop()
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
            intent => self.switch_track(intent == Intent::Play, None),
        }
    }

    /// Opens the current entry for the user's command, as
    /// [`Self::load_current_at`] does. A track being heard fades out first;
    /// the UI shows the new one at once.
    fn switch_track(&mut self, play: bool, seek: Option<(Duration, bool)>) -> AppResult<()> {
        if self.queue.current().is_none() {
            return self.stop();
        }
        if !self.fade_first() {
            return self.load_current_at(play, seek);
        }
        self.begin_load(play, seek);
        self.refresh_status();
        self.fade_out_then(AfterFade::Load);
        Ok(())
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
        self.begin_load(play, seek);
        self.open_current(play)
    }

    /// Shows the current entry as the one opening: everything but the
    /// pipeline, which [`Self::open_current`] then opens.
    fn begin_load(&mut self, play: bool, seek: Option<(Duration, bool)>) {
        self.pending_seek = seek;
        self.play_after_seek = false;
        let Some(track) = self.queue.current().map(|e| e.track.clone()) else {
            return;
        };
        self.intent = if play { Intent::Play } else { Intent::Pause };
        self.loading = true;
        self.seeking = false;
        self.position = Duration::ZERO;
        self.duration = known_duration(&track);
        self.decoded = None;
        self.output = None;
        self.emit_track();
        self.emit_queue_state();
        self.emit_position();
        self.emit_signal();
    }

    /// Opens the current entry in the engine, playing if `play`.
    fn open_current(&mut self, play: bool) -> Result<(), EngineError> {
        let Some(track) = self.queue.current().map(|e| e.track.clone()) else {
            return Ok(());
        };
        // No fade back to full level here: the engine's reset tells the
        // processor to start the new stream from rest, at full level.
        self.cancel_fade();
        let uri = filesystem::path_to_uri(&track.path).map_err(|e| EngineError {
            kind: EngineErrorKind::NotFound,
            message: e.to_string(),
            detail: None,
            uri: None,
        })?;
        log::info!("opening {} ({}){}", track.file_name, describe(&track), if play { "" } else { ", paused" });
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
        loaded?;
        self.refresh_status();
        Ok(())
    }

    /// The stream to continue with gaplessly after the current one. Only one
    /// the output can take as it is: the output keeps the format it was
    /// opened with (see `audio`), so a track at another rate, or one whose
    /// format is not known, is opened on its own once the current one ends.
    fn next_uri(&self) -> Option<String> {
        if !self.gapless {
            return None;
        }
        let (current, next) = (self.queue.current()?, self.queue.peek_auto_next()?);
        let bit_perfect = self.dsp.bypassed();
        let (from, to) = (output_shape(&current.track, bit_perfect), output_shape(&next.track, bit_perfect));
        if from.is_none() || from != to {
            log::debug!(
                "not joining {} to {} gaplessly: {from:?} → {to:?}",
                current.track.file_name,
                next.track.file_name
            );
            return None;
        }
        filesystem::path_to_uri(&next.track.path).ok()
    }

    /// Tells the engine what to continue with after the queue changed.
    fn update_next(&self) {
        self.engine.set_next(self.next_uri());
    }

    // ── engine events ───────────────────────────────────────────────────────

    fn on_engine(&mut self, event: EngineEvent) {
        if self.after_fade.is_some_and(|a| a != AfterFade::Pause) && self.while_fading(&event) {
            return;
        }
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
            EngineEvent::FormatChanged { from, to, new_stream } => self.reopen_for_format(&from, &to, new_stream),
        }
    }

    /// The engine held back a stream the output could not take without
    /// reopening (see `audio`): opens it anew, on its own, where it was.
    /// Nothing of it has been heard, so there is nothing to fade out.
    fn reopen_for_format(&mut self, from: &StreamFormat, to: &StreamFormat, new_stream: bool) {
        let name = self.queue.current().map(|e| e.track.file_name.clone()).unwrap_or_default();
        log::warn!("reopening {name}: the output was open in {from}, the stream is in {to}");
        let play = self.wants_play();
        let position = if new_stream { Duration::ZERO } else { self.heard_position() };
        let seek = (position > Duration::from_secs(1)).then_some((position, play));
        self.failures = 0;
        if let Err(e) = self.load_current_at(play && seek.is_none(), seek) {
            log::error!("could not reopen {name}: {e}");
        }
    }

    /// Engine events arriving while a seek, skip or stop fades out. Returns
    /// whether the event was dealt with; the rest are handled as usual.
    fn while_fading(&mut self, event: &EngineEvent) -> bool {
        let seek = matches!(self.after_fade, Some(AfterFade::Seek(_)));
        match event {
            // The stream ran out before the fade did: nothing is audible any
            // more, so do what was waiting now.
            EngineEvent::EndOfStream => {
                self.finish_fade();
                true
            }
            // The stream being left broke: release it, then open or stop.
            EngineEvent::Error(_) if !seek => {
                self.engine.recover();
                self.engine_state = EngineState::Null;
                self.finish_fade();
                true
            }
            EngineEvent::Error(_) => {
                self.cancel_fade();
                false
            }
            // The next track began gaplessly: the seek was meant for the one
            // before it, so it is dropped (a pause asked for meanwhile stays).
            EngineEvent::StreamStarted { gapless: true, .. } if seek => {
                self.seeking = false;
                if self.intent == Intent::Play {
                    self.cancel_fade();
                    self.dsp.fade(1.0, FADE_S);
                } else {
                    self.after_fade = Some(AfterFade::Pause);
                }
                false
            }
            // The seek has not been issued yet: this is not its completion.
            EngineEvent::AsyncDone if seek => true,
            _ if seek => false,
            // Anything else describes the stream being left.
            _ => true,
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
        if let Some(track) = self.queue.current().map(|e| &e.track) {
            log::info!("continuing gaplessly with {} ({})", track.file_name, describe(track));
        }
        self.position = Duration::ZERO;
        self.duration = self.queue.current().and_then(|e| known_duration(&e.track));
        self.error = None;
        self.update_next();
        self.emit_track();
        self.emit_queue_state();
        self.emit_position();
        // It may have ended a seek that was still fading out.
        self.refresh_status();
    }

    /// End of stream is only reached when no gapless continuation was queued:
    /// gapless is off, the next track is in another format, or the queue
    /// changed after the continuation point.
    fn on_end_of_stream(&mut self) {
        if self.queue.peek_auto_next().is_some() {
            // A soft pause still fading out when the track ended stays paused.
            self.queue.advance_auto();
            if let Err(e) = self.load_current(self.intent == Intent::Play) {
                log::error!("could not continue after end of stream: {e}");
            }
            return;
        }
        log::info!("end of the queue");
        if let Err(e) = self.stop_now() {
            log::error!("could not stop at end of queue: {e}");
        }
    }

    fn on_error(&mut self, err: EngineError) {
        self.cancel_fade();
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
                Direction::Backward => {
                    self.queue.retreat().is_some() || {
                        self.direction = Direction::Forward;
                        self.queue.advance().is_some()
                    }
                }
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
        log::warn!(
            "could not play {}: {} ({:?}; {})",
            error.file_name.as_deref().unwrap_or("a track"),
            error.message,
            error.kind,
            error.detail.as_deref().unwrap_or("no details"),
        );
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
        let locked = self.dsp.bypassed();
        let gain_db = if locked {
            (!self.muted).then_some(0.0)
        } else {
            (!self.muted && self.volume > 0.0).then(|| 60.0 * self.volume.log10())
        };
        VolumeState { level: self.volume, muted: self.muted, gain_db, locked }
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
        let exclusive = self.engine.exclusive();
        let bit_perfect = exclusive || self.dsp.bypassed();
        let mode = match (exclusive, bit_perfect) {
            (true, _) => Mode::Exclusive,
            (false, true) => Mode::Shared,
            (false, false) => Mode::Normal,
        };
        // Only PipeWire's own clock matters for a shared stream.
        let pipewire_clock =
            (self.engine.backend() == OutputBackend::PipeWire).then(crate::audio::output::pipewire_clock).flatten();
        SignalPath {
            backend: self.engine.backend(),
            device: self.engine.device().cloned(),
            buffer_ms: self.engine.buffer_ms(),
            decoded: self.decoded.clone(),
            output: self.output.clone(),
            exclusive,
            bit_perfect,
            verdict: verdict(mode, self.decoded.as_ref(), self.output.as_ref(), pipewire_clock.as_ref()),
            pipewire_clock,
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

/// The sample rate and channel count `track` reaches the output with, as
/// far as its properties tell: mono is upmixed to stereo unless nothing may
/// change the samples, and Opus always decodes at 48 kHz (its header holds
/// the rate it was encoded from).
fn output_shape(track: &TrackInfo, bit_perfect: bool) -> Option<(u32, u8)> {
    let p = &track.properties;
    let rate = if p.codec == "Opus" { 48_000 } else { p.sample_rate? };
    let channels = if bit_perfect { p.channels? } else { p.channels?.max(2) };
    Some((rate, channels))
}

/// "FLAC, 96000 Hz, 2 ch", for the log.
fn describe(track: &TrackInfo) -> String {
    let p = &track.properties;
    let known = |v: Option<String>| v.unwrap_or_else(|| "?".into());
    format!(
        "{}, {} Hz, {} ch",
        p.codec,
        known(p.sample_rate.map(|r| r.to_string())),
        known(p.channels.map(|c| c.to_string()))
    )
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
        let (a, locked, c) =
            (h.dir.path().join("a.flac"), h.dir.path().join("locked.flac"), h.dir.path().join("c.flac"));
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
    async fn replacing_the_queue_can_be_undone() {
        let mut h = harness().await;
        let (a, b, c) = (h.dir.path().join("a.flac"), h.dir.path().join("b.flac"), h.dir.path().join("c.flac"));
        write_flac(&a, 4.0, "First");
        write_flac(&b, 4.0, "Second");
        write_flac(&c, 4.0, "Other");
        h.open(vec![a, b]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        h.open(vec![c]).await;
        wait_for(&mut h.events, |e| matches!(e, PlayerEvent::Track { track: Some(t) } if t.metadata.title == "Other"))
            .await;
        h.player.command(PlayerCommand::UndoQueue).await.unwrap();
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        let snapshot = h.player.snapshot().await.unwrap();
        let titles: Vec<_> = snapshot.queue.entries.iter().map(|e| e.track.metadata.title.as_str()).collect();
        assert_eq!(titles, ["First", "Second"]);
        assert_eq!(snapshot.track.unwrap().metadata.title, "First");
        assert!(h.player.command(PlayerCommand::UndoQueue).await.is_err(), "undo is used up");

        h.player.command(PlayerCommand::ClearQueue).await.unwrap();
        assert_eq!(h.player.snapshot().await.unwrap().queue.entries.len(), 0);
        h.player.command(PlayerCommand::UndoQueue).await.unwrap();
        assert_eq!(h.player.snapshot().await.unwrap().queue.entries.len(), 2, "a cleared queue comes back");
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
        let output = Output { sink, backend: crate::audio::OutputBackend::Auto, device: None, exclusive: false };
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

    /// A buffer a recording output rendered.
    #[derive(Debug, Clone, Copy)]
    struct Buffer {
        peak: f64,
        /// Peak of its last 2 ms: where a fade inside the buffer ended up.
        tail: f64,
        /// Stream time (ms).
        pts: u64,
    }

    /// What a recording output rendered, and when; `None` marks a flush.
    type Rendered = Arc<std::sync::Mutex<Vec<(std::time::Instant, Option<Buffer>)>>>;

    /// A fake output that records each buffer it renders, and each flush.
    fn recording_output() -> (Output, Rendered) {
        use gstreamer::prelude::*;
        let log: Rendered = Arc::default();
        let sink = gstreamer::ElementFactory::make("fakesink")
            .name("matrixplayer-output")
            .property("sync", true)
            .property("signal-handoffs", true)
            .build()
            .unwrap();
        let rendered = log.clone();
        sink.connect("handoff", false, move |args| {
            let buffer = args[1].get::<gstreamer::Buffer>().ok()?;
            let pts = buffer.pts().map_or(0, |t| t.mseconds());
            let map = buffer.map_readable().ok()?;
            let samples: Vec<f64> =
                map.as_slice().chunks_exact(8).map(|b| f64::from_ne_bytes(b.try_into().unwrap()).abs()).collect();
            let peak = samples.iter().copied().fold(0.0, f64::max);
            // 2 ms of stereo 44.1 kHz.
            let tail = samples[samples.len().saturating_sub(176)..].iter().copied().fold(0.0, f64::max);
            rendered.lock().unwrap().push((std::time::Instant::now(), Some(Buffer { peak, tail, pts })));
            None
        });
        let flushes = log.clone();
        sink.static_pad("sink").unwrap().add_probe(gstreamer::PadProbeType::EVENT_FLUSH, move |_, info| {
            if let Some(gstreamer::EventView::FlushStop(_)) = info.event().map(|e| e.view()) {
                flushes.lock().unwrap().push((std::time::Instant::now(), None));
            }
            gstreamer::PadProbeReturn::Ok
        });
        (Output { sink, backend: crate::audio::OutputBackend::Auto, device: None, exclusive: false }, log)
    }

    /// A player on a recording output, with the default DSP settings (soft
    /// fades on) and the volume at 100%, playing a 6 s tone.
    async fn recording_player() -> (PlayerHandle, broadcast::Receiver<PlayerEvent>, Rendered, tempfile::TempDir) {
        gstreamer::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (output, rendered) = recording_output();
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
        let (a, b) = (dir.path().join("a.flac"), dir.path().join("b.flac"));
        write_flac(&a, 6.0, "First");
        write_flac(&b, 6.0, "Second");
        let tracks = [a, b].iter().map(|p| Arc::new(TrackInfo::load(p, &artwork))).collect();
        player.command(PlayerCommand::Insert { tracks, mode: InsertMode::Replace, start: 0 }).await.unwrap();
        wait_for(&mut events, is_status(PlaybackStatus::Playing)).await;
        tokio::time::sleep(Duration::from_millis(800)).await;
        (player, events, rendered, dir)
    }

    fn steady_level(rendered: &Rendered) -> f64 {
        rendered.lock().unwrap().iter().filter_map(|(_, r)| r.map(|b| b.peak)).fold(0.0, f64::max)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_seek_fades_out_then_comes_back_at_full_level() {
        let (player, mut events, rendered, _dir) = recording_player().await;
        let steady = steady_level(&rendered);
        let before = rendered.lock().unwrap().len();
        player.command(PlayerCommand::Seek(Duration::from_secs(3))).await.unwrap();
        assert_eq!(player.snapshot().await.unwrap().position_ms, 3000, "the new position shows at once");
        wait_for(&mut events, is_status(PlaybackStatus::Playing)).await;
        tokio::time::sleep(Duration::from_millis(600)).await;

        let log = rendered.lock().unwrap();
        let after = &log[before..];
        let flush = after.iter().position(|(_, r)| r.is_none()).expect("the seek flushed the output");
        assert_eq!(after.iter().filter(|(_, r)| r.is_none()).count(), 1);
        let last = after[..flush].iter().rev().find_map(|(_, r)| *r).unwrap();
        assert!(last.tail < steady * 0.01, "the output was cut at {:.3} of {steady:.3}", last.tail);
        let resumed: Vec<_> = after[flush..].iter().filter_map(|(t, r)| r.map(|b| (*t, b))).collect();
        assert!(resumed[0].1.pts >= 2990, "plays from the new position: {} ms", resumed[0].1.pts);
        assert!(resumed[0].1.peak < steady * 0.9, "fades in rather than starting at full level");
        let back = resumed.iter().position(|(_, b)| b.peak > steady * 0.99).expect("back at full level");
        assert!(resumed[back].0 - after[flush].0 < Duration::from_millis(400), "full level again after the fade-in");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_burst_of_seeks_performs_only_the_last() {
        let (player, mut events, rendered, _dir) = recording_player().await;
        let before = rendered.lock().unwrap().len();
        for ms in [1500, 2500, 3500, 4000] {
            player.command(PlayerCommand::Seek(Duration::from_millis(ms))).await.unwrap();
        }
        let statuses_seen = statuses(&wait_for(&mut events, is_status(PlaybackStatus::Playing)).await);
        assert_eq!(statuses_seen, [PlaybackStatus::Seeking, PlaybackStatus::Playing]);
        tokio::time::sleep(Duration::from_millis(300)).await;

        let log = rendered.lock().unwrap();
        let after = &log[before..];
        assert_eq!(after.iter().filter(|(_, r)| r.is_none()).count(), 1, "one flush for the whole burst");
        let flush = after.iter().position(|(_, r)| r.is_none()).unwrap();
        let first_pts = after[flush..].iter().find_map(|(_, r)| r.map(|b| b.pts)).unwrap();
        assert!((3990..4100).contains(&first_pts), "played from the last target: {first_pts} ms");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_seek_still_fading_out_survives_an_output_switch() {
        let mut h = harness().await;
        let a = h.dir.path().join("a.flac");
        write_flac(&a, 5.0, "Long");
        h.open(vec![a]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        h.player.command(PlayerCommand::Seek(Duration::from_secs(3))).await.unwrap();
        h.player.command(PlayerCommand::SetOutput(Box::new(Output::fake(true)))).await.unwrap();
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        let position = h.player.snapshot().await.unwrap().position_ms;
        assert!(position >= 2900, "reopened at {position} ms, not where the seek was going");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn previous_while_a_stop_fades_out_is_not_an_error() {
        let mut h = harness().await;
        let a = h.dir.path().join("a.flac");
        write_flac(&a, 5.0, "Only");
        h.open(vec![a]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        h.player.command(PlayerCommand::Stop).await.unwrap();
        h.player.command(PlayerCommand::Previous).await.expect("nothing to go back to is not an error");
        assert_eq!(h.player.snapshot().await.unwrap().status, PlaybackStatus::Stopped);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn skipping_fades_out_the_track_being_left() {
        let (player, mut events, rendered, _dir) = recording_player().await;
        let steady = steady_level(&rendered);
        let before = rendered.lock().unwrap().len();
        player.command(PlayerCommand::Next).await.unwrap();
        assert_eq!(player.snapshot().await.unwrap().track.unwrap().metadata.title, "Second", "shown at once");
        wait_for(&mut events, is_status(PlaybackStatus::Playing)).await;
        tokio::time::sleep(Duration::from_millis(600)).await;

        let log = rendered.lock().unwrap();
        let played: Vec<_> = log[before..].iter().filter_map(|(_, r)| *r).collect();
        // The second track starts over at stream time zero.
        let start = played.windows(2).position(|w| w[1].pts < w[0].pts).expect("the next track played") + 1;
        assert!(played[start - 1].tail < steady * 0.01, "the first track was cut at {:.3}", played[start - 1].tail);
        assert!(played[start..].iter().any(|b| b.peak > steady * 0.99), "the next track plays at full level");
    }

    /// An output that takes only 16-bit samples, like many ALSA devices.
    fn integer_output() -> Output {
        use gstreamer::prelude::*;
        let bin = gstreamer::parse::bin_from_description(
            "capsfilter caps=audio/x-raw,format=S16LE ! fakesink sync=true",
            true,
        )
        .unwrap();
        Output { sink: bin.upcast(), backend: crate::audio::OutputBackend::Auto, device: None, exclusive: false }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn renegotiating_reopens_integer_outputs_where_they_were() {
        let mut h = harness().await;
        let a = h.dir.path().join("a.flac");
        write_flac(&a, 5.0, "Long");
        h.open(vec![a]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;

        // A float output never dithers: nothing to reopen.
        h.player.command(PlayerCommand::RenegotiateOutput).await.unwrap();
        assert_eq!(h.player.snapshot().await.unwrap().status, PlaybackStatus::Playing);

        h.player.command(PlayerCommand::SetOutput(Box::new(integer_output()))).await.unwrap();
        wait_for(&mut h.events, |e| matches!(e, PlayerEvent::Position { position_ms, .. } if *position_ms >= 1500))
            .await;
        h.player.command(PlayerCommand::RenegotiateOutput).await.unwrap();
        let events = wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        assert!(statuses(&events).contains(&PlaybackStatus::Loading), "reopened: {:?}", statuses(&events));
        let position = h.player.snapshot().await.unwrap().position_ms;
        assert!(position >= 1400, "resumed at {position} ms");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bit_perfect_playback_locks_the_volume_and_reports_the_path() {
        use super::super::bit_perfect::BitPerfect;
        let mut h = harness().await;
        let a = h.dir.path().join("a.flac");
        write_flac(&a, 4.0, "Long");
        h.open(vec![a]).await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        let normal = h.player.snapshot().await.unwrap();
        assert!(!normal.volume.locked && normal.signal.verdict == BitPerfect::Off);

        h.player.dsp().set_bypass(true);
        h.player.command(PlayerCommand::Reopen).await.unwrap();
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        let snapshot = h.player.snapshot().await.unwrap();
        assert!(snapshot.volume.locked, "the volume is fixed at 100%");
        assert_eq!(snapshot.volume.gain_db, Some(0.0));
        assert!((snapshot.volume.level - 0.8).abs() < 1e-9, "the slider keeps its place for afterwards");
        assert!(snapshot.signal.bit_perfect && !snapshot.signal.exclusive);
        assert_eq!(snapshot.signal.verdict, BitPerfect::Yes { shared: true });
    }

    /// Each buffer a recording output rendered: its rate, frames and peak.
    type Formats = Arc<std::sync::Mutex<Vec<(u32, usize, f64)>>>;

    /// A fake output, in real time, that takes any format and records what
    /// it renders.
    fn format_recording_output() -> (Output, Formats) {
        use gstreamer::prelude::*;
        gstreamer::init().unwrap();
        let log: Formats = Arc::default();
        let sink = gstreamer::ElementFactory::make("fakesink")
            .name("matrixplayer-output")
            .property("sync", true)
            .property("signal-handoffs", true)
            .build()
            .unwrap();
        let rendered = log.clone();
        sink.connect("handoff", false, move |args| {
            let buffer = args[1].get::<gstreamer::Buffer>().ok()?;
            let pad = args[2].get::<gstreamer::Pad>().ok()?;
            let caps = pad.current_caps()?;
            let info = gstreamer_audio::AudioInfo::from_caps(&caps).ok()?;
            let map = buffer.map_readable().ok()?;
            let peak = map
                .as_slice()
                .chunks_exact(8)
                .map(|b| f64::from_ne_bytes(b.try_into().unwrap()).abs())
                .fold(0.0, f64::max);
            rendered.lock().unwrap().push((info.rate(), map.size() / info.bpf() as usize, peak));
            None
        });
        (Output { sink, backend: crate::audio::OutputBackend::Auto, device: None, exclusive: false }, log)
    }

    /// The rates the output played sound at, in order, and for how long (s).
    fn heard(rendered: &Formats) -> Vec<(u32, f64)> {
        let mut heard: Vec<(u32, f64)> = Vec::new();
        for &(rate, frames, _) in rendered.lock().unwrap().iter().filter(|(_, _, peak)| *peak > 0.05) {
            let seconds = frames as f64 / f64::from(rate);
            match heard.last_mut() {
                Some((r, s)) if *r == rate => *s += seconds,
                _ => heard.push((rate, seconds)),
            }
        }
        heard
    }

    /// A player at full volume on `output`, and its events.
    async fn player_on(output: Output) -> (PlayerHandle, broadcast::Receiver<PlayerEvent>) {
        gstreamer::init().unwrap();
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
        let events = player.subscribe();
        (player, events)
    }

    /// Writes 0.5 s of the tone at each rate, named in order.
    fn tracks_at(dir: &std::path::Path, rates: &[u32], artwork: &ArtworkStore) -> Vec<Arc<TrackInfo>> {
        rates
            .iter()
            .enumerate()
            .map(|(i, rate)| {
                let path = dir.join(format!("{}.flac", char::from(b'a' + i as u8)));
                crate::test_support::write_flac_format(&path, 0.5, *rate, 2);
                Arc::new(TrackInfo::load(&path, artwork))
            })
            .collect()
    }

    /// The silent-track bug: a gapless join to another sample rate left the
    /// output on the old format, and the track after it played in silence
    /// (or at the wrong pitch). Each change of rate is now opened on its
    /// own; tracks at the same rate still join without a gap.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_queue_of_mixed_sample_rates_plays_every_track_in_full() {
        let (output, rendered) = format_recording_output();
        let (player, mut events) = player_on(output).await;
        let dir = tempfile::tempdir().unwrap();
        let artwork = ArtworkStore::new(dir.path().join("art")).unwrap();
        let tracks = tracks_at(dir.path(), &[44_100, 44_100, 48_000, 96_000, 44_100], &artwork);
        player.command(PlayerCommand::Insert { tracks, mode: InsertMode::Replace, start: 0 }).await.unwrap();
        let events = wait_for(&mut events, is_status(PlaybackStatus::Stopped)).await;

        assert_eq!(track_titles(&events), ["a", "b", "c", "d", "e"]);
        assert!(errors(&events).is_empty(), "{:#?}", errors(&events));
        let opened = statuses(&events).iter().filter(|s| **s == PlaybackStatus::Loading).count();
        assert_eq!(opened, 4, "a and b join; c, d and e are opened on their own");
        let heard = heard(&rendered);
        let rates: Vec<u32> = heard.iter().map(|(r, _)| *r).collect();
        assert_eq!(rates, [44_100, 48_000, 96_000, 44_100], "{heard:?}");
        // Each fixture is 0.51 s long (whole 1024-frame blocks).
        for ((rate, seconds), tracks) in heard.iter().zip([2.0, 1.0, 1.0, 1.0]) {
            assert!((seconds - 0.51 * tracks).abs() < 0.03, "{rate} Hz heard for {seconds:.3} s");
        }
    }

    /// A track whose properties misstate its rate is joined gaplessly; the
    /// engine holds it back from the output, and it is opened anew and
    /// heard from its start.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_the_output_cannot_take_is_reopened_from_the_start() {
        let (output, rendered) = format_recording_output();
        let (player, mut events) = player_on(output).await;
        let dir = tempfile::tempdir().unwrap();
        let artwork = ArtworkStore::new(dir.path().join("art")).unwrap();
        let mut tracks = tracks_at(dir.path(), &[44_100, 48_000], &artwork);
        Arc::make_mut(&mut tracks[1]).properties.sample_rate = Some(44_100);
        player.command(PlayerCommand::Insert { tracks, mode: InsertMode::Replace, start: 0 }).await.unwrap();
        let events = wait_for(&mut events, is_status(PlaybackStatus::Stopped)).await;

        let mut titles = track_titles(&events);
        titles.dedup();
        assert_eq!(titles, ["a", "b"], "b is announced again as it is reopened");
        assert!(errors(&events).is_empty(), "{:#?}", errors(&events));
        let heard = heard(&rendered);
        assert_eq!(heard.iter().map(|(r, _)| *r).collect::<Vec<_>>(), [44_100, 48_000], "{heard:?}");
        assert!((heard[1].1 - 0.51).abs() < 0.03, "the reopened track is heard in full: {heard:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn skipping_to_a_track_at_another_rate_is_heard() {
        let (output, rendered) = format_recording_output();
        let (player, mut events) = player_on(output).await;
        let dir = tempfile::tempdir().unwrap();
        let artwork = ArtworkStore::new(dir.path().join("art")).unwrap();
        let (a, b) = (dir.path().join("a.flac"), dir.path().join("b.flac"));
        crate::test_support::write_flac_format(&a, 4.0, 44_100, 2);
        crate::test_support::write_flac_format(&b, 4.0, 96_000, 2);
        let tracks = [a, b].iter().map(|p| Arc::new(TrackInfo::load(p, &artwork))).collect();
        player.command(PlayerCommand::Insert { tracks, mode: InsertMode::Replace, start: 0 }).await.unwrap();
        wait_for(&mut events, is_status(PlaybackStatus::Playing)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        player.command(PlayerCommand::Next).await.unwrap();
        wait_for(&mut events, is_status(PlaybackStatus::Playing)).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let snapshot = player.snapshot().await.unwrap();
        assert_eq!(snapshot.signal.output.map(|f| f.sample_rate), Some(96_000));
        let heard = heard(&rendered);
        assert!(matches!(heard.last(), Some((96_000, s)) if *s > 0.3), "{heard:?}");
    }

    /// Switching bit-perfect playback on renegotiates a mono stream in
    /// place (it is no longer upmixed). The engine holds the new layout
    /// back and the player reopens the track: where it was, not from the
    /// start, whichever of that and the reopen asked for comes first.
    #[tokio::test(flavor = "multi_thread")]
    async fn bit_perfect_on_a_mono_track_reopens_it_where_it_was() {
        let mut h = harness().await;
        let a = h.dir.path().join("mono.flac");
        crate::test_support::write_flac_format(&a, 6.0, 44_100, 1);
        h.open(vec![a]).await;
        wait_for(&mut h.events, |e| matches!(e, PlayerEvent::Position { position_ms, .. } if *position_ms >= 2000))
            .await;

        h.player.dsp().set_bypass(true);
        h.player.command(PlayerCommand::Reopen).await.unwrap();
        wait_for(
            &mut h.events,
            |e| matches!(e, PlayerEvent::Signal(s) if s.output.as_ref().is_some_and(|f| f.channels == 1)),
        )
        .await;
        wait_for(&mut h.events, is_status(PlaybackStatus::Playing)).await;
        let position = h.player.snapshot().await.unwrap().position_ms;
        assert!(position >= 1900, "reopened at {position} ms");
    }

    #[test]
    fn tracks_join_gaplessly_only_in_the_same_output_format() {
        gstreamer::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let artwork = ArtworkStore::new(dir.path().join("art")).unwrap();
        let load = |name: &str, rate, channels| {
            let path = dir.path().join(name);
            crate::test_support::write_flac_format(&path, 0.1, rate, channels);
            TrackInfo::load(&path, &artwork)
        };
        let (stereo, mono) = (load("stereo.flac", 44_100, 2), load("mono.flac", 44_100, 1));
        assert_eq!(output_shape(&stereo, false), Some((44_100, 2)));
        assert_eq!(output_shape(&mono, false), Some((44_100, 2)), "mono is upmixed: it joins stereo");
        assert_eq!(output_shape(&mono, true), Some((44_100, 1)), "but not in bit-perfect playback");
        assert_eq!(output_shape(&load("hi.flac", 96_000, 2), false), Some((96_000, 2)));

        let mut opus = stereo.clone();
        (opus.properties.codec, opus.properties.sample_rate) = ("Opus".into(), Some(44_100));
        assert_eq!(output_shape(&opus, false), Some((48_000, 2)), "Opus decodes at 48 kHz");
        let mut unknown = stereo;
        unknown.properties.sample_rate = None;
        assert_eq!(output_shape(&unknown, false), None);
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
