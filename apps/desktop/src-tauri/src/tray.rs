//! The tray icon: its menu shows the main window or quits, and the close
//! button hides the window to it when `core.general.onClose` is `tray`.
//! The icon is the app icon's glyph without its dark square
//! (`scripts/tray-icons.py`).
//!
//! - Windows: a left click shows the window, a right click opens the menu.
//! - macOS: a menu bar extra. A click opens the menu, and the icon is a
//!   template image that macOS tints to match the bar. A click on the Dock
//!   icon shows a hidden window ([`on_run_event`]).
//! - Linux: an AppIndicator (StatusNotifierItem), loaded at run time from
//!   libayatana-appindicator3 or libappindicator3. Without either, Caskade
//!   runs without an icon and the close button quits. Indicators get no
//!   clicks, only the menu, and GNOME shows them only with the AppIndicator
//!   extension.
//!
//! Everywhere, a second `caskade` start shows the window too.
//!
//! The menu's labels are in the core's language and change with it, without
//! a restart ([`follow_language`]).
//!
//! A close in console mode quits: console mode puts the desktop window back
//! first (`commands::console`), then closes again through [`Tray::quit`].
//!
//! While the webview holds closes (`close_hold`, unsaved Settings edits, #58)
//! a close that would quit waits for its question: the window comes back
//! ([`main_window::raise`]) and console mode stays on. Hiding to the tray goes
//! on as ever, since the webview and its draft stay. A webview that doesn't
//! answer within [`UNANSWERED`] is given up on ([`give_up`]), so a hold can
//! never strand a quit.
//!
//! On Linux the icon is hidden a moment before Caskade exits or restarts
//! ([`hide_before_exit`]): hiding it makes the status-notifier watcher ask
//! for the icon's properties, and xapp's watcher (Cinnamon) crashes when
//! the icon's owner leaves the bus before answering, taking Cinnamon's tray
//! applet, and sometimes Cinnamon itself, down with it.

use crate::main_window;
pub use caskade_app::tray::{Close, Tray, quit_label, show_label};
use caskade_core::settings::SettingsService;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, Runtime};
use tokio::sync::watch;

const SHOW: &str = "tray.show";
const QUIT: &str = "tray.quit";

/// How long Caskade keeps answering the bus after hiding its icon, before
/// it exits: the watcher asks for the icon's properties within ~10 ms.
const HIDDEN_BEFORE_EXIT: Duration = Duration::from_millis(500);

/// How long the webview gets to say it heard a close it holds: it may be
/// gone, or its question out of sight.
const UNANSWERED: Duration = Duration::from_secs(5);

#[cfg(target_os = "macos")]
const ICON: Image<'_> = tauri::include_image!("icons/tray-template.png");
#[cfg(not(target_os = "macos"))]
const ICON: Image<'_> = tauri::include_image!("icons/tray.png");

/// Closes the main window for good, through console mode's restore and
/// window-state's save; the app exits with its last window.
pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(tray) = app.try_state::<Tray>() {
        tray.quit();
    }
    match app.get_webview_window("main") {
        Some(window) => {
            if let Err(error) = window.close() {
                tracing::warn!(%error, "couldn't close the main window");
                app.exit(0);
            }
        }
        None => app.exit(0),
    }
}

/// Gives up on a webview that didn't answer a held close: exits past it,
/// through console mode's restore. `app.exit`, because a listener left by a
/// dead webview has Tauri refuse a close; the exit still hides the icon first
/// ([`on_run_event`]).
fn give_up<R: Runtime>(app: &AppHandle<R>) {
    tracing::warn!("the webview didn't answer a close: quitting anyway");
    let tray = app.state::<Tray>();
    tray.hold(false);
    tray.quit();
    crate::commands::console::restore_before_exit(app);
    app.exit(0);
}

/// Manages [`Tray`], follows the close setting, hides the main window on
/// close and puts the icon up.
pub fn setup<R: Runtime>(app: &AppHandle<R>, settings: &SettingsService) {
    attach(app, settings);
    if !indicator_available() {
        tracing::warn!(
            "no tray icon: install libayatana-appindicator3 (or libappindicator3); \
             closing the window quits"
        );
        return;
    }
    match icon(app) {
        Ok(()) => app.state::<Tray>().set_shown(true),
        Err(error) => tracing::warn!(%error, "no tray icon; closing the window quits"),
    }
}

