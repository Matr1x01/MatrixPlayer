//! Listening history: turns what the player reports into play counts.
//!
//! A play counts once the listener has actually heard half of a track, or
//! four minutes of a long one (the rule scrobblers use). Only time that
//! passed while playing counts: seeking forward does not, so skimming through
//! a track is not a listen. The count is recorded the moment the threshold is
//! reached, so it survives the app being closed mid-track.

use tokio::sync::broadcast::{self, error::RecvError};

use super::{LibraryChange, LibraryEvent, LibraryHandle};
use crate::error::AppResult;
use crate::player::PlayerEvent;

const MAX_THRESHOLD_MS: u64 = 4 * 60 * 1000;
/// Position reports further apart than this are seeks, not playback.
const MAX_STEP_MS: u64 = 1500;

#[derive(Debug)]
struct Session {
    track_id: i64,
    threshold_ms: u64,
    listened_ms: u64,
    last_position: u64,
    counted: bool,
}

/// What the listener heard of the current track.
#[derive(Debug, Default)]
pub struct Listening {
    session: Option<Session>,
}

impl Listening {
    /// Feeds one player event. Returns a track id when it just earned a play.
    pub fn on_event(&mut self, event: &PlayerEvent) -> Option<(i64, u64)> {
        match event {
            PlayerEvent::Track { track } => {
                self.session = track.as_ref().and_then(|t| {
                    let duration = t.properties.duration_ms;
                    Some(Session {
                        track_id: t.library?.track_id,
                        threshold_ms: (duration / 2).clamp(1, MAX_THRESHOLD_MS),
                        listened_ms: 0,
                        last_position: 0,
                        counted: false,
                    })
                });
                None
            }
            PlayerEvent::Position { position_ms, .. } => {
                let session = self.session.as_mut()?;
                let step = position_ms.checked_sub(session.last_position);
                session.last_position = *position_ms;
                if let Some(step) = step.filter(|s| *s <= MAX_STEP_MS) {
                    session.listened_ms += step;
                }
                if !session.counted && session.listened_ms >= session.threshold_ms {
                    session.counted = true;
                    return Some((session.track_id, session.listened_ms));
                }
                None
            }
            _ => None,
        }
    }
}

/// Records plays for the lifetime of the player.
pub fn spawn(library: LibraryHandle, mut events: broadcast::Receiver<PlayerEvent>) {
    tauri::async_runtime::spawn(async move {
        let mut listening = Listening::default();
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            if let Some((track_id, listened_ms)) = listening.on_event(&event) {
                match record_play(&library, track_id, listened_ms).await {
                    Ok(()) => library.emit(LibraryEvent::Changed(LibraryChange::History)),
                    Err(e) => log::warn!("could not record a play: {e}"),
                }
            }
        }
    });
}

pub async fn record_play(library: &LibraryHandle, track_id: i64, listened_ms: u64) -> AppResult<()> {
    let mut tx = library.db().pool().begin().await?;
    sqlx::query("INSERT INTO play_history (track_id, played_at, listened_ms) VALUES (?, unixepoch(), ?)")
        .bind(track_id)
        .bind(i64::try_from(listened_ms).unwrap_or(i64::MAX))
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE tracks SET play_count = play_count + 1, last_played_at = unixepoch() WHERE id = ?")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::player::TrackInfo;

    fn track(id: i64, duration_ms: u64) -> PlayerEvent {
        let mut info = TrackInfo::for_test("t");
        info.library = Some(crate::player::LibraryRef { track_id: id, album_id: None, artist_id: None, folder_id: 1 });
        info.properties.duration_ms = duration_ms;
        PlayerEvent::Track { track: Some(Arc::new(info)) }
    }

    fn at(position_ms: u64) -> PlayerEvent {
        PlayerEvent::Position { position_ms, duration_ms: None }
    }

    #[test]
    fn counts_a_play_after_half_the_track_once() {
        let mut l = Listening::default();
        l.on_event(&track(7, 4000));
        let mut plays = Vec::new();
        for ms in (0..=4000).step_by(200) {
            plays.extend(l.on_event(&at(ms)));
        }
        assert_eq!(plays, [(7, 2000)]);
    }

    #[test]
    fn seeking_ahead_does_not_count_as_listening() {
        let mut l = Listening::default();
        l.on_event(&track(1, 10_000));
        assert_eq!(l.on_event(&at(200)), None);
        assert_eq!(l.on_event(&at(9_800)), None, "a jump is a seek");
        assert_eq!(l.on_event(&at(10_000)), None);
    }

    #[test]
    fn long_tracks_count_after_four_minutes_and_files_outside_the_library_never() {
        let mut l = Listening::default();
        l.on_event(&track(3, 60 * 60 * 1000));
        let mut plays = Vec::new();
        for ms in (0..=MAX_THRESHOLD_MS).step_by(1000) {
            plays.extend(l.on_event(&at(ms)));
        }
        assert_eq!(plays, [(3, MAX_THRESHOLD_MS)]);

        l.on_event(&PlayerEvent::Track { track: Some(Arc::new(TrackInfo::for_test("file"))) });
        assert_eq!(l.on_event(&at(1000)), None);
    }
}
