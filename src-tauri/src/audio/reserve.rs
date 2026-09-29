//! Borrowing a sound card from the sound server for exclusive playback.
//!
//! PipeWire's session manager (like PulseAudio and JACK before it) holds
//! every card it drives as the D-Bus name
//! `org.freedesktop.ReserveDevice1.Audio<card>` and gives the card up when
//! another program asks through the device reservation protocol:
//!
//! 1. request the name, without queueing and allowing replacement;
//! 2. if someone holds it, call `RequestRelease(priority)` on them; a true
//!    answer means they have closed the device;
//! 3. take the name over (`ReplaceExisting`).
//!
//! While MatrixPlayer holds the name it answers such requests itself: no
//! while it is playing, and while paused or stopped it closes the device
//! and says yes. Dropping a [`Reservation`] closes its own bus connection,
//! which frees the name; the sound server then takes the card back. The bus
//! frees it too if the app exits or crashes.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use zbus::fdo::{RequestNameFlags, RequestNameReply};

use crate::error::{AppError, AppResult};

/// Our priority in the protocol: above the sound server's (WirePlumber
/// yields to anything above −20).
pub const PRIORITY: i32 = 10;
const INTERFACE: &str = "org.freedesktop.ReserveDevice1";

/// Closes the device when another program asks for it; resolves to whether
/// it did. Called only while nothing is playing.
pub type ReleaseFn = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// A card held for exclusive use, until dropped.
pub struct Reservation {
    /// A connection of its own: closing it frees the name.
    _bus: zbus::Connection,
    card: u32,
    playing: Arc<AtomicBool>,
}

impl std::fmt::Debug for Reservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reservation").field("card", &self.card).finish()
    }
}

fn names(card: u32) -> (String, String) {
    (format!("org.freedesktop.ReserveDevice1.Audio{card}"), format!("/org/freedesktop/ReserveDevice1/Audio{card}"))
}

impl Reservation {
    /// Borrows ALSA card `card` on `bus` (a connection used for nothing
    /// else). `device_name` is what other programs are told holds it;
    /// `on_release` closes the device if another program asks while
    /// nothing plays.
    pub async fn acquire(
        bus: zbus::Connection,
        card: u32,
        device_name: &str,
        on_release: ReleaseFn,
    ) -> AppResult<Self> {
        let (name, path) = names(card);
        let playing = Arc::new(AtomicBool::new(false));
        let owner = Owner { device_name: device_name.to_owned(), playing: playing.clone(), on_release };
        bus.object_server().at(path.as_str(), owner).await.map_err(dbus_error)?;

        let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::AllowReplacement;
        if !owns(bus.request_name_with_flags(name.as_str(), flags).await)? {
            // Held: ask the holder to let go, then take the name over.
            let released = bus
                .call_method(Some(name.as_str()), path.as_str(), Some(INTERFACE), "RequestRelease", &(PRIORITY,))
                .await
                .ok()
                .and_then(|reply| reply.body().deserialize::<bool>().ok())
                .unwrap_or(false);
            if !released {
                let holder = holder_name(&bus, &name, &path).await;
                return Err(AppError::Audio(match holder {
                    Some(app) => format!("{app} is using {device_name} and did not let it go"),
                    None => format!("{device_name} is in use by another program"),
                }));
            }
            if !owns(bus.request_name_with_flags(name.as_str(), flags | RequestNameFlags::ReplaceExisting).await)? {
                return Err(AppError::Audio(format!("{device_name} could not be taken over")));
            }
        }
        log::info!("holding sound card {card} ({device_name}) for exclusive playback");
        Ok(Self { _bus: bus, card, playing })
    }

    /// Whether audio is playing: the card is not handed over meanwhile.
    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::SeqCst);
    }
}

/// The name of the program holding `name`, if it says.
async fn holder_name(bus: &zbus::Connection, name: &str, path: &str) -> Option<String> {
    let proxy = zbus::Proxy::new(bus, name.to_owned(), path.to_owned(), INTERFACE).await.ok()?;
    proxy.get_property::<String>("ApplicationName").await.ok().filter(|n| !n.is_empty())
}

/// Whether a name request made us the owner; `false` when someone else
/// holds the name (zbus reports that as an error).
fn owns(reply: zbus::Result<RequestNameReply>) -> AppResult<bool> {
    match reply {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(true),
        Ok(_) | Err(zbus::Error::NameTaken) => Ok(false),
        Err(e) => Err(dbus_error(e)),
    }
}

fn dbus_error(e: zbus::Error) -> AppError {
    AppError::Audio(format!("the session bus is not available: {e}"))
}

/// Our side of the protocol, served while the card is held.
struct Owner {
    device_name: String,
    playing: Arc<AtomicBool>,
    on_release: ReleaseFn,
}

#[zbus::interface(name = "org.freedesktop.ReserveDevice1")]
impl Owner {
    /// Another program wants the card: refused while playing; otherwise
    /// the device is closed first, then the answer is yes.
    async fn request_release(&self, priority: i32) -> bool {
        if self.playing.load(Ordering::SeqCst) {
            log::info!("refusing to hand {} over (priority {priority}) while playing", self.device_name);
            return false;
        }
        log::info!("handing {} over (priority {priority})", self.device_name);
        (self.on_release)().await
    }