/// The app's run events: on macOS, a click on the Dock icon while the
/// window is in the tray shows it; on Linux, an exit waits for the icon to
/// be hidden ([`hide_before_exit`]).
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: RunEvent) {
    match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => main_window::raise(app),
        // A restart can't be held back: its callers hide the icon first
        // ([`restart`]).
        RunEvent::ExitRequested { code, api, .. }
            if code != Some(tauri::RESTART_EXIT_CODE) && hide_before_exit(app) =>
        {
            api.prevent_exit();
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(HIDDEN_BEFORE_EXIT);
                app.exit(code.unwrap_or(0));
            });
        }
        _ => {}
    }
}

/// Restarts Caskade, after hiding the icon on Linux ([`hide_before_exit`]).
pub fn restart<R: Runtime>(app: &AppHandle<R>) {
    if !hide_before_exit(app) {
        app.restart();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(HIDDEN_BEFORE_EXIT);
        app.restart();
    });
}

/// Linux: hides the icon while the event loop still answers the bus, so
/// the watcher's request for its properties gets an answer before Caskade
/// leaves. The icon's own drop at exit would hide it with no one left to
/// answer: xapp 3.2.3's watcher then reads the item it already freed
/// (`sn-item.c`: dispose drops the request's cancellable without
/// cancelling it). Returns whether the icon was up, so the caller waits
/// [`HIDDEN_BEFORE_EXIT`] before exiting; elsewhere it never is.
fn hide_before_exit<R: Runtime>(app: &AppHandle<R>) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let Some(tray) = app.try_state::<Tray>() else {
        return false;
    };
    if !tray.take_shown() {
        return false;
    }
    if let Some(icon) = app.tray_by_id("main")
        && let Err(error) = icon.set_visible(false)
    {
        tracing::warn!(%error, "couldn't hide the tray icon");
    }
    true
}

/// Everything but the icon: the state, the setting and the close handler.
pub fn attach<R: Runtime>(app: &AppHandle<R>, settings: &SettingsService) {
    app.manage(Tray::default());
    // Followed before the window can close, so the first close obeys it.
    app.state::<Tray>().follow(&tauri::async_runtime::block_on(
        settings.read_or_default("core.general"),
    ));
    let handle = app.clone();
    tauri::async_runtime::spawn(caskade_app::tray::follow_settings(
        settings.clone(),
        move |general| handle.state::<Tray>().follow(general),
    ));
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        let console = crate::commands::console::is_on(&handle);
        match handle.state::<Tray>().close(console) {
            Close::Go => {}
            Close::Hide => {
                api.prevent_close();
                if let Some(window) = handle.get_webview_window("main")
                    && let Err(error) = window.hide()
                {
                    tracing::warn!(%error, "couldn't hide the main window");
                }
            }
            Close::Held(close) => {
                api.prevent_close();
                // The tray's Quit reaches a hidden or minimized window here,
                // and its question has to be seen.
                main_window::raise(&handle);
                let handle = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(UNANSWERED);
                    if handle.state::<Tray>().unanswered(close) {
                        give_up(&handle);
                    }
                });
            }
        }
    });
}

/// The menu's items, relabelled when the language changes.
struct MenuItems<R: Runtime> {
    show: MenuItem<R>,
    quit: MenuItem<R>,
}

// Not derived: that would ask for `R: Clone`.
impl<R: Runtime> Clone for MenuItems<R> {
    fn clone(&self) -> Self {
        Self {
            show: self.show.clone(),
            quit: self.quit.clone(),
        }
    }
}

impl<R: Runtime> MenuItems<R> {
    fn new(app: &AppHandle<R>) -> tauri::Result<Self> {
        Ok(Self {
            show: MenuItem::with_id(app, SHOW, show_label(), true, None::<&str>)?,
            quit: MenuItem::with_id(app, QUIT, quit_label(), true, None::<&str>)?,
        })
    }

