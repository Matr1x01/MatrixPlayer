//! PipeWire: the device monitor, on a connection of its own, and noticing
//! when the PipeWire daemon restarts.
//!
//! GStreamer's PipeWire elements share one connection per process (one
//! `pw_core` per file descriptor, -1 for the default socket), refcounted and
//! kept while anything holds it. When the daemon goes away nothing drops it:
//! a stream on it just goes quiet, without an error, and each new sink joins
//! the same dead connection for as long as one holder remains. MatrixPlayer
//! used to keep its device monitor on that connection for good, so after a
//! PipeWire restart (`systemctl --user restart pipewire`, a desktop's
//! "restart audio" key) nothing played again until it was restarted.
//!
//! So the monitor gets a connection of its own (its `fd` property), and
//! sinks are made directly, on the default connection, never from the
//! monitor's devices (which would hand them the monitor's). The monitor's
//! socket also tells when the daemon goes away, by hanging up: a thread
//! then waits for PipeWire to come back, restarts the monitor on a new
//! connection, and calls the restart hook, so the player rebuilds its
//! output. Taking the old sink down releases the dead connection, and the
//! new sink opens a live one.
//!
//! Older plugins (PipeWire before 0.3.64, as in Ubuntu 22.04) have neither
//! the monitor's `fd` nor the sink's `target-object`; MatrixPlayer then
//! plays through PipeWire's PulseAudio service instead (see
//! `output::create`).

use std::ffi::OsString;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex, Once, OnceLock};
use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;

const PROVIDER: &str = "pipewiredeviceprovider";
/// How long a device listing waits for the monitor to connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How often to look for PipeWire again after it went away (and how long
/// to leave it first: a daemon that is shutting down may still accept a
/// connection).
const RETRY: Duration = Duration::from_secs(1);

struct Monitor {
    provider: gst::DeviceProvider,
    /// The monitor's own connection: its peer hanging up means the daemon
    /// is gone.
    socket: UnixStream,
}

enum State {
    /// Not looked for yet, or connecting.
    Connecting,
    Up(Monitor),
    /// Not running (or not answering).
    Down,
}

static STATE: Mutex<State> = Mutex::new(State::Connecting);
static CHANGED: Condvar = Condvar::new();
static WATCH: Once = Once::new();
type Hook = Box<dyn Fn() + Send + Sync>;
static RESTARTED: Mutex<Option<Hook>> = Mutex::new(None);

/// Whether the installed PipeWire plugin can keep the monitor apart from
/// the streams (PipeWire 0.3.64 and later).
pub fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let monitor = gst::DeviceProviderFactory::by_name(PROVIDER).is_some_and(|p| p.has_property("fd"));
        let sink = gst::ElementFactory::make("pipewiresink").build().is_ok_and(|s| s.has_property("target-object"));
        if !(monitor && sink) {
            log::info!("the PipeWire plugin is too old to use (before 0.3.64)");
        }
        monitor && sink
    })
}

/// PipeWire's devices, from the monitor; `None` when PipeWire is not
/// running, does not answer, or its plugin is too old. Waits for the
/// monitor to connect the first time. Blocking.
pub fn devices() -> Option<Vec<gst::Device>> {
    if !supported() {
        return None;
    }
    WATCH.call_once(|| {
        if let Err(e) = std::thread::Builder::new().name("pipewire-watch".into()).spawn(watch) {
            log::warn!("cannot watch PipeWire: {e}");
            *STATE.lock().unwrap() = State::Down;
        }
    });
    let state = STATE.lock().unwrap();
    let (state, _) = CHANGED.wait_timeout_while(state, CONNECT_TIMEOUT, |s| matches!(s, State::Connecting)).unwrap();
    match &*state {
        State::Up(monitor) => Some(monitor.provider.devices().into_iter().collect()),
        _ => None,
    }
}

/// Calls `hook` (on the watching thread) each time PipeWire comes back
/// after going away, once the monitor lists its devices again.
pub fn on_restart(hook: impl Fn() + Send + Sync + 'static) {
    *RESTARTED.lock().unwrap() = Some(Box::new(hook));
}

/// A sink on PipeWire's default connection, playing to the node named
/// `target` (by name, which survives the device or PipeWire coming back),
/// or to the default output.
pub fn sink(name: &str, target: Option<&str>) -> Option<gst::Element> {
    let sink = gst::ElementFactory::make("pipewiresink").name(name).build().ok()?;
    if let Some(target) = target {
        sink.set_property("target-object", target);
    }
    Some(sink)
}

fn set(state: State) {
    *STATE.lock().unwrap() = state;
    CHANGED.notify_all();
}

