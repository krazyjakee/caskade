//! Console mode's window (#148): fullscreen in console mode, the desktop
//! size and position back on leaving it. Window-state never sees the
//! fullscreen window: it doesn't save fullscreen, and closing in console
//! mode first puts the desktop window back.
//!
//! Caskade starts in console mode with `--console`, with `CASKADE_CONSOLE=1`
//! (`0` keeps it out), or by the `core.console` setting: `auto` follows
//! gamescope (Steam Deck game mode), then the mode it was last left in.
//!
//! Which mode to start in, and remembering it, are
//! `caskade_app::commands::console`'s; the window is the adapter's.

use crate::tray::Tray;
use caskade_app::commands::console as app;
use caskade_core::settings::SettingsService;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};
use tauri_specta::Event as _;

pub use app::Startup;
#[cfg(test)]
use {
    app::{SETTINGS, in_gamescope, wants_console},
    caskade_core::settings::ConsoleSettings,
};

/// Console mode went on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleModeEvent {
    pub active: bool,
}

/// A second `caskade --console` while the webview has unsaved edits: it asks
/// about them first, then switches (#70).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleRequestEvent {
    pub active: bool,
}

/// The desktop window, put back on leaving console mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopFrame {
    pub maximized: bool,
    pub position: PhysicalPosition<i32>,
    pub size: PhysicalSize<u32>,
}

/// What console mode does to a window: the real one, or a fake in tests.
pub trait Frame {
    fn desktop(&self) -> tauri::Result<DesktopFrame>;
    fn fullscreen(&self, on: bool) -> tauri::Result<()>;
    fn restore(&self, desktop: &DesktopFrame) -> tauri::Result<()>;
}

impl<R: Runtime> Frame for WebviewWindow<R> {
    fn desktop(&self) -> tauri::Result<DesktopFrame> {
        Ok(DesktopFrame {
            maximized: self.is_maximized()?,
            position: self.outer_position()?,
            size: self.inner_size()?,
        })
    }

    fn fullscreen(&self, on: bool) -> tauri::Result<()> {
        self.set_fullscreen(on)
    }

    fn restore(&self, desktop: &DesktopFrame) -> tauri::Result<()> {
        if desktop.maximized {
            return self.maximize();
        }
        self.set_size(desktop.size)?;
        self.set_position(desktop.position)
    }
}

/// Console mode's state: the desktop window to return to while it is on.
#[derive(Debug, Default)]
pub struct ConsoleWindow(Mutex<Option<DesktopFrame>>);

impl ConsoleWindow {
    pub fn active(&self) -> bool {
        self.lock().is_some()
    }

    /// Keeps the desktop window to put back (`known`, or as the window is
    /// now) and goes fullscreen; false when console mode was already on.
    pub fn enter(&self, frame: &impl Frame, known: Option<DesktopFrame>) -> tauri::Result<bool> {
        let mut state = self.lock();
        if state.is_some() {
            return Ok(false);
        }
        let desktop = match known {
            Some(desktop) => desktop,
            None => frame.desktop()?,
        };
        frame.fullscreen(true)?;
        *state = Some(desktop);
        Ok(true)
    }

    /// Leaves fullscreen and puts the desktop window back; false when
    /// console mode was off.
    pub fn leave(&self, frame: &impl Frame) -> tauri::Result<bool> {
        let Some(desktop) = self.lock().take() else {
            return Ok(false);
        };
        frame.fullscreen(false)?;
        frame.restore(&desktop)?;
        Ok(true)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<DesktopFrame>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Whether console mode is on.
pub fn is_on<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<ConsoleWindow>()
        .is_some_and(|console| console.active())
}

/// Whether a close puts the desktop window back now. Not while the webview
/// holds it: leaving console mode swaps the shell, and that remounts Settings
/// and drops the edits it's about to ask about.
fn restores_on_close<R: Runtime>(app: &AppHandle<R>) -> bool {
    is_on(app) && !app.try_state::<Tray>().is_some_and(|tray| tray.held())
}

/// Puts the desktop window back before an exit that can't go through a
/// close, and gives it the time a close would.
pub fn restore_before_exit<R: Runtime>(app: &AppHandle<R>) {
    if !is_on(app) {
        return;
    }
    if let Err(error) = switch(app, false) {
        tracing::warn!(%error, "couldn't leave console mode before exiting");
    }
    std::thread::sleep(CLOSE_AFTER_RESTORE);
}

/// Turns console mode on or off for the main window and tells the webview.
pub fn switch<R: Runtime>(app: &AppHandle<R>, active: bool) -> Result<(), String> {
    change(app, active, None)
}

/// `caskade --console` while Caskade runs: switches at once, unless the
/// shell swap would drop unsaved edits, which the webview asks about first.
pub fn requested<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if app.try_state::<Tray>().is_some_and(|tray| tray.held()) {
        return ConsoleRequestEvent { active: true }
            .emit(app)
            .map_err(|error| error.to_string());
    }
    switch(app, true)
}

