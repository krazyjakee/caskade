//! The end-to-end harness's driver (#153), built only with the `e2e`
//! feature. `CASKADE_E2E_SCRIPT` names a script of controller presses; the
//! driver feeds them to the input service as a synthetic Xbox pad (the same
//! path real pads take: normalizer, `inputActionEvent`, focus engine,
//! command bindings) and never sends a key or a pointer event. Between
//! presses it reads the page, so a script can wait for what a player would
//! wait for. It writes a report to `CASKADE_E2E_REPORT` and quits.
//!
//! Script lines (`#` starts a comment):
//!
//! - `press <action> [times]`: a pad button, by its action (`confirm`,
//!   `nav.down`, `tab.next`, `guide`, …).
//! - `seek <direction> <text>`: press a direction until the focused
//!   control's text contains `<text>`.
//! - `seek-section <direction> <section>`: press a direction until focus is
//!   inside the focus section `<section>` (`sidebar`, `main`, …).
//! - `focus-is <text>`: the focused control reads `<text>`.
//! - `focus-has <text>`: the focused control's text contains `<text>`.
//! - `wait-text <text>`: the page shows `<text>`.
//! - `wait-event <event>`: `game-started`, `game-exited`, `settings-changed`
//!   or `console-on`/`console-off` happens.
//! - `sleep <ms>`.

use crate::commands::console::ConsoleModeEvent;
use caskade_input::{Button, GlyphFamily, InputService, Raw};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Listener, Manager, Runtime};
use tauri_specta::Event as _;

const PAD: u32 = 1;
/// Between a press and its release, and after it: the UI's frame to react.
const PRESS: Duration = Duration::from_millis(60);
const SETTLE: Duration = Duration::from_millis(250);
const WAIT: Duration = Duration::from_secs(20);
const SEEK_LIMIT: usize = 40;
const PROBE_EVENT: &str = "e2e-probe";

/// Whether this run is driven by a script.
pub fn enabled() -> bool {
    std::env::var_os("CASKADE_E2E_SCRIPT").is_some()
}

/// What the run did, for the test that started it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub ok: bool,
    pub failure: Option<String>,
    /// The script lines that ran.
    pub steps: Vec<String>,
    /// Trusted keyboard and pointer events the page saw: none, for a pad-only run.
    pub keyboard_events: u64,
    pub pointer_events: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Probe {
    id: u64,
    focused: String,
    /// The focus sections around the focused control, innermost first.
    sections: Vec<String>,
    text: String,
    keyboard: u64,
    pointer: u64,
}

/// Counts trusted key and pointer events from here on.
const INSTALL: &str = r#"(() => {
  if (window.__caskadeE2E) return;
  const counts = (window.__caskadeE2E = { keyboard: 0, pointer: 0 });
  for (const type of ["keydown", "keyup", "keypress"])
    addEventListener(type, (event) => { if (event.isTrusted) counts.keyboard++; }, true);
  for (const type of ["pointerdown", "pointerup", "mousedown", "mouseup", "click", "wheel", "touchstart"])
    addEventListener(type, (event) => { if (event.isTrusted) counts.pointer++; }, true);
})()"#;

/// Reports the focused control's text, the page's text and the counts.
fn probe_script(id: u64) -> String {
    format!(
        r#"(() => {{
  const at = document.activeElement;
  const labelled = (element) => (element.getAttribute("aria-labelledby") || "").split(" ")
    .map((id) => document.getElementById(id)?.textContent || "").join(" ").trim();
  const read = (element) => !element || element === document.body ? "" :
    (element.getAttribute("aria-label") || labelled(element) || element.closest("label")?.textContent || element.textContent || "")
      .replace(/\s+/g, " ").trim();
  const counts = window.__caskadeE2E ?? {{ keyboard: 0, pointer: 0 }};
  const sections = [];
  for (let section = at?.closest("[data-focus-section]"); section; section = section.parentElement?.closest("[data-focus-section]"))
    sections.push(section.dataset.focusSection);
  window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {{
    event: "{PROBE_EVENT}",
    payload: {{ id: {id}, focused: read(at), sections, text: (document.body.textContent || "").replace(/\s+/g, " "), keyboard: counts.keyboard, pointer: counts.pointer }},
  }});
}})()"#
    )
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

