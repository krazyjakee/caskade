//! Console mode input (#146): gamepads read by `caskade-input`, forwarded
//! to the webview as `inputActionEvent`s while a Caskade window has focus.
//! `guide` is always forwarded, so it can summon Caskade over a game.

pub use app::Summoner;
use caskade_app::commands::input as app;
// The webview's event, which the app sends as `caskade_app`'s.
pub use crate::events::InputActionEvent;
use caskade_core::bus::Bus;
use caskade_input::{InputEvent, InputService, PadInfo};
use tauri::{AppHandle, Manager, Runtime};

/// Where the service's events go (`caskade_app`'s sink, with the window's
/// focus and raise).
pub fn sink<R: Runtime>(
    app: AppHandle<R>,
    bus: Option<Bus>,
    focused: impl Fn(&AppHandle<R>) -> bool + Send + Sync + 'static,
    summon: impl Fn(&AppHandle<R>) + Send + Sync + 'static,
) -> impl Fn(InputEvent) + Send + Sync + 'static {
    let (focus, raise) = (app.clone(), app.clone());
    app::sink(
        crate::events::sink(app),
        bus,
        move || focused(&focus),
        move || summon(&raise),
    )
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
    app::input_devices(&state)
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
