//! The main window's size and place, through window-state. The plugin saves
//! them only when the app exits cleanly, so a kill (`tauri dev` rebuilding
//! or stopped with Ctrl-C, a crash, a logout) lost every move since the last
//! Quit. The window's frame is also saved once it settles after a move or a
//! resize.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Runtime, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

/// How long the window has to stay put before its frame is saved.
const SETTLE: Duration = Duration::from_millis(500);

/// What window-state saves and restores. The main window is undecorated
/// (Caskade draws its title bar), so a saved `decorated` from an older build
/// must not bring the native one back, and console mode's fullscreen isn't
/// the desktop window's state either (#148). [`show`] shows the window, so a
/// Quit while it was hidden in the tray doesn't start Caskade hidden.
pub fn flags() -> StateFlags {
    StateFlags::all()
        .difference(StateFlags::DECORATIONS)
        .difference(StateFlags::FULLSCREEN)
        .difference(StateFlags::VISIBLE)
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_window_state::Builder::default()
        .with_state_flags(flags())
        .build()
}

/// Shows the main window once window-state and console mode have placed
/// it. It starts hidden (`tauri.conf.json`) because tao maps a window before
/// taking its decorations off: Muffin and Mutter then keep reporting the
/// title bar's frame extents for it, window-state saves a position that much
/// above the window, and every start put it higher (macOS keeps its
/// decorated window and starts it shown).
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        tracing::warn!(%error, "couldn't show the main window");
    }
}

/// Whether this start leaves the main window hidden in the tray: a login
/// start with Start minimized on (#114), when there's a tray icon to bring
/// it back from. The tray, a second start and the Dock show it later
/// (`main_window::raise`).
pub fn starts_in_tray<R: Runtime>(
    app: &AppHandle<R>,
    args: &[String],
    general: &caskade_core::settings::GeneralSettings,
) -> bool {
    if !caskade_core::login_item::starts_hidden(args, general) {
        return false;
    }
    let tray = app
        .try_state::<crate::tray::Tray>()
        .is_some_and(|tray| tray.shown());
    if tray {
        tracing::info!("started at login: Caskade stays in the tray");
        // macOS starts its window shown (`tauri.macos.conf.json`).
        #[cfg(target_os = "macos")]
        if let Some(window) = app.get_webview_window("main")
            && let Err(error) = window.hide()
        {
            tracing::warn!(%error, "couldn't hide the main window");
        }
    } else {
        tracing::info!("started at login without a tray icon: the window shows");
    }
    tray
}

/// Saves the window state a moment after the main window last moved or
/// resized.
pub fn save_on_change<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let (changed, changes) = mpsc::channel();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Moved(_) | WindowEvent::Resized(_)) {
            let _ = changed.send(());
        }
    });
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("window-state".into())
        .spawn(move || {
            settle(&changes, SETTLE, || {
                // On the main thread: window-state holds its cache's lock
                // while it asks the window for its frame, and the main
                // thread takes that lock for every move.
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || save(&handle));
            });
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "the window's place is saved only on quit");
    }
}

/// Calls `save` once the changes stop for `quiet`, until the sender is gone.
fn settle(changes: &Receiver<()>, quiet: Duration, save: impl Fn()) {
    while changes.recv().is_ok() {
        loop {
            match changes.recv_timeout(quiet) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        save();
    }
}

fn save<R: Runtime>(app: &AppHandle<R>) {
    // Console mode's fullscreen window isn't the desktop window; leaving it
    // puts that back, and moves it again.
    if crate::commands::console::is_on(app) {
        return;
    }
    if let Err(error) = app.save_window_state(flags()) {
        tracing::warn!(%error, "couldn't save the window's place");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_burst_of_changes_saves_once() {
        let (changed, changes) = mpsc::channel();
        let saves = &AtomicUsize::new(0);
        std::thread::scope(|scope| {
            scope.spawn(move || {
                settle(&changes, Duration::from_millis(50), || {
                    saves.fetch_add(1, Ordering::SeqCst);
                });
            });
            for _ in 0..5 {
                changed.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(saves.load(Ordering::SeqCst), 1);
            changed.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(saves.load(Ordering::SeqCst), 2);
            drop(changed);
        });
    }
}
