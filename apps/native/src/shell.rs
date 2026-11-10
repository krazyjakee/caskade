//! The native shell's side of the app layer (#87): the services start
//! (`caskade_app::start`) with what a winit window can answer, the main
//! window follows games (`caskade_app::game_focus`), and its place and size
//! are kept (`window_state`). Everything here runs without Tauri.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use caskade_app::events::{Emit, Event, Events};
use caskade_app::game_focus::{GameWindow, Making};
use caskade_app::start::{Core, Services, Shell};
use caskade_core::launch::WindowPlacement;
use dioxus_native::winit::{
    event::WindowEvent,
    window::{Window, WindowLevel},
};
use tokio::sync::broadcast;

use crate::window_state::WindowState;

/// How many events a slow reader may fall behind before it misses some.
const EVENTS_BUFFER: usize = 1024;
/// The window state is written at most this often while it changes; the
/// last one is written at exit.
const SAVE_EVERY: Duration = Duration::from_millis(500);

/// What the app tells the UI: every event, for the UI's data layer (#89)
/// to read.
pub fn events() -> &'static broadcast::Sender<Event> {
    static EVENTS: OnceLock<broadcast::Sender<Event>> = OnceLock::new();
    EVENTS.get_or_init(|| broadcast::channel(EVENTS_BUFFER).0)
}

/// The app layer's sink for the native UI.
struct NativeEvents;

impl Emit for NativeEvents {
    fn emit(&self, event: Event) -> Result<(), String> {
        // Nobody listening yet isn't a failure: the UI reads state when it
        // starts.
        let _ = events().send(event);
        Ok(())
    }
}

/// The native UI's events, as the app layer sends them.
pub fn sink() -> Events {
    Events::new(NativeEvents)
}

/// The services, once they've started.
pub fn services() -> Option<&'static Services> {
    SERVICES.get()
}

static SERVICES: OnceLock<Services> = OnceLock::new();

/// The main window's place on screen, while it's shown.
static PLACEMENT: Mutex<Option<WindowPlacement>> = Mutex::new(None);

/// The key Caskade's releases are signed with, which the add-on catalogue
/// is checked against too: the Tauri app's updater key.
fn release_key() -> Option<String> {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../desktop/src-tauri/tauri.conf.json")).ok()?;
    config["plugins"]["updater"]["pubkey"]
        .as_str()
        .map(str::to_owned)
}

/// Starts every other service on `core`, off the main thread so the window
/// shows at once. `ready` runs with them once they're up.
pub fn start(core: Core, ready: impl FnOnce(&'static Services) + Send + 'static) {
    let started = std::thread::Builder::new()
        .name("caskade-start".into())
        .spawn(move || {
            let shell = Shell {
                events: sink(),
                placement: Arc::new(|| *PLACEMENT.lock().unwrap_or_else(|e| e.into_inner())),
                // Nothing yet tells a started program which startup it
                // belongs to (an X11 startup notification): BLITZ-9 (#88).
                activation: Arc::new(Vec::new),
                release_key: release_key(),
            };
            let services = caskade_app::start::start(core, shell);
            if SERVICES.set(services).is_err() {
                return;
            }
            if let Some(services) = SERVICES.get() {
                tracing::info!("services started");
                ready(services);
            }
        });
    if let Err(error) = started {
        tracing::error!(%error, "the services couldn't start");
    }
}

/// When Caskade quits: the streaming host stops with it (#257).
pub fn stop() {
    if let Some(host) = services().and_then(|services| services.streaming_host.as_ref()) {
        host.shutdown();
    }
}

/// The main window as games see it (`caskade_app::game_focus`).
pub struct NativeWindow(pub Arc<dyn Window>);

impl GameWindow for NativeWindow {
    fn present(&self) -> bool {
        true
    }

    fn cover(&self, on: bool) -> Result<(), String> {
        self.0.set_window_level(if on {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        });
        Ok(())
    }

    fn make_way(&self, way: Making) -> Result<(), String> {
        match way {
            Making::Minimize => self.0.set_minimized(true),
            Making::Hide => self.0.set_visible(false),
            // Console mode isn't in the native app yet (BLITZ-13, #92).
            Making::Stay | Making::Fade => {}
        }
        Ok(())
    }

    fn raise(&self) {
        self.0.set_visible(true);
        self.0.set_minimized(false);
        self.0.focus_window();
    }

    fn tray_shown(&self) -> bool {
        crate::tray::state().shown()
    }
}

/// The main window's state as it moves and resizes, written to `file` at
/// most every [`SAVE_EVERY`] and at exit.
pub struct Keeper {
    file: std::path::PathBuf,
    state: Mutex<(Option<WindowState>, Option<Instant>)>,
}

impl Keeper {
    pub fn new(file: std::path::PathBuf, restored: Option<WindowState>) -> Self {
        Self {
            file,
            state: Mutex::new((restored, None)),
        }
    }

    /// Follows the window after `event`, and tells the services where it is.
    pub fn follow(&self, window: &dyn Window, event: &WindowEvent) {
        if !matches!(
            event,
            WindowEvent::SurfaceResized(_) | WindowEvent::Moved(_) | WindowEvent::CloseRequested
        ) {
            return;
        }
        let Ok(position) = window.outer_position() else {
            return;
        };
        let size = window.surface_size();
        let minimized = window.is_minimized().unwrap_or(false);
        let shown = window.is_visible().unwrap_or(true) && !minimized;
        *PLACEMENT.lock().unwrap_or_else(|e| e.into_inner()) = shown.then_some(WindowPlacement {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        });
        if minimized || size.width == 0 || size.height == 0 {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let next = WindowState::followed(
            state.0,
            (position.x, position.y),
            (size.width, size.height),
            window.is_maximized(),
        );
        state.0 = Some(next);
        let due = state.1.is_none_or(|at| at.elapsed() >= SAVE_EVERY)
            || matches!(event, WindowEvent::CloseRequested);
        if due {
            next.save(&self.file);
            state.1 = Some(Instant::now());
        }
    }

    /// Writes the last state, at exit.
    pub fn save(&self) {
        if let Some(state) = self.state.lock().unwrap_or_else(|e| e.into_inner()).0 {
            state.save(&self.file);
        }
    }
}

static MAIN_WINDOW: OnceLock<Arc<dyn Window>> = OnceLock::new();

/// The main window, once it's open.
pub fn set_main_window(window: Arc<dyn Window>) {
    let _ = MAIN_WINDOW.set(window);
}

/// The main window, once it's open.
pub fn main_window() -> Option<&'static Arc<dyn Window>> {
    MAIN_WINDOW.get()
}

/// The video window open now (`caskade_app::commands::video`): one at a
/// time.
pub fn video_window() -> &'static caskade_app::commands::video::VideoWindow {
    static VIDEO: OnceLock<caskade_app::commands::video::VideoWindow> = OnceLock::new();
    VIDEO.get_or_init(Default::default)
}