    #[zbus(property)]
    fn priority(&self) -> i32 {
        PRIORITY
    }

    #[zbus(property)]
    fn application_name(&self) -> String {
        "MatrixPlayer".into()
    }

    #[zbus(property)]
    fn application_device_name(&self) -> String {
        self.device_name.clone()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::AtomicUsize;

    /// A private session bus for the test, so the real one (and the real
    /// sound server's cards) are never touched. `None` without dbus-daemon.
    pub(crate) struct TestBus {
        daemon: Child,
        pub address: String,
    }

    impl TestBus {
        pub(crate) fn start() -> Option<Self> {
            let mut daemon = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address=1"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let mut address = String::new();
            BufReader::new(daemon.stdout.take()?).read_line(&mut address).ok()?;
            Some(Self { daemon, address: address.trim().to_owned() })
        }

        pub(crate) async fn connect(&self) -> zbus::Connection {
            zbus::connection::Builder::address(self.address.as_str()).unwrap().build().await.unwrap()
        }
    }

    impl Drop for TestBus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    /// Stands in for the sound server: holds card `card`, letting it go
    /// (or not) when asked, and counting the requests.
    struct Server {
        lets_go: bool,
        asked: Arc<AtomicUsize>,
    }

    #[zbus::interface(name = "org.freedesktop.ReserveDevice1")]
    impl Server {
        async fn request_release(&self, _priority: i32) -> bool {
            self.asked.fetch_add(1, Ordering::SeqCst);
            self.lets_go
        }

        #[zbus(property)]
        fn application_name(&self) -> String {
            "Sound Server".into()
        }
    }

    async fn sound_server(bus: &TestBus, card: u32, lets_go: bool) -> (zbus::Connection, Arc<AtomicUsize>) {
        let conn = bus.connect().await;
        let asked = Arc::new(AtomicUsize::new(0));
        let (name, path) = names(card);
        conn.object_server().at(path.as_str(), Server { lets_go, asked: asked.clone() }).await.unwrap();
        let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::AllowReplacement;
        assert_eq!(conn.request_name_with_flags(name.as_str(), flags).await.unwrap(), RequestNameReply::PrimaryOwner);
        (conn, asked)
    }

    fn released(counter: &Arc<AtomicUsize>) -> ReleaseFn {
        let counter = counter.clone();
        Arc::new(move || {
            let counter = counter.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                true
            })
        })
    }

    async fn owner_of(bus: &TestBus, card: u32) -> Option<String> {
        let conn = bus.connect().await;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
        let name = zbus::names::BusName::try_from(names(card).0).unwrap();
        dbus.get_name_owner(name).await.ok().map(|o| o.to_string())
    }

    #[tokio::test]
    async fn takes_the_card_over_from_the_sound_server_and_gives_it_back() {
        let Some(bus) = TestBus::start() else { return eprintln!("skipping: no dbus-daemon") };
        let (server, asked) = sound_server(&bus, 7, true).await;
        let server_id = server.unique_name().unwrap().to_string();

        let calls = Arc::new(AtomicUsize::new(0));
        let ours = bus.connect().await;
        let our_id = ours.unique_name().unwrap().to_string();
        let reservation = Reservation::acquire(ours, 7, "Test DAC", released(&calls)).await.unwrap();
        assert_eq!(asked.load(Ordering::SeqCst), 1, "the server was asked to let go");
        assert_eq!(owner_of(&bus, 7).await.as_deref(), Some(our_id.as_str()));

        drop(reservation);
        // The name is free again, for the server to take back.
        for _ in 0..50 {
            if owner_of(&bus, 7).await.is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(owner_of(&bus, 7).await, None);
        assert_ne!(server_id, our_id);
    }

    #[tokio::test]
    async fn a_card_that_is_not_let_go_is_reported_busy() {
        let Some(bus) = TestBus::start() else { return eprintln!("skipping: no dbus-daemon") };
        let (_server, _) = sound_server(&bus, 3, false).await;
        let calls = Arc::new(AtomicUsize::new(0));
        let err = Reservation::acquire(bus.connect().await, 3, "Test DAC", released(&calls)).await.unwrap_err();
        assert!(err.to_string().contains("Sound Server is using Test DAC"), "{err}");
    }

    #[tokio::test]
    async fn keeps_the_card_while_playing_and_hands_it_over_when_not() {
        let Some(bus) = TestBus::start() else { return eprintln!("skipping: no dbus-daemon") };
        let calls = Arc::new(AtomicUsize::new(0));
        let reservation = Reservation::acquire(bus.connect().await, 4, "Test DAC", released(&calls)).await.unwrap();

        // Another program asks, as JACK would.
        let other = bus.connect().await;
        let (name, path) = names(4);
        let ask = || async {
            other
                .call_method(Some(name.as_str()), path.as_str(), Some(INTERFACE), "RequestRelease", &(20i32,))
                .await
                .unwrap()
                .body()
                .deserialize::<bool>()
                .unwrap()
        };
        reservation.set_playing(true);
        assert!(!ask().await, "refused while playing");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        reservation.set_playing(false);
        assert!(ask().await, "handed over while paused");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the device was closed first");
    }
}
