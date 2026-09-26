//! "Now playing" notifications through the desktop's notification server.
//!
//! One notification is kept and replaced on each track change, shown only
//! while MatrixPlayer's window is not focused (when it is, the player bar
//! already says what is playing), and only if the user wants them.

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tokio::sync::broadcast::error::RecvError;
use zbus::zvariant::Value;

use crate::artwork::ArtworkStore;
use crate::database::Database;
use crate::player::{PlaybackStatus, PlayerEvent, PlayerHandle, TrackInfo};

use super::mpris::DESKTOP_ENTRY;

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Body markup is a small HTML subset on servers that support it.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn body(track: &TrackInfo) -> String {
    let md = &track.metadata;
    let artist = md.artist.as_deref().unwrap_or("Unknown artist");
    match &md.album {
        Some(album) => format!("{} — {}", escape(artist), escape(album)),
        None => escape(artist),
    }
}

/// Shows (or replaces) the notification for `track`. Returns its id.
async fn announce(
    proxy: &NotificationsProxy<'_>,
    replaces: u32,
    track: &TrackInfo,
    image: Option<&str>,
) -> zbus::Result<u32> {
    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("desktop-entry", Value::from(DESKTOP_ENTRY));
    hints.insert("category", Value::from("x-gnome.music"));
    hints.insert("transient", Value::from(true));
    if let Some(path) = image {
        hints.insert("image-path", Value::from(path));
    }
    let icon = image.unwrap_or("matrixplayer");
    proxy.notify("MatrixPlayer", replaces, icon, &track.metadata.title, &body(track), &[], hints, 5000).await
}

pub fn spawn(app: AppHandle, player: PlayerHandle, artwork: Arc<ArtworkStore>, db: Database) {
    let mut events = player.subscribe();
    tauri::async_runtime::spawn(async move {
        let connection = match zbus::Connection::session().await {
            Ok(c) => c,
            Err(e) => {
                log::warn!("notifications are not available: {e}");
                return;
            }
        };
        let proxy = match NotificationsProxy::new(&connection).await {
            Ok(p) => p,
            Err(e) => {
                log::warn!("notifications are not available: {e}");
                return;
            }
        };
        let mut last_id = 0u32;
        let mut pending: Option<Arc<TrackInfo>> = None;
        let mut status = PlaybackStatus::Idle;
        loop {
            match events.recv().await {
                Ok(PlayerEvent::Track { track }) => pending = track,
                Ok(PlayerEvent::Status { status: s, .. }) => status = s,
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
            // Announce a new track once it is actually playing.
            if status != PlaybackStatus::Playing || pending.is_none() {
                continue;
            }
            let track = pending.take().expect("checked above");
            let wanted = crate::preferences::load(&db).await.map(|p| p.track_notifications).unwrap_or(true);
            let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
            if !wanted || focused {
                continue;
            }
            let image = match &track.artwork {
                Some(info) => {
                    let (store, id) = (artwork.clone(), info.id.clone());
                    tokio::task::spawn_blocking(move || store.thumbnail_for(&id)).await.ok().flatten()
                }
                None => None,
            };
            let image = image.map(|p| p.to_string_lossy().into_owned());
            match announce(&proxy, last_id, &track, image.as_deref()).await {
                Ok(id) => last_id = id,
                Err(e) => log::debug!("could not show a notification: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use zbus::zvariant::OwnedValue;

    struct Call {
        replaces_id: u32,
        icon: String,
        summary: String,
        body: String,
        hints: HashMap<String, OwnedValue>,
    }

    #[derive(Default)]
    struct FakeServer {
        calls: Arc<Mutex<Vec<Call>>>,
    }

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl FakeServer {
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            _app_name: String,
            replaces_id: u32,
            app_icon: String,
            summary: String,
            body: String,
            _actions: Vec<String>,
            hints: HashMap<String, OwnedValue>,
            _expire_timeout: i32,
        ) -> u32 {
            let mut calls = self.calls.lock().unwrap();
            calls.push(Call { replaces_id, icon: app_icon, summary, body, hints });
            calls.len() as u32 + 40
        }
    }

    #[tokio::test]
    async fn announces_tracks_and_replaces_the_previous_notification() {
        let (a, b) = tokio::net::UnixStream::pair().unwrap();
        let server = FakeServer::default();
        let calls = server.calls.clone();
        let guid = zbus::Guid::generate();
        let (server_conn, client_conn) = tokio::join!(
            zbus::connection::Builder::unix_stream(a)
                .server(guid)
                .unwrap()
                .p2p()
                .serve_at("/org/freedesktop/Notifications", server)
                .unwrap()
                .build(),
            zbus::connection::Builder::unix_stream(b).p2p().build(),
        );
        let (_server_conn, client_conn) = (server_conn.unwrap(), client_conn.unwrap());
        let proxy = NotificationsProxy::builder(&client_conn)
            .destination("org.freedesktop.Notifications")
            .unwrap()
            .build()
            .await
            .unwrap();

        let mut track = TrackInfo::for_test("Isobar");
        track.metadata.artist = Some("Halden Reach".into());
        let first = announce(&proxy, 0, &track, Some("/cache/art.t360.jpg")).await.unwrap();
        let second = announce(&proxy, first, &TrackInfo::for_test("Next"), None).await.unwrap();
        assert_ne!(first, second);

        let calls = calls.lock().unwrap();
        let Call { replaces_id, icon, summary, body, hints } = &calls[0];
        assert_eq!(
            (*replaces_id, icon.as_str(), summary.as_str(), body.as_str()),
            (0, "/cache/art.t360.jpg", "Isobar", "Halden Reach")
        );
        assert_eq!(
            hints.get("desktop-entry").and_then(|v| v.downcast_ref::<String>().ok()).as_deref(),
            Some(DESKTOP_ENTRY)
        );
        assert!(hints.contains_key("image-path"));
        assert_eq!(calls[1].replaces_id, first, "the second notification replaces the first");
        assert_eq!(calls[1].icon, "matrixplayer", "no artwork: the app icon");
    }

    #[test]
    fn escapes_markup_in_tags() {
        let mut track = TrackInfo::for_test("Title");
        track.metadata.artist = Some("Simon & <Garfunkel>".into());
        track.metadata.album = Some("Bridge".into());
        assert_eq!(body(&track), "Simon &amp; &lt;Garfunkel&gt; — Bridge");
    }
}
