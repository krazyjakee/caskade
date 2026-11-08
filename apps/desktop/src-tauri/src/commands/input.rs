//! Console mode input (#146): gamepads read by `caskade-input`, forwarded
//! to the webview as `inputActionEvent`s while a Caskade window has focus.
//! `guide` is always forwarded, so it can summon Caskade over a game.

use caskade_core::bus::{Bus, Event};
use caskade_input::{Action, InputAction, InputEvent, InputService, PadInfo};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event as _;

/// One normalized action from a pad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct InputActionEvent(pub InputAction);

/// Where the service's events go. `focused` decides whether the user is in
/// Caskade; `summon` runs on `guide` (#149) before the webview hears it;
/// pad connects reach the bus as `input.devices.changed`.
pub fn sink<R: Runtime>(
    app: AppHandle<R>,
    bus: Option<Bus>,
    focused: impl Fn(&AppHandle<R>) -> bool + Send + Sync + 'static,
    summon: impl Fn(&AppHandle<R>) + Send + Sync + 'static,
) -> impl Fn(InputEvent) + Send + Sync + 'static {
    move |event| match event {
        InputEvent::Action(action) => {
            if action.action == Action::Guide {
                summon(&app);
            }
            if (action.action == Action::Guide || focused(&app))
                && let Err(error) = InputActionEvent(action).emit(&app)
            {
                tracing::warn!(%error, "input action not delivered");
            }
        }
        InputEvent::Devices(pads) => {
            tracing::info!(pads = pads.len(), "gamepads changed");
            if let Some(bus) = &bus {
                bus.emit(Event::InputDevicesChanged {
                    connected: u32::try_from(pads.len()).unwrap_or(u32::MAX),
                });
            }
        }
    }
}

/// Whether any Caskade window has focus.
pub fn window_focused<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.webview_windows()
        .values()
        .any(|window| window.is_focused().unwrap_or(false))
}

/// Starts reading pads and manages the service.
pub fn setup<R: Runtime>(app: &AppHandle<R>, bus: Bus) {
    let summoner = Summoner::default();
    let summon = move |app: &AppHandle<R>| {
        if summoner.due(std::time::Instant::now()) {
            raise(app);
        }
    };
    // A scripted end-to-end run (#153) is the only "pad", and the window
    // counts as focused (Xvfb may run without a window manager).
    #[cfg(feature = "e2e")]
    if crate::e2e::enabled() {
        let service = InputService::without_pads(
            sink(app.clone(), Some(bus), |_| true, summon),
            caskade_input::Timing::default(),
        );
        crate::e2e::start(app, service.clone());
        app.manage(service);
        return;
    }
    let service = InputService::start(sink(app.clone(), Some(bus), window_focused, summon));
    app.manage(service);
}

/// Repeated guide presses within this raise the window once.
const SUMMON_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(600);

/// Debounces `guide`.
#[derive(Default)]
pub struct Summoner(std::sync::Mutex<Option<std::time::Instant>>);

impl Summoner {
    /// Whether a press at `now` should raise the window.
    pub fn due(&self, now: std::time::Instant) -> bool {
        let mut last = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.is_some_and(|at| now.saturating_duration_since(at) < SUMMON_DEBOUNCE) {
            return false;
        }
        *last = Some(now);
        true
    }
}

/// Brings the main window to the front for guide, even over a running game
/// or from the tray.
pub fn raise<R: Runtime>(app: &AppHandle<R>) {
    crate::main_window::raise(app);
    tracing::info!("guide: Caskade raised");
}

/// The connected gamepads; the one used last is active.
#[tauri::command]
#[specta::specta]
pub fn input_devices(state: tauri::State<'_, InputService>) -> Vec<PadInfo> {
    state.devices()
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
