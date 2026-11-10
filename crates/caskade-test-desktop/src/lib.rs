//! A private desktop for tests that start Caskade (#122): their own Xvfb
//! display, their own D-Bus session bus and their own runtime folder, so
//! nothing a test starts (a window, a tray icon, a notification, a keyring
//! prompt) reaches the developer's desktop. Running under `xvfb-run`
//! isolates X only: the session bus would still be the developer's, where a
//! test's tray icon registers with the real StatusNotifierWatcher, and
//! Cinnamon's crashes when such an icon vanishes (10 Nov 2026).
//!
//! [`PrivateDesktop::start`] starts both; [`PrivateDesktop::command`] gives
//! a [`Command`] with a cleared environment and only the private display,
//! bus and runtime folder (and `PATH`). Nothing is inherited, so a test
//! can't use the developer's desktop by accident. [`guard`] refuses a
//! display or bus that belongs to a login session, loudly, for harnesses
//! that are handed theirs.
//!
//! The bus has no service folders: nothing starts on it by activation (a
//! notification daemon, a keyring), so an app that asks for one gets an
//! error, as on a headless machine.
//!
//! Unix only (Xvfb and dbus-daemon): elsewhere the crate is empty, and the
//! tests that use it are Unix only too.
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long a stopped Xvfb may take to remove its socket (the test).
#[cfg(test)]
const READY: Duration = Duration::from_secs(15);

/// An Xvfb display and a session bus of the test's own, stopped on drop.
pub struct PrivateDesktop {
    dir: tempfile::TempDir,
    display: String,
    bus: String,
    xvfb: Child,
    dbus: Child,
}

impl PrivateDesktop {
    /// Starts a display of `1920x1080x24` and a bus. Panics, saying why,
    /// when either can't start: a test must not go on without them.
    pub fn start() -> Self {
        Self::start_sized("1920x1080x24")
    }

    /// As [`Self::start`], with the screen `size` (`WxHxDEPTH`).
    pub fn start_sized(size: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("caskade-desktop-")
            .tempdir()
            .expect("a folder for the private desktop");
        let runtime = dir.path().join("runtime");
        create_private(&runtime);
        let (xvfb, display) = start_xvfb(size);
        let (dbus, bus) = start_bus(dir.path(), &runtime);
        let desktop = Self {
            dir,
            display,
            bus,
            xvfb,
            dbus,
        };
        if let Err(error) = guard(Some(&desktop.display), Some(&desktop.bus)) {
            panic!("the private desktop isn't private: {error}");
        }
        desktop
    }

    /// The display, e.g. `:140`.
    pub fn display(&self) -> &str {
        &self.display
    }

    /// The bus's address.
    pub fn bus(&self) -> &str {
        &self.bus
    }

    /// The runtime folder (`XDG_RUNTIME_DIR`), private to the desktop.
    pub fn runtime(&self) -> PathBuf {
        self.dir.path().join("runtime")
    }

    /// The environment a program on this desktop gets: the display, the
    /// bus, the runtime folder, `PATH`, and accessibility off (its bus would
    /// be started on demand).
    pub fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("PATH", std::env::var("PATH").unwrap_or_default()),
            ("DISPLAY", self.display.clone()),
            ("DBUS_SESSION_BUS_ADDRESS", self.bus.clone()),
            ("XDG_RUNTIME_DIR", self.runtime().display().to_string()),
            ("NO_AT_BRIDGE", "1".into()),
            ("GTK_A11Y", "none".into()),
        ]
    }

    /// `program` with a cleared environment and [`Self::env`]; the caller
    /// adds its own (`HOME`, the XDG folders, the app's variables).
    pub fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command.env_clear().envs(self.env());
        command
    }
}

impl Drop for PrivateDesktop {
    fn drop(&mut self) {
        // Our own children, by PID; never anything by name. SIGTERM lets
        // Xvfb remove its lock and socket.
        for child in [&mut self.dbus, &mut self.xvfb] {
            stop(child);
        }
        let number = &self.display[1..];
        for leftover in [
            format!("/tmp/.X{number}-lock"),
            format!("/tmp/.X11-unix/X{number}"),
        ] {
            if Path::new(&leftover).exists()
                && self.xvfb.try_wait().is_ok_and(|done| done.is_some())
            {
                let _ = std::fs::remove_file(leftover);
            }
        }
    }
}

