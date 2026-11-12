//! The end-to-end harness's driver for the native app (#92, the Tauri
//! app's `e2e.rs` ported): debug builds and the `e2e` feature only.
//! `CASKADE_E2E_SCRIPT` names a script of controller presses; the driver
//! feeds them to the input service as a synthetic Xbox pad (the same path
//! real pads take: normalizer, app event, input root, bindings) and never
//! sends a key or a pointer event. Between presses it reads the window, so
//! a script can wait for what a player would wait for. It writes a report to
//! `CASKADE_E2E_REPORT` and quits.
//!
//! Script lines (`#` starts a comment), as the Tauri harness's:
//!
//! - `press <action> [times]`: a pad button, by its action (`confirm`,
//!   `nav.down`, `tab.next`, `guide`, …).
//! - `seek <direction> <text>`: press a direction until the focused
//!   control's text contains `<text>`.
//! - `seek-section <direction> <section>`: press a direction until focus is
//!   inside the focus section `<section>`.
//! - `focus-is <text>`: the focused control reads `<text>` (nothing focused
//!   reads as empty).
//! - `focus-has <text>`: the focused control's text contains `<text>`.
//! - `wait-text <text>`: the window shows `<text>`.
//! - `wait-event <event>`: `game-started`, `game-exited` or
//!   `settings-changed` happens. (Console mode's events come with console
//!   mode.)
//! - `sleep <ms>`.

use caskade_app::events::Event as AppEvent;
use caskade_input::{Button, GlyphFamily, InputService, Raw};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc as async_mpsc};

const PAD: u32 = 1;
/// Between a press and its release, and after it: the UI's frame to react.
const PRESS: Duration = Duration::from_millis(60);
const SETTLE: Duration = Duration::from_millis(250);
const WAIT: Duration = Duration::from_secs(20);
const SEEK_LIMIT: usize = 40;

/// Whether this run is driven by a script.
pub fn enabled() -> bool {
    cfg!(any(debug_assertions, feature = "e2e")) && std::env::var_os("CASKADE_E2E_SCRIPT").is_some()
}

/// What the run did, for the test that started it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub ok: bool,
    pub failure: Option<String>,
    /// The script lines that ran.
    pub steps: Vec<String>,
    /// Real keyboard and pointer events the window saw: none, for a
    /// pad-only run.
    pub keyboard_events: u64,
    pub pointer_events: u64,
}

/// The window as the driver reads it.
#[derive(Debug, Clone, Default)]
pub struct Probe {
    /// The focused control's accessible text; empty when none has focus.
    pub focused: String,
    /// The focus sections around the focused control, innermost first.
    pub sections: Vec<String>,
    /// The window's text.
    pub text: String,
    pub keyboard: u64,
    pub pointer: u64,
}

type Asks = async_mpsc::UnboundedReceiver<mpsc::Sender<Probe>>;

/// The driver's questions to the window, which the input root answers on
/// the UI thread; the receiving end until the root takes it.
struct Questions {
    ask: async_mpsc::UnboundedSender<mpsc::Sender<Probe>>,
    answer: Mutex<Option<Asks>>,
}

fn asks() -> &'static Questions {
    static ASKS: OnceLock<Questions> = OnceLock::new();
    ASKS.get_or_init(|| {
        let (ask, answer) = async_mpsc::unbounded_channel();
        Questions {
            ask,
            answer: Mutex::new(Some(answer)),
        }
    })
}

