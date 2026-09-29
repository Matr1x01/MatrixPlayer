//! Bit-perfect and exclusive playback: which output the player uses, and
//! holding the device's hardware while exclusive.
//!
//! - Bit-perfect (shared) keeps the sound server, but nothing in
//!   MatrixPlayer changes a sample: see `DspControl::set_bypass`.
//! - Exclusive borrows the device's card from the sound server (see
//!   `audio::reserve`) and plays straight to its hardware PCM, bit-perfect
//!   by definition. The card is refused to other programs while playing and
//!   handed over when paused or stopped; if the device goes away, or is
//!   handed over, playback moves back to the sound server.
//!
//! While a device changes hands the player plays into a silent stand-in, so
//! the old and the new output are never open at once; if it was playing it
//! pauses (with its soft fade) first and plays on after.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::Mutex;

use super::{PlaybackStatus, PlayerCommand, PlayerError, PlayerEvent, PlayerHandle};
use crate::audio::EngineErrorKind;
use crate::audio::output::{self, OutputDevice};
use crate::audio::reserve::{ReleaseFn, Reservation};
use crate::database::{Database, settings};
use crate::error::{AppError, AppResult};

pub const BIT_PERFECT_KEY: &str = "player.bit_perfect";
pub const EXCLUSIVE_KEY: &str = "player.exclusive";
use super::OUTPUT_DEVICE_KEY;

/// Long enough for a soft pause to be heard before the output changes.
const PAUSE_SETTLE: Duration = Duration::from_millis(300);
/// How long the sound server may take to bring a released card back.
const RETURN_TIMEOUT: Duration = Duration::from_secs(3);

struct Held {
    reservation: Reservation,
    device: OutputDevice,
}

pub struct OutputMode {
    player: PlayerHandle,
    db: Database,
    /// Bit-perfect playback through the sound server, as the user set it.
    bit_perfect: AtomicBool,
    /// The card held while exclusive.
    held: Mutex<Option<Held>>,
    /// One change of output at a time.
    changing: Mutex<()>,
}

impl OutputMode {
    /// Restores the saved mode: bit-perfect applies at once; exclusive
    /// access is requested in the background (the device may be busy).
    pub async fn start(player: PlayerHandle, db: Database) -> Arc<Self> {
        let bit_perfect = settings::get::<bool>(&db, BIT_PERFECT_KEY).await.ok().flatten().unwrap_or(false);
        let exclusive = settings::get::<bool>(&db, EXCLUSIVE_KEY).await.ok().flatten().unwrap_or(false);
        let device = settings::get::<Option<String>>(&db, OUTPUT_DEVICE_KEY).await.ok().flatten().flatten();
        player.dsp().set_bypass(bit_perfect);
        let mode = Arc::new(Self {
            player,
            db,
            bit_perfect: AtomicBool::new(bit_perfect),
            held: Mutex::new(None),
            changing: Mutex::new(()),
        });
        mode.follow_playback();
        if exclusive && let Some(id) = device {
            let mode = mode.clone();
            tokio::spawn(async move {
                if let Err(e) = mode.enable(&id).await {
                    log::warn!("could not restore exclusive access: {e}");
                    let _ = settings::set(&mode.db, EXCLUSIVE_KEY, &false).await;
                }
            });
        }
        mode
    }

    pub fn bit_perfect(&self) -> bool {
        self.bit_perfect.load(Ordering::SeqCst)
    }

    pub async fn exclusive(&self) -> bool {
        self.held.lock().await.is_some()
    }

    /// Bit-perfect playback through the sound server, on or off. Exclusive
    /// access is bit-perfect whatever this says.
    pub async fn set_bit_perfect(&self, on: bool) -> AppResult<()> {
        let _changing = self.changing.lock().await;
        self.bit_perfect.store(on, Ordering::SeqCst);
        settings::set(&self.db, BIT_PERFECT_KEY, &on).await?;
        if on {
            tokio::task::spawn_blocking(output::refresh_pipewire_clock).await.ok();
        }
        let exclusive = self.held.lock().await.is_some();
        self.player.dsp().set_bypass(on || exclusive);
        self.player.command(PlayerCommand::Reopen).await
    }

    /// Exclusive access to the selected device, on or off.
    pub async fn set_exclusive(self: &Arc<Self>, on: bool) -> AppResult<()> {
        let id = settings::get::<Option<String>>(&self.db, OUTPUT_DEVICE_KEY).await?.flatten();
        match (on, id) {
            (true, Some(id)) => self.enable(&id).await,
            (true, None) => Err(AppError::InvalidArgument("choose a device to use exclusively".into())),
            (false, id) => self.disable(id).await,
        }
    }