struct Driver<R: Runtime> {
    app: AppHandle<R>,
    pads: InputService,
    probes: Receiver<Probe>,
    events: Receiver<&'static str>,
    next_probe: u64,
    last: Probe,
}

impl<R: Runtime> Driver<R> {
    fn probe(&mut self) -> Result<Probe, String> {
        self.next_probe += 1;
        let id = self.next_probe;
        let window = self
            .app
            .get_webview_window("main")
            .ok_or("no main window")?;
        window
            .eval(probe_script(id))
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.probes.recv_timeout(left) {
                Ok(probe) if probe.id == id => {
                    self.last = probe.clone();
                    return Ok(probe);
                }
                Ok(_) => continue,
                Err(_) => return Err("the page didn't answer".into()),
            }
        }
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
                    "timed out waiting for {what}; focus is on {:?}; the page reads {page:?}",
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
                for _ in 0..SEEK_LIMIT {
                    let focused = self.probe()?.focused;
                    tracing::debug!(focused, "e2e seek");
                    if focused.contains(text) {
                        return Ok(());
                    }
                    self.press(direction)?;
                }
                Err(format!(
                    "never reached {text:?}; focus is on {:?}",
                    self.last.focused
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
            "wait-text" => self.until(&format!("{rest:?} on the page"), |probe| {
                probe.text.contains(rest)
            }),
            "wait-event" => {
                let wanted = match rest {
                    "game-started" => crate::events::GameStarted::NAME,
                    "game-exited" => crate::events::GameExited::NAME,
                    "settings-changed" => crate::events::SettingsChanged::NAME,
                    "console-on" | "console-off" => ConsoleModeEvent::NAME,
                    other => return Err(format!("unknown event {other:?}")),
                };
                let deadline = Instant::now() + WAIT;
                loop {
                    let left = deadline.saturating_duration_since(Instant::now());
                    match self.events.recv_timeout(left) {
                        Ok(name) if name == wanted => break,
                        Ok(_) => continue,
                        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                            return Err(format!("no {rest} event"));
                        }
                    }
                }
                if let Some(active) = rest.strip_prefix("console-") {
                    let on = self
                        .app
                        .state::<crate::commands::console::ConsoleWindow>()
                        .active();
                    if on != (active == "on") {
                        return Err(format!("console mode isn't {active}"));
                    }
                }
                Ok(())
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

/// Starts the scripted run in the background once the page is up.
pub fn start<R: Runtime>(app: &AppHandle<R>, pads: InputService) {
    let script = PathBuf::from(std::env::var_os("CASKADE_E2E_SCRIPT").unwrap_or_default());
    let report_path = std::env::var_os("CASKADE_E2E_REPORT").map(PathBuf::from);
    let (probe_tx, probes) = mpsc::channel();
    app.listen_any(PROBE_EVENT, move |event| {
        if let Ok(probe) = serde_json::from_str::<Probe>(event.payload()) {
            let _ = probe_tx.send(probe);
        }
    });
    let (event_tx, events) = mpsc::channel();
    for name in [
        crate::events::GameStarted::NAME,
        crate::events::GameExited::NAME,
        crate::events::SettingsChanged::NAME,
        ConsoleModeEvent::NAME,
    ] {
        let sender = event_tx.clone();
        app.listen_any(name, move |_| {
            let _ = sender.send(name);
        });
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut driver = Driver {
            app: app.clone(),
            pads,
            probes,
            events,
            next_probe: 0,
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
        app.exit(if report.ok { 0 } else { 1 });
    });
}

fn run<R: Runtime>(
    driver: &mut Driver<R>,
    script: &PathBuf,
    report: &mut Report,
) -> Result<(), String> {
    let script = std::fs::read_to_string(script).map_err(|error| format!("script: {error}"))?;
    // The page is up once it answers; then count input from the start.
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if driver.probe().is_ok() {
            break;
        }
        if Instant::now() > deadline {
            return Err("the page never loaded".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    driver
        .app
        .get_webview_window("main")
        .ok_or("no main window")?
        .eval(INSTALL)
        .map_err(|error| error.to_string())?;
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