/// The questions, for the input root to answer: once, in a scripted run.
pub fn take_asks() -> Option<Asks> {
    if !enabled() {
        return None;
    }
    asks()
        .answer
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

fn button(action: &str) -> Option<Button> {
    Some(match action {
        "confirm" => Button::South,
        "back" => Button::East,
        "details" => Button::West,
        "search" => Button::North,
        "tab.prev" => Button::LeftBumper,
        "tab.next" => Button::RightBumper,
        "page.prev" => Button::LeftTrigger,
        "page.next" => Button::RightTrigger,
        "menu" => Button::Start,
        "view" => Button::Select,
        "guide" => Button::Guide,
        "nav.up" => Button::DPadUp,
        "nav.down" => Button::DPadDown,
        "nav.left" => Button::DPadLeft,
        "nav.right" => Button::DPadRight,
        _ => return None,
    })
}

struct Driver {
    pads: InputService,
    events: broadcast::Receiver<AppEvent>,
    last: Probe,
}

impl Driver {
    /// Reads the window. A window busy drawing (the first library frame on
    /// a CPU renderer takes seconds under CI's Xvfb) answers late: the
    /// driver waits as long as a step may, not less.
    fn probe(&mut self) -> Result<Probe, String> {
        let (reply, answer) = mpsc::channel();
        asks()
            .ask
            .send(reply)
            .map_err(|_| "the window is gone".to_owned())?;
        let probe = answer
            .recv_timeout(WAIT)
            .map_err(|_| format!("the window didn't answer for {} s", WAIT.as_secs()))?;
        self.last = probe.clone();
        Ok(probe)
    }

    fn press(&self, action: &str) -> Result<(), String> {
        let button = button(action).ok_or_else(|| format!("no button for {action:?}"))?;
        self.pads.inject(Raw::Pressed(PAD, button));
        std::thread::sleep(PRESS);
        self.pads.inject(Raw::Released(PAD, button));
        std::thread::sleep(SETTLE);
        Ok(())
    }

    fn until(&mut self, what: &str, done: impl Fn(&Probe) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + WAIT;
        loop {
            if done(&self.probe()?) {
                return Ok(());
            }
            if Instant::now() > deadline {
                let page: String = self.last.text.chars().take(400).collect();
                return Err(format!(
                    "timed out waiting for {what}; focus is on {:?}; the window reads {page:?}",
                    self.last.focused
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn step(&mut self, line: &str) -> Result<(), String> {
        let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        // A wait is for what the steps before it cause, not anything older.
        if command != "wait-event" {
            while self.events.try_recv().is_ok() {}
        }
        match command {
            "press" => {
                let mut parts = rest.split_whitespace();
                let action = parts.next().ok_or("press what?")?;
                let times: usize = parts
                    .next()
                    .map_or(Ok(1), str::parse)
                    .map_err(|_| "times")?;
                for _ in 0..times {
                    self.press(action)?;
                }
                Ok(())
            }
            "seek" => {
                let (direction, text) = rest.split_once(' ').ok_or("seek <direction> <text>")?;
                // Where focus went, to tell a layout's path from a missing
                // control.
                let mut path: Vec<String> = Vec::new();
                for _ in 0..SEEK_LIMIT {
                    let focused = self.probe()?.focused;
                    if focused.contains(text) {
                        return Ok(());
                    }
                    if path.last() != Some(&focused) {
                        path.push(focused);
                    }
                    self.press(direction)?;
                }
                Err(format!(
                    "never reached {text:?}; focus went {}",
                    path.iter()
                        .map(|focused| format!("{focused:?}"))
                        .collect::<Vec<_>>()
                        .join(" → ")
                ))
            }
            "seek-section" => {
                let (direction, section) = rest
                    .split_once(' ')
                    .ok_or("seek-section <direction> <section>")?;
                for _ in 0..SEEK_LIMIT {
                    if self.probe()?.sections.iter().any(|name| name == section) {
                        return Ok(());
                    }
                    self.press(direction)?;
                }
                Err(format!(
                    "never reached {section:?}; focus is on {:?}",
                    self.last.focused
                ))
            }
            "focus-is" => self.until(&format!("focus on {rest:?}"), |probe| probe.focused == rest),
            "focus-has" => self.until(&format!("focus on {rest:?}"), |probe| {
                probe.focused.contains(rest)
            }),
            "wait-text" => self.until(&format!("{rest:?} in the window"), |probe| {
                probe.text.contains(rest)
            }),
            "wait-event" => {
                let wanted = match rest {
                    "game-started" => "game.started",
                    "game-exited" => "game.exited",
                    "settings-changed" => "settings.changed",
                    "console-on" | "console-off" => {
                        return Err("the native app has no console mode yet".into());
                    }
                    other => return Err(format!("unknown event {other:?}")),
                };
                let deadline = Instant::now() + WAIT;
                loop {
                    match self.events.try_recv() {
                        Ok(event) if event.name() == wanted => return Ok(()),
                        Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                        Err(_) if Instant::now() > deadline => {
                            return Err(format!("no {rest} event"));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            }
            "sleep" => {
                let ms: u64 = rest.parse().map_err(|_| "sleep <ms>")?;
                std::thread::sleep(Duration::from_millis(ms));
                Ok(())
            }
            other => Err(format!("unknown step {other:?}")),
        }
    }
}

/// Starts the scripted run in the background; it waits for the window.
pub fn start(pads: InputService) {
    let script = PathBuf::from(std::env::var_os("CASKADE_E2E_SCRIPT").unwrap_or_default());
    let report_path = std::env::var_os("CASKADE_E2E_REPORT").map(PathBuf::from);
    let events = crate::shell::events().subscribe();
    std::thread::spawn(move || {
        let mut driver = Driver {
            pads,
            events,
            last: Probe::default(),
        };
        let mut report = Report::default();
        let outcome = run(&mut driver, &script, &mut report);
        // The counts as the run ends.
        if driver.probe().is_ok() {
            report.keyboard_events = driver.last.keyboard;
            report.pointer_events = driver.last.pointer;
        }
        report.ok = outcome.is_ok();
        report.failure = outcome.err();
        if let Some(path) = report_path {
            let _ = std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap_or_default());
        }
        tracing::info!(ok = report.ok, failure = ?report.failure, "e2e run finished");
        if !report.ok {
            std::process::exit(1);
        }
        crate::launch::ask(crate::launch::Request::Quit);
        // A quit that hangs still ends the run.
        std::thread::sleep(Duration::from_secs(20));
        std::process::exit(0);
    });
}

fn run(driver: &mut Driver, script: &PathBuf, report: &mut Report) -> Result<(), String> {
    let script = std::fs::read_to_string(script).map_err(|error| format!("script: {error}"))?;
    // The window is up once it answers.
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if driver.probe().is_ok() {
            break;
        }
        if Instant::now() > deadline {
            return Err("the window never came up".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    driver.pads.inject(Raw::Connected {
        pad: PAD,
        name: "Xbox Wireless Controller".into(),
        glyphs: GlyphFamily::Xbox,
    });
    for line in script.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        tracing::info!(step = line, "e2e");
        report.steps.push(line.to_owned());
        driver
            .step(line)
            .map_err(|error| format!("{line}: {error}"))?;
    }
    Ok(())
}
