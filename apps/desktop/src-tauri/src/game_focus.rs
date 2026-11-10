//! Making way for a game (`caskade_app::game_focus`): the main window, as
//! the app layer moves it when games start and exit.

use caskade_app::game_focus::{self as app, GameWindow, Making};
use caskade_core::{bus::Bus, db::LibraryStore, launch::LaunchService, settings::SettingsService};
use tauri::{AppHandle, Manager, Runtime};

/// The main window, to [`app::follow`].
struct MainWindow<R: Runtime>(AppHandle<R>);

impl<R: Runtime> GameWindow for MainWindow<R> {
    fn present(&self) -> bool {
        self.0.get_webview_window("main").is_some()
    }

    fn cover(&self, on: bool) -> Result<(), String> {
        let Some(window) = self.0.get_webview_window("main") else {
            return Ok(());
        };
        window
            .set_always_on_top(on)
            .map_err(|error| error.to_string())
    }

    fn make_way(&self, way: Making) -> Result<(), String> {
        let Some(window) = self.0.get_webview_window("main") else {
            return Ok(());
        };
        match way {
            Making::Minimize => window.minimize(),
            Making::Hide => window.hide(),
            Making::Stay | Making::Fade => Ok(()),
        }
        .map_err(|error| error.to_string())
    }

    fn raise(&self) {
        crate::main_window::raise(&self.0);
    }

    fn tray_shown(&self) -> bool {
        self.0
            .try_state::<crate::tray::Tray>()
            .is_some_and(|tray| tray.shown())
    }
}

/// Follows game sessions until the bus is gone.
pub fn follow<R: Runtime>(
    app: AppHandle<R>,
    bus: &Bus,
    settings: SettingsService,
    store: LibraryStore,
    launches: LaunchService,
) {
    app::follow(MainWindow(app), bus, settings, store, launches);
}