/// Stops `child` with SIGTERM, then SIGKILL after a second.
fn stop(child: &mut Child) {
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(1) {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn create_private(folder: &Path) {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
        .expect("the private runtime folder");
}

/// Xvfb on a display it picks itself (`-displayfd`): free, and taken
/// atomically, so tests starting at once never share one.
fn start_xvfb(size: &str) -> (Child, String) {
    let mut xvfb = Command::new("Xvfb")
        .args([
            "-displayfd",
            "1",
            "-nolisten",
            "tcp",
            "-noreset",
            "-screen",
            "0",
            size,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Xvfb (install xvfb)");
    let mut number = String::new();
    let read = xvfb
        .stdout
        .take()
        .map(|out| BufReader::new(out).read_line(&mut number));
    let number = number.trim().to_owned();
    if !matches!(read, Some(Ok(n)) if n > 0) || number.parse::<u32>().is_err() {
        let _ = xvfb.kill();
        let _ = xvfb.wait();
        panic!("Xvfb didn't say which display it took");
    }
    (xvfb, format!(":{number}"))
}

/// A session bus listening in `dir`, without service folders.
fn start_bus(dir: &Path, runtime: &Path) -> (Child, String) {
    let config = dir.join("bus.conf");
    std::fs::write(&config, bus_config(runtime)).expect("the bus's configuration");
    let mut dbus = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .args(["--nofork", "--print-address"])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbus-daemon (install dbus)");
    let mut address = String::new();
    let read = dbus
        .stdout
        .take()
        .map(|out| BufReader::new(out).read_line(&mut address));
    let address = address.trim().to_owned();
    if !matches!(read, Some(Ok(n)) if n > 0) || address.is_empty() {
        let _ = dbus.kill();
        let _ = dbus.wait();
        panic!("dbus-daemon didn't print its address");
    }
    (dbus, address)
}

/// A session bus that anyone in this user may use, with no service
/// folders, listening in `dir`.
fn bus_config(dir: &Path) -> String {
    format!(
        r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <keep_umask/>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
        dir.display()
    )
}

/// This user's id, from `/proc/self/status`.
fn uid() -> Option<u32> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// The socket path of a `unix:path=…` bus address.
fn bus_path(address: &str) -> Option<String> {
    address
        .split(';')
        .flat_map(|part| part.split(','))
        .find_map(|item| {
            item.strip_prefix("unix:path=")
                .or_else(|| item.strip_prefix("path="))
        })
        .map(str::to_owned)
}

fn output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

/// What the login sessions use: their displays and their buses' sockets.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LoginSessions {
    /// X displays (`:0`) and Wayland sockets (`wayland-0`).
    pub displays: Vec<String>,
    /// Session bus socket paths.
    pub buses: Vec<String>,
}

/// The login sessions of this machine, read without starting anything:
/// `loginctl`'s graphical sessions, this user's bus (`/run/user/<uid>/bus`)
/// and the bus the user's service manager hands out.
pub fn login_sessions() -> LoginSessions {
    let mut sessions = LoginSessions::default();
    for line in output("loginctl", &["list-sessions", "--no-legend"]).lines() {
        let Some(id) = line.split_whitespace().next() else {
            continue;
        };
        let display = output(
            "loginctl",
            &["show-session", id, "-p", "Display", "--value"],
        );
        let display = display.trim();
        if !display.is_empty() {
            sessions.displays.push(display.to_owned());
        }
    }
    if let Some(uid) = uid() {
        sessions.buses.push(format!("/run/user/{uid}/bus"));
        let runtime = PathBuf::from(format!("/run/user/{uid}"));
        if let Ok(entries) = std::fs::read_dir(&runtime) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("wayland-") && !name.ends_with(".lock") {
                    sessions.displays.push(name);
                }
            }
        }
    }
    for line in output("systemctl", &["--user", "show-environment"]).lines() {
        if let Some(address) = line.strip_prefix("DBUS_SESSION_BUS_ADDRESS=")
            && let Some(path) = bus_path(address)
        {
            sessions.buses.push(path);
        }
        if let Some(display) = line.strip_prefix("DISPLAY=") {
            sessions.displays.push(display.to_owned());
        }
        if let Some(display) = line.strip_prefix("WAYLAND_DISPLAY=") {
            sessions.displays.push(display.to_owned());
        }
    }
    sessions.displays.sort();
    sessions.displays.dedup();
    sessions.buses.sort();
    sessions.buses.dedup();
    sessions
}

/// `:0.0` and `:0` are the same display; `unix/:0` too.
fn same_display(a: &str, b: &str) -> bool {
    let norm = |display: &str| {
        let display = display.rsplit('/').next().unwrap_or(display);
        let display = display.strip_prefix("unix").unwrap_or(display);
        match display.split_once('.') {
            Some((head, screen)) if screen.chars().all(|c| c.is_ascii_digit()) => head.to_owned(),
            _ => display.to_owned(),
        }
    };
    norm(a) == norm(b)
}

/// Refuses a display or bus a login session uses, saying which: a harness
/// handed the developer's desktop must stop before it starts anything.
pub fn guard(display: Option<&str>, bus: Option<&str>) -> Result<(), String> {
    guard_against(&login_sessions(), display, bus)
}

/// [`guard`] against `sessions`.
pub fn guard_against(
    sessions: &LoginSessions,
    display: Option<&str>,
    bus: Option<&str>,
) -> Result<(), String> {
    let mut problems = Vec::new();
    if let Some(display) = display.filter(|display| !display.is_empty())
        && sessions
            .displays
            .iter()
            .any(|login| same_display(login, display))
    {
        problems.push(format!(
            "the display {display} is a login session's: run on a private one (caskade-test-desktop, scripts/private-desktop.sh)"
        ));
    }
    if let Some(bus) = bus.filter(|bus| !bus.is_empty()) {
        match bus_path(bus) {
            Some(path) if sessions.buses.contains(&path) => problems.push(format!(
                "the bus {bus} is a login session's: its tray and notifications are the developer's desktop; run on a private bus"
            )),
            Some(_) => {}
            // An abstract socket or another transport can't be compared:
            // only a bus this harness started is trusted.
            None => problems.push(format!(
                "the bus {bus} isn't a socket path this guard can check: start a private bus"
            )),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// [`guard`] on this process's own `DISPLAY`, `WAYLAND_DISPLAY` and
/// `DBUS_SESSION_BUS_ADDRESS`: for a harness about to hand them on.
pub fn guard_this_process() -> Result<(), String> {
    let display = std::env::var("DISPLAY").ok();
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok();
    guard(display.as_deref(), bus.as_deref())?;
    guard(wayland.as_deref(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sessions() -> LoginSessions {
        LoginSessions {
            displays: vec![":0".into(), "wayland-0".into()],
            buses: vec!["/run/user/1000/bus".into()],
        }
    }

    #[test]
    fn the_guard_refuses_a_login_sessions_display_and_bus() {
        let refused = guard_against(
            &sessions(),
            Some(":0.0"),
            Some("unix:path=/run/user/1000/bus"),
        )
        .unwrap_err();
        assert!(refused.contains("display :0.0"), "{refused}");
        assert!(refused.contains("/run/user/1000/bus"), "{refused}");
        assert!(guard_against(&sessions(), Some("wayland-0"), None).is_err());
        assert!(
            guard_against(&sessions(), None, Some("unix:abstract=/tmp/dbus-x,guid=1")).is_err()
        );
        assert!(
            guard_against(
                &sessions(),
                Some(":140"),
                Some("unix:path=/tmp/caskade-desktop-x/runtime/dbus-1,guid=2")
            )
            .is_ok()
        );
        assert!(guard_against(&sessions(), None, None).is_ok());
    }

    #[test]
    fn this_machines_login_sessions_are_refused() {
        let sessions = login_sessions();
        for display in &sessions.displays {
            assert!(guard(Some(display), None).is_err(), "{display}");
        }
        for bus in &sessions.buses {
            assert!(
                guard(None, Some(&format!("unix:path={bus}"))).is_err(),
                "{bus}"
            );
        }
    }

    /// The desktop is private, and gone after the test.
    #[test]
    #[ignore = "starts Xvfb and dbus-daemon"]
    fn a_private_desktop_is_its_own() {
        let desktop = PrivateDesktop::start();
        let socket = format!("/tmp/.X11-unix/X{}", &desktop.display()[1..]);
        assert!(Path::new(&socket).exists());
        let env = desktop.env();
        assert!(env.iter().all(|(name, _)| *name != "WAYLAND_DISPLAY"));
        let names = desktop
            .command("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus.ListActivatableNames",
            ])
            .output()
            .unwrap();
        let names = String::from_utf8_lossy(&names.stdout);
        // Nothing can be started on it by activation but the bus itself.
        assert!(!names.contains("Notifications"), "{names}");
        assert!(names.contains("org.freedesktop.DBus"), "{names}");
        drop(desktop);
        let started = Instant::now();
        while Path::new(&socket).exists() && started.elapsed() < READY {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!Path::new(&socket).exists());
    }
}
