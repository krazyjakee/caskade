//! Console mode's window (#148): fullscreen in console mode, the desktop
//! size and position back on leaving it. Window-state never sees the
//! fullscreen window: it doesn't save fullscreen, and closing in console
//! mode first puts the desktop window back.
//!
//! Caskade starts in console mode with `--console`, with `CASKADE_CONSOLE=1`
//! (`0` keeps it out), or by the `core.console` setting: `auto` follows
//! gamescope (Steam Deck game mode), then the mode it was last left in.

use crate::tray::Tray;
use caskade_core::settings::{ConsoleSettings, SettingsService};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};
use tauri_specta::Event as _;

/// Console mode went on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleModeEvent {
    pub active: bool,
}

/// How Caskade was started, for the first mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Startup {
    /// `--console` on the command line.
    pub flag: bool,
    /// `CASKADE_CONSOLE`: `1` or `0` overrides the setting.
    pub forced: Option<bool>,
    /// Running inside gamescope.
    pub gamescope: bool,
}

impl Startup {
    pub fn read(args: &[String]) -> Self {
        Self::from(args, |name| std::env::var(name).ok())
    }

    pub fn from(args: &[String], var: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            flag: wants_console(args),
            forced: match var("CASKADE_CONSOLE").as_deref().map(str::trim) {
                Some("1" | "true") => Some(true),
                Some("0" | "false") => Some(false),
                _ => None,
            },
            gamescope: in_gamescope(var),
        }
    }

    /// Whether to start in console mode.
    pub fn console(&self, settings: &ConsoleSettings) -> bool {
        if self.flag {
            return true;
        }
        if let Some(forced) = self.forced {
            return forced;
        }
        match settings.start.as_str() {
            "console" => true,
            "desktop" => false,
            _ => self.gamescope || settings.active,
        }
    }
}

/// `--console` among `args` (the first one is the program).
pub fn wants_console(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg == "--console")
}

/// gamescope sets `GAMESCOPE_WAYLAND_DISPLAY` for its clients; Steam's game
/// mode also sets `SteamGamepadUI`.
pub fn in_gamescope(var: impl Fn(&str) -> Option<String>) -> bool {
    var("GAMESCOPE_WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
        || var("XDG_CURRENT_DESKTOP").is_some_and(|value| value.eq_ignore_ascii_case("gamescope"))
        || var("SteamGamepadUI").as_deref() == Some("1")
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

const SETTINGS: &str = "core.console";

/// Remembers the mode chosen with the toggle, for `auto` starts.
async fn remember(service: &SettingsService, active: bool) -> Result<(), String> {
    let mut value: ConsoleSettings = service.read_or_default(SETTINGS).await;
    if value.active == active {
        return Ok(());
    }
    value.active = active;
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    service
        .set(SETTINGS, value, None)
        .await
        .map(drop)
        .map_err(|error| error.to_string())
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
    let preferences = tauri::async_runtime::block_on(service.read_or_default(SETTINGS));
    if startup.console(&preferences)
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

/// Turns console mode on or off from the toggle, and remembers the choice.
#[tauri::command]
#[specta::specta]
pub async fn set_console_mode<R: Runtime>(
    app: AppHandle<R>,
    active: bool,
    service: tauri::State<'_, SettingsService>,
) -> Result<(), crate::error::CommandError> {
    switch(&app, active).map_err(crate::error::internal)?;
    remember(&service, active)
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