/// Keeps the monitor connected: connects it, waits for the daemon to go
/// away, then for it to come back, and says so.
fn watch() {
    let mut seen = false;
    loop {
        let monitor = match start() {
            Ok(monitor) => monitor,
            Err(e) if !seen => {
                // Never there: nothing to watch for.
                log::info!("PipeWire is not available: {e}");
                return set(State::Down);
            }
            Err(_) => {
                std::thread::sleep(RETRY);
                continue;
            }
        };
        let hangup = match monitor.socket.try_clone() {
            Ok(socket) => socket,
            Err(e) => {
                log::warn!("cannot watch PipeWire: {e}");
                return set(State::Up(monitor));
            }
        };
        set(State::Up(monitor));
        if seen {
            log::info!("PipeWire is back");
            if let Some(hook) = RESTARTED.lock().unwrap().as_ref() {
                hook();
            }
        }
        seen = true;

        wait_for_hangup(&hangup, -1);
        log::warn!("PipeWire went away; waiting for it to come back");
        let old = std::mem::replace(&mut *STATE.lock().unwrap(), State::Down);
        if let State::Up(monitor) = old {
            monitor.provider.stop();
        }
        std::thread::sleep(RETRY);
    }
}

/// Connects a socket of its own to PipeWire and starts the monitor on it.
fn start() -> io::Result<Monitor> {
    let path = socket_path(|name| std::env::var_os(name))
        .ok_or_else(|| io::Error::other("no runtime directory to find its socket in"))?;
    let socket = UnixStream::connect(&path)?;
    let provider = gst::DeviceProviderFactory::by_name(PROVIDER)
        .ok_or_else(|| io::Error::other("its GStreamer device monitor is not installed"))?;
    // Nobody reads the hotplug messages; the device list is what matters.
    provider.bus().set_sync_handler(|_, _| gst::BusSyncReply::Drop);
    provider.set_property("fd", socket.as_raw_fd());
    provider.start().map_err(|e| io::Error::other(e.to_string()))?;
    // The monitor starts "successfully" even when the daemon drops the
    // connection: check that it is still there.
    if wait_for_hangup(&socket, 0) {
        provider.stop();
        return Err(io::Error::other("it closed the connection"));
    }
    Ok(Monitor { provider, socket })
}

/// The PipeWire socket, found as libpipewire finds it: `PIPEWIRE_REMOTE`
/// (default `pipewire-0`), in `PIPEWIRE_RUNTIME_DIR`, `XDG_RUNTIME_DIR` or
/// `USERPROFILE`, unless it is an absolute path.
fn socket_path(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |name: &str| var(name).filter(|v| !v.is_empty());
    let remote = PathBuf::from(set("PIPEWIRE_REMOTE").unwrap_or_else(|| "pipewire-0".into()));
    if remote.is_absolute() {
        return Some(remote);
    }
    let dir = ["PIPEWIRE_RUNTIME_DIR", "XDG_RUNTIME_DIR", "USERPROFILE"].into_iter().find_map(set)?;
    Some(PathBuf::from(dir).join(remote))
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    // From the C library, which std already links.
    fn poll(fds: *mut PollFd, nfds: std::ffi::c_ulong, timeout: i32) -> i32;
}

const POLLERR: i16 = 0x8;
const POLLHUP: i16 = 0x10;
const POLLRDHUP: i16 = 0x2000;

/// Waits up to `timeout_ms` (-1: forever) for the peer of `socket` to hang
/// up; true if it did. Only the hang-up is asked for: the socket's data
/// belongs to the monitor's own loop, which reads it from a copy.
fn wait_for_hangup(socket: &UnixStream, timeout_ms: i32) -> bool {
    let mut fd = PollFd { fd: socket.as_raw_fd(), events: POLLRDHUP, revents: 0 };
    loop {
        // SAFETY: one valid pollfd, alive for the duration of the call.
        let ready = unsafe { poll(&mut fd, 1, timeout_ms) };
        if ready > 0 {
            return fd.revents & (POLLRDHUP | POLLHUP | POLLERR) != 0;
        }
        if ready == 0 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn finds_the_socket_as_libpipewire_does() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |name: &str| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| OsString::from(*v))
        };
        assert_eq!(
            socket_path(env(&[("XDG_RUNTIME_DIR", "/run/user/1000")])),
            Some("/run/user/1000/pipewire-0".into())
        );
        assert_eq!(
            socket_path(env(&[("XDG_RUNTIME_DIR", "/run/user/1000"), ("PIPEWIRE_RUNTIME_DIR", "/tmp/pw")])),
            Some("/tmp/pw/pipewire-0".into())
        );
        assert_eq!(
            socket_path(env(&[("XDG_RUNTIME_DIR", "/run/user/1000"), ("PIPEWIRE_REMOTE", "pipewire-1")])),
            Some("/run/user/1000/pipewire-1".into())
        );
        assert_eq!(socket_path(env(&[("PIPEWIRE_REMOTE", "/srv/pw.sock")])), Some("/srv/pw.sock".into()));
        assert_eq!(socket_path(env(&[("XDG_RUNTIME_DIR", "")])), None);
    }

    #[test]
    fn notices_the_peer_hanging_up_but_not_its_data() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        assert!(!wait_for_hangup(&ours, 50), "still connected");
        theirs.write_all(b"a message for someone else").unwrap();
        assert!(!wait_for_hangup(&ours, 50), "data is not a hang-up");
        drop(theirs);
        assert!(wait_for_hangup(&ours, 1000));
    }
}