fn change<R: Runtime>(
    app: &AppHandle<R>,
    active: bool,
    known: Option<DesktopFrame>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| caskade_core::tr!("the main window is gone"))?;
    let console = app
        .try_state::<ConsoleWindow>()
        .ok_or_else(|| caskade_core::tr!("console mode isn't ready yet"))?;
    let changed = if active {
        console.enter(&window, known)
    } else {
        console.leave(&window)
    }
    .map_err(|error| error.to_string())?;
    if changed {
        tracing::info!(active, "console mode");
        ConsoleModeEvent { active }
            .emit(app)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// The desktop window as window-state saved it. Starting in console mode
/// races window-state's restore, so the window itself isn't asked yet.
fn saved_desktop<R: Runtime>(app: &AppHandle<R>) -> Option<DesktopFrame> {
    let path = app
        .path()
        .app_config_dir()
        .ok()?
        .join(tauri_plugin_window_state::DEFAULT_FILENAME);
    desktop_from_saved(&std::fs::read_to_string(path).ok()?)
}

/// The main window's frame in window-state's file: its inner size and
/// outer position, in physical pixels.
pub fn desktop_from_saved(text: &str) -> Option<DesktopFrame> {
    let saved: serde_json::Value = serde_json::from_str(text).ok()?;
    let main = saved.get("main")?;
    let number = |key: &str| main.get(key).and_then(serde_json::Value::as_i64);
    let size = PhysicalSize::new(
        u32::try_from(number("width")?).ok()?,
        u32::try_from(number("height")?).ok()?,
    );
    if size.width == 0 || size.height == 0 {
        return None;
    }
    Some(DesktopFrame {
        maximized: main.get("maximized")?.as_bool()?,
        position: PhysicalPosition::new(
            i32::try_from(number("x")?).ok()?,
            i32::try_from(number("y")?).ok()?,
        ),
        size,
    })
}

/// How long the window gets to leave fullscreen before a close in console
/// mode goes through.
const CLOSE_AFTER_RESTORE: Duration = Duration::from_millis(400);

/// Manages console mode, enters it when Caskade starts in it, and makes a
/// close in console mode put the desktop window back first.
pub fn setup<R: Runtime>(app: &AppHandle<R>, startup: Startup, service: &SettingsService) {
    app.manage(ConsoleWindow::default());
    if tauri::async_runtime::block_on(app::starts_in_console(&startup, service))
        && let Err(error) = change(app, true, saved_desktop(app))
    {
        tracing::warn!(%error, "couldn't start in console mode");
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        if !restores_on_close(&handle) {
            return;
        }
        api.prevent_close();
        // The close after the restore quits, even with close-to-tray.
        if let Some(tray) = handle.try_state::<crate::tray::Tray>() {
            tray.quit();
        }
        if let Err(error) = switch(&handle, false) {
            tracing::warn!(%error, "couldn't leave console mode before closing");
        }
        let handle = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(CLOSE_AFTER_RESTORE);
            // As the tray's Quit: a close that fails exits instead, or the
            // app would run on with no way to quit it (#160).
            crate::tray::quit(&handle);
        });
    });
}

/// Whether console mode is on (the frontend's first render).
#[tauri::command]
#[specta::specta]
pub fn console_initial_mode(state: tauri::State<'_, ConsoleWindow>) -> bool {
    state.active()
}

/// Turns console mode on or off. `keep` remembers it, as the toggle does; a
/// one-off `--console` doesn't.
#[tauri::command]
#[specta::specta]
pub async fn set_console_mode<R: Runtime>(
    app: AppHandle<R>,
    active: bool,
    keep: bool,
    service: tauri::State<'_, SettingsService>,
) -> Result<(), crate::error::CommandError> {
    switch(&app, active).map_err(crate::error::internal)?;
    if !keep {
        return Ok(());
    }
    app::remember(&service, active)
        .await
        .map_err(crate::error::internal)
}

/// Holds closes of the main window while the webview has unsaved edits, so
/// it can ask before one quits (`tray::Tray::hold` has what each call says).
/// Every account may call it: it asks, and the core's rules are untouched.
#[tauri::command]
#[specta::specta]
pub fn close_hold(held: bool, tray: tauri::State<'_, Tray>) {
    tray.hold(held);
}

#[cfg(test)]
#[path = "console_tests.rs"]
mod tests;
