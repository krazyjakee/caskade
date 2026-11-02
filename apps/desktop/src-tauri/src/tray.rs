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

use crate::main_window;
use caskade_core::settings::{GeneralSettings, SettingsService};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, Runtime};
use tokio::sync::watch;

const SHOW: &str = "tray.show";
const QUIT: &str = "tray.quit";

#[cfg(target_os = "macos")]
const ICON: Image<'_> = tauri::include_image!("icons/tray-template.png");
#[cfg(not(target_os = "macos"))]
const ICON: Image<'_> = tauri::include_image!("icons/tray.png");

/// Whether a close hides the main window or lets it close.
#[derive(Debug, Default)]
pub struct Tray {
    /// The icon is up.
    shown: AtomicBool,
    /// `core.general.onClose` is `tray`.
    close_to_tray: AtomicBool,
    /// Something asked to quit: closes go through from now on.
    quitting: AtomicBool,
}

impl Tray {
    /// The icon is up, so a hidden window can be brought back from it.
    pub fn shown(&self) -> bool {
        self.shown.load(Ordering::Relaxed)
    }

    /// Whether a close of the main window hides it instead.
    pub fn hides_on_close(&self, console: bool) -> bool {
        self.shown()
            && self.close_to_tray.load(Ordering::Relaxed)
            && !self.quitting.load(Ordering::Relaxed)
            && !console
    }

    /// Tests: as if the icon were up.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn set_shown(&self, shown: bool) {
        self.shown.store(shown, Ordering::Relaxed);
    }

    /// Lets the next close of the main window quit.
    pub fn quit(&self) {
        self.quitting.store(true, Ordering::Relaxed);
    }

    fn follow(&self, settings: &GeneralSettings) {
        self.close_to_tray
            .store(settings.on_close == "tray", Ordering::Relaxed);
    }
}

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
        Ok(()) => app.state::<Tray>().shown.store(true, Ordering::Relaxed),
        Err(error) => tracing::warn!(%error, "no tray icon; closing the window quits"),
    }
}

/// The app's run events: on macOS, a click on the Dock icon while the
/// window is in the tray shows it.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: RunEvent) {
    #[cfg(target_os = "macos")]
    if let RunEvent::Reopen {
        has_visible_windows: false,
        ..
    } = event
    {
        main_window::raise(app);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, event);
}

/// Everything but the icon: the state, the setting and the close handler.
pub fn attach<R: Runtime>(app: &AppHandle<R>, settings: &SettingsService) {
    app.manage(Tray::default());
    app.state::<Tray>()
        .follow(&tauri::async_runtime::block_on(general(settings)));
    tauri::async_runtime::spawn({
        let app = app.clone();
        let settings = settings.clone();
        let mut changes = settings.subscribe();
        async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match changes.recv().await {
                    Ok(changed) if changed.namespace == "core.general" => {}
                    Ok(_) | Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
                app.state::<Tray>().follow(&general(&settings).await);
            }
        }
    });
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        let console = crate::commands::console::is_on(&handle);
        if !handle.state::<Tray>().hides_on_close(console) {
            return;
        }
        api.prevent_close();
        if let Some(window) = handle.get_webview_window("main")
            && let Err(error) = window.hide()
        {
            tracing::warn!(%error, "couldn't hide the main window");
        }
    });
}

async fn general(service: &SettingsService) -> GeneralSettings {
    service.read_or_default("core.general").await
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

fn show_label() -> String {
    caskade_core::tr!("Show Caskade")
}

fn quit_label() -> String {
    caskade_core::tr!("Quit Caskade")
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

    fn tray(shown: bool, on_close: &str) -> Tray {
        let tray = Tray::default();
        tray.shown.store(shown, Ordering::Relaxed);
        tray.follow(&GeneralSettings {
            on_close: on_close.into(),
            ..GeneralSettings::default()
        });
        tray
    }

    #[test]
    fn a_close_hides_only_to_a_tray_icon_that_is_up() {
        assert!(tray(true, "tray").hides_on_close(false));
        assert!(!tray(true, "quit").hides_on_close(false));
        assert!(!tray(false, "tray").hides_on_close(false));
    }

    #[test]
    fn quitting_and_console_mode_close_for_good() {
        let tray = tray(true, "tray");
        assert!(!tray.hides_on_close(true));
        tray.quit();
        assert!(!tray.hides_on_close(false));
    }

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
        state.shown.store(true, Ordering::Relaxed);
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