    /// Plays through `id`, or the system default with `None`. While
    /// exclusive, exclusive access moves to the new device if it can have
    /// it, and ends otherwise.
    pub async fn select_device(self: &Arc<Self>, id: Option<String>) -> AppResult<()> {
        if self.exclusive().await {
            if let Some(id) = &id
                && self.find(id).await.is_some_and(|d| d.exclusive)
            {
                return self.enable(id).await;
            }
            return self.disable(id).await;
        }
        let _changing = self.changing.lock().await;
        let wanted = id.clone();
        let out = tokio::task::spawn_blocking(move || output::create(wanted.as_deref()))
            .await
            .map_err(|e| AppError::Audio(e.to_string()))??;
        if id.is_some() && out.device.is_none() {
            return Err(AppError::Audio("That output device is no longer available".into()));
        }
        self.player.command(PlayerCommand::SetOutput(Box::new(out))).await?;
        settings::set(&self.db, OUTPUT_DEVICE_KEY, &id).await
    }

    async fn enable(self: &Arc<Self>, id: &str) -> AppResult<()> {
        let _changing = self.changing.lock().await;
        if self.held.lock().await.as_ref().is_some_and(|h| h.device.id == id) {
            return Ok(());
        }
        let device =
            self.find(id).await.ok_or_else(|| AppError::Audio("That output device is not available".into()))?;
        let pcm = device.pcm.ok_or_else(|| AppError::Audio(format!("{} cannot be used exclusively", device.name)))?;

        let playing = self.detach().await?;
        // Another device held until now goes back first.
        drop(self.held.lock().await.take());
        let bus = zbus::Connection::session()
            .await
            .map_err(|e| AppError::Audio(format!("the session bus is not available: {e}")));
        let reservation = match bus {
            Ok(bus) => Reservation::acquire(bus, pcm.card, &device.name, self.release_fn()).await,
            Err(e) => Err(e),
        };
        let opened = match reservation {
            Ok(reservation) => {
                let target = device.clone();
                match tokio::task::spawn_blocking(move || output::open_exclusive(&target)).await {
                    Ok(Ok(out)) => Ok((reservation, out)),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(AppError::Audio(e.to_string())),
                }
            }
            Err(e) => Err(e),
        };
        let (reservation, out) = match opened {
            Ok(opened) => opened,
            Err(e) => {
                // Back to the sound server, on this device, and saved so.
                self.attach_shared(Some(id.to_owned()), playing).await?;
                settings::set(&self.db, OUTPUT_DEVICE_KEY, &Some(id.to_owned())).await?;
                settings::set(&self.db, EXCLUSIVE_KEY, &false).await?;
                return Err(e);
            }
        };
        self.player.dsp().set_bypass(true);
        self.player.command(PlayerCommand::SetOutput(Box::new(out))).await?;
        // Held before playing, so the card is refused from the first sample.
        *self.held.lock().await = Some(Held { reservation, device });
        if playing {
            self.player.command(PlayerCommand::Play).await?;
        }
        settings::set(&self.db, OUTPUT_DEVICE_KEY, &Some(id.to_owned())).await?;
        settings::set(&self.db, EXCLUSIVE_KEY, &true).await
    }

    /// Ends exclusive access: the card goes back to the sound server, and
    /// playback follows it there, on device `to` (or the default).
    async fn disable(&self, to: Option<String>) -> AppResult<()> {
        let _changing = self.changing.lock().await;
        let Some(held) = self.held.lock().await.take() else {
            return Ok(());
        };
        let playing = self.detach().await?;
        drop(held);
        if let Some(id) = &to {
            self.wait_for_return(id).await;
        }
        self.attach_shared(to.clone(), playing).await?;
        settings::set(&self.db, OUTPUT_DEVICE_KEY, &to).await?;
        settings::set(&self.db, EXCLUSIVE_KEY, &false).await
    }

    /// Pauses if playing (letting the fade be heard) and moves the player
    /// to the silent stand-in. Returns whether it was playing.
    async fn detach(&self) -> AppResult<bool> {
        let playing = matches!(
            self.player.snapshot().await?.status,
            PlaybackStatus::Playing | PlaybackStatus::Loading | PlaybackStatus::Seeking
        );
        if playing {
            self.player.command(PlayerCommand::Pause).await?;
            tokio::time::sleep(PAUSE_SETTLE).await;
        }
        self.player.command(PlayerCommand::SetOutput(Box::new(output::silent()))).await?;
        Ok(playing)
    }

    /// Back on the sound server, on `device` (or the default), playing on if
    /// it was.
    async fn attach_shared(&self, device: Option<String>, play: bool) -> AppResult<()> {
        self.player.dsp().set_bypass(self.bit_perfect());
        let out = tokio::task::spawn_blocking(move || output::create(device.as_deref()))
            .await
            .map_err(|e| AppError::Audio(e.to_string()))??;
        self.player.command(PlayerCommand::SetOutput(Box::new(out))).await?;
        if play {
            self.player.command(PlayerCommand::Play).await?;
        }
        Ok(())
    }

