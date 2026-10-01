//! What the log records about the system at startup: enough to see what
//! changed when MatrixPlayer worked before and does not now (a system
//! update, a different launcher, a stale instance still running).

use std::fs;
use std::path::Path;
use std::time::Duration;

/// Environment variables that change how the app, or the libraries under
/// it, behave: a name, or a prefix ending in `_`. An AppImage's launcher
/// sets several (`APPDIR`, `GST_PLUGIN_SYSTEM_PATH_1_0`, …).
const VARIABLES: [&str; 25] = [
    "APPIMAGE",
    "APPDIR",
    "ARGV0",
    "OWD",
    "GST_",
    "LD_",
    "SPA_",
    "PIPEWIRE_",
    "PULSE_",
    "WEBKIT_",
    "GDK_",
    "GTK_",
    "GIO_",
    "GSETTINGS_",
    "LIBGL_",
    "MESA_",
    "__GL",
    "__EGL",
    "MATRIXPLAYER_",
    "RUST_",
    "XDG_DATA_DIRS",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "WAYLAND_DISPLAY",
    "DISPLAY",
];

fn relevant(name: &str) -> bool {
    VARIABLES.iter().any(|v| if v.ends_with('_') { name.starts_with(v) } else { name == *v })
}

/// Logs the version, how the app was started, and the system it runs on.
pub fn report() {
    log::info!(
        "MatrixPlayer {} starting ({} build, pid {}); times in this log are UTC",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) { "debug" } else { "release" },
        std::process::id()
    );
    log::info!("command line: {:?}", std::env::args().collect::<Vec<_>>());
    if let Ok(exe) = std::env::current_exe() {
        log::info!("executable: {}", exe.display());
    }
    log::info!("system: {}, kernel {}", os_name(), read("/proc/sys/kernel/osrelease").unwrap_or_default());
    let mut vars: Vec<_> = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.to_string_lossy().into_owned())))
        .filter(|(k, _)| relevant(k))
        .collect();
    vars.sort();
    for (name, value) in vars {
        log::info!("environment: {name}={value}");
    }
}

fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

/// The distribution's name and version, from os-release.
fn os_name() -> String {
    let release = read("/etc/os-release").or_else(|| read("/usr/lib/os-release")).unwrap_or_default();
    let field = |key: &str| {
        release.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).map(|v| v.trim_matches('"').to_owned())
    };
    let name = field("PRETTY_NAME").or_else(|| field("NAME")).unwrap_or_else(|| "unknown Linux".into());
    match field("VERSION_ID").or_else(|| field("BUILD_ID")) {
        Some(version) if !name.contains(&version) => format!("{name} ({version})"),
        _ => name,
    }
}

/// What another process is: its executable, how long it has run, and
/// whether it is running, sleeping, or stuck (`D`, uninterruptible).
pub fn describe_process(pid: u32) -> String {
    let proc = Path::new("/proc").join(pid.to_string());
    let exe = fs::read_link(proc.join("exe")).map(|p| p.display().to_string()).unwrap_or_else(|_| "?".into());
    let state = fs::read_to_string(proc.join("status"))
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("State:").map(|v| v.trim().to_owned())))
        .unwrap_or_else(|| "?".into());
    let age = fs::read_to_string(proc.join("stat"))
        .ok()
        .zip(read("/proc/uptime"))
        .and_then(|(stat, uptime)| running_for(&stat, &uptime))
        .map(|d| format!(", running for {}", duration(d)))
        .unwrap_or_default();
    format!("pid {pid}, {exe}{age}, state {state}")
}

/// How long a process has run, from its `/proc/<pid>/stat` (start time in
/// clock ticks since boot, 100 a second) and `/proc/uptime`.
fn running_for(stat: &str, uptime: &str) -> Option<Duration> {
    // Fields after the command name, which is in parentheses and may hold
    // spaces; the start time is field 22, the 20th of these.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let started = fields.get(19)?.parse::<u64>().ok()? as f64 / 100.0;
    let up = uptime.split_whitespace().next()?.parse::<f64>().ok()?;
    Some(Duration::from_secs_f64((up - started).max(0.0)))
}

fn duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min", s / 60),
        3600..86_400 => format!("{} h {} min", s / 3600, s / 60 % 60),
        _ => format!("{} d {} h", s / 86_400, s / 3600 % 24),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_variables_that_matter() {
        for name in
            ["APPDIR", "GST_PLUGIN_SYSTEM_PATH_1_0", "LD_LIBRARY_PATH", "WEBKIT_DISABLE_DMABUF_RENDERER", "DISPLAY"]
        {
            assert!(relevant(name), "{name}");
        }
        for name in ["HOME", "PATH", "DISPLAYS", "APPDIRS", "SSH_AUTH_SOCK"] {
            assert!(!relevant(name), "{name}");
        }
    }

    #[test]
    fn tells_how_long_a_process_has_run() {
        let stat = "4242 (matrix player) S 1 4242 4242 0 -1 4194560 2 0 0 0 0 0 0 0 20 0 30 0 100000 0 0";
        assert_eq!(running_for(stat, "4600.50 9000.00"), Some(Duration::from_secs_f64(3600.5)));
        assert_eq!(running_for("garbage", "1.0 1.0"), None);
        assert_eq!(duration(Duration::from_secs(42)), "42 s");
        assert_eq!(duration(Duration::from_secs(3 * 3600 + 120)), "3 h 2 min");
        assert_eq!(duration(Duration::from_secs(2 * 86_400 + 5 * 3600)), "2 d 5 h");
    }

    #[test]
    fn describes_this_process() {
        let me = describe_process(std::process::id());
        assert!(me.contains(&format!("pid {}", std::process::id())) && me.contains("running for"), "{me}");
        // Whatever it is doing at the moment (other tests write files).
        assert!(me.contains(", state ") && !me.ends_with("state ?"), "{me}");
    }
}