    fn relabel(&self) -> tauri::Result<()> {
        self.show.set_text(show_label())?;
        self.quit.set_text(quit_label())
    }
}

/// Relabels the menu each time the core's language changes, until the
/// core goes away. `changes` comes from before the items were made, so no
/// change falls between the two.
async fn follow_language<R: Runtime>(items: MenuItems<R>, mut changes: watch::Receiver<String>) {
    while changes.changed().await.is_ok() {
        if let Err(error) = items.relabel() {
            tracing::warn!(%error, "couldn't relabel the tray menu");
        }
    }
}

fn icon<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let changes = caskade_core::i18n::subscribe();
    let items = MenuItems::new(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &items.show,
            &PredefinedMenuItem::separator(app)?,
            &items.quit,
        ],
    )?;
    let macos = cfg!(target_os = "macos");
    TrayIconBuilder::with_id("main")
        .icon(ICON)
        .icon_as_template(macos)
        .tooltip("Caskade")
        .menu(&menu)
        // A click opens the menu on macOS, as on any menu bar extra, and
        // shows the window on Windows.
        .show_menu_on_left_click(macos)
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW => main_window::raise(app),
            QUIT => quit(app),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
                && !macos
            {
                main_window::raise(tray.app_handle());
            }
        })
        .build(app)?;
    tauri::async_runtime::spawn(follow_language(items, changes));
    Ok(())
}

/// libappindicator-sys panics on first use when neither library loads, so
/// load it here first, the same way, where the panic can be caught.
#[cfg(target_os = "linux")]
fn indicator_available() -> bool {
    std::panic::catch_unwind(|| {
        let _ = &*libappindicator_sys::LIB;
    })
    .is_ok()
}

#[cfg(not(target_os = "linux"))]
fn indicator_available() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Polls `future` once, on this thread.
    fn poll_once<F: std::future::Future>(future: std::pin::Pin<&mut F>) {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(future.poll(&mut context).is_pending());
    }

    #[test]
    fn the_menu_follows_the_language_without_a_restart() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        assert_eq!(caskade_core::i18n::set_locale("sv"), "sv");
        let changes = caskade_core::i18n::subscribe();
        let items = MenuItems::new(app.handle()).unwrap();
        assert_eq!(items.show.text().unwrap(), "Visa Caskade");
        assert_eq!(items.quit.text().unwrap(), "Avsluta Caskade");

        // The follower runs here, between switches, so the menu is never
        // read while it changes.
        let mut follow = std::pin::pin!(follow_language(items.clone(), changes));
        poll_once(follow.as_mut());
        caskade_core::i18n::set_locale("en");
        poll_once(follow.as_mut());
        assert_eq!(items.show.text().unwrap(), "Show Caskade");
        assert_eq!(items.quit.text().unwrap(), "Quit Caskade");

        caskade_core::i18n::set_locale("sv");
        poll_once(follow.as_mut());
        assert_eq!(items.show.text().unwrap(), "Visa Caskade");
        caskade_core::i18n::set_locale("en");
        poll_once(follow.as_mut());
        assert_eq!(items.quit.text().unwrap(), "Quit Caskade");
    }

    #[test]
    fn the_close_setting_is_followed() {
        let store =
            tauri::async_runtime::block_on(caskade_core::db::LibraryStore::in_memory()).unwrap();
        let settings = SettingsService::new(store);
        settings
            .register_core(&caskade_core::AppPaths::under(std::path::Path::new(
                "fixture",
            )))
            .unwrap();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        attach(app.handle(), &settings);
        let state = app.state::<Tray>();
        state.set_shown(true);
        // Hiding to the tray is the default.
        assert!(state.hides_on_close(false));

        let mut value = tauri::async_runtime::block_on(settings.get("core.general"))
            .unwrap()
            .value;
        value["onClose"] = "quit".into();
        tauri::async_runtime::block_on(settings.set("core.general", value, None)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.hides_on_close(false) {
            assert!(std::time::Instant::now() < deadline, "setting not followed");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