    async fn find(&self, id: &str) -> Option<OutputDevice> {
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || output::find_device(output::shared_backend(), &id)).await.ok().flatten()
    }

    /// Waits until the sound server lists device `id` again.
    async fn wait_for_return(&self, id: &str) {
        let deadline = tokio::time::Instant::now() + RETURN_TIMEOUT;
        while tokio::time::Instant::now() < deadline {
            if self.find(id).await.is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        log::warn!("{id} did not come back to the sound server in time");
    }

    /// What to do when another program asks for the card (only asked while
    /// nothing plays): close it, say yes, then move to the system output.
    fn release_fn(self: &Arc<Self>) -> ReleaseFn {
        let weak: Weak<Self> = Arc::downgrade(self);
        Arc::new(move || {
            let weak = weak.clone();
            Box::pin(async move {
                let Some(mode) = weak.upgrade() else { return true };
                // Busy changing outputs: better to say no than to race.
                let Ok(changing) = mode.changing.try_lock() else { return false };
                if mode.player.command(PlayerCommand::SetOutput(Box::new(output::silent()))).await.is_err() {
                    return false;
                }
                drop(changing);
                tokio::spawn(async move { mode.handed_over().await });
                true
            })
        })
    }

    /// After handing the card over: forget it, and play on through the
    /// system's default output (the device now belongs to someone else).
    async fn handed_over(&self) {
        // Let the other program take the name before it is let go.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _changing = self.changing.lock().await;
        let Some(held) = self.held.lock().await.take() else { return };
        let name = held.device.name.clone();
        drop(held);
        if let Err(e) = self.attach_shared(None, false).await {
            log::error!("could not return to the system output: {e}");
        }
        let _ = settings::set(&self.db, EXCLUSIVE_KEY, &false).await;
        let notice = PlayerError {
            kind: EngineErrorKind::OutputUnavailable,
            message: format!("{name} was handed to another program; MatrixPlayer uses the system output now"),
            detail: None,
            file_name: None,
        };
        let _ = self.player.command(PlayerCommand::Notice(notice)).await;
    }

    /// Keeps the card's "playing" flag current (it decides whether the card
    /// may be handed over), and gives the card up if the device fails.
    fn follow_playback(self: &Arc<Self>) {
        let mut events = self.player.subscribe();
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                let Some(mode) = weak.upgrade() else { break };
                match event {
                    PlayerEvent::Status { status, .. } => {
                        let busy = matches!(
                            status,
                            PlaybackStatus::Playing | PlaybackStatus::Loading | PlaybackStatus::Seeking
                        );
                        if let Some(held) = mode.held.lock().await.as_ref() {
                            held.reservation.set_playing(busy);
                        }
                    }
                    // Unplugged, or gone: give it back and fall back.
                    PlayerEvent::Error(e) if e.kind == EngineErrorKind::OutputUnavailable && mode.exclusive().await => {
                        log::warn!("the exclusive device failed: {}", e.message);
                        if let Err(e) = mode.disable(None).await {
                            log::error!("could not return to the system output: {e}");
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::output::Output;
    use crate::player::{PlayerSettings, RepeatMode};

    async fn start(db: &Database) -> Arc<OutputMode> {
        gstreamer::init().unwrap();
        let settings = PlayerSettings {
            volume: 0.8,
            muted: false,
            repeat: RepeatMode::Off,
            shuffle: false,
            gapless: true,
            output_device: None,
        };
        let (player, service) = PlayerHandle::new(db.clone(), settings, Output::fake(true)).unwrap();
        tokio::spawn(service);
        OutputMode::start(player, db.clone()).await
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bit_perfect_is_saved_and_restored() {
        let db = Database::in_memory().await.unwrap();
        let mode = start(&db).await;
        assert!(!mode.bit_perfect() && !mode.player.dsp().bypassed());

        mode.set_bit_perfect(true).await.unwrap();
        assert!(mode.player.dsp().bypassed());
        let restarted = start(&db).await;
        assert!(restarted.bit_perfect() && restarted.player.dsp().bypassed());
        assert!(restarted.player.snapshot().await.unwrap().volume.locked);

        restarted.set_bit_perfect(false).await.unwrap();
        assert!(!restarted.player.dsp().bypassed());
        assert!(!restarted.player.snapshot().await.unwrap().volume.locked);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn exclusive_access_needs_a_chosen_device() {
        let db = Database::in_memory().await.unwrap();
        let mode = start(&db).await;
        let err = mode.set_exclusive(true).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument(_)), "{err}");
        mode.set_exclusive(false).await.unwrap();
        assert!(!mode.exclusive().await && !mode.player.dsp().bypassed());
    }
}
