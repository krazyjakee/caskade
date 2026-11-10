//! The app layer's events (`caskade_app::events`) to the webview. Each one
//! goes out under its typed tauri-specta event, a transparent wrapper with
//! the app type's name, so the bindings name the app layer's types.

use caskade_app::events::{self as app, Event as AppEvent};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

pub use caskade_app::events::{LIBRARY_WINDOW, LibraryChange};

/// A webview event for each app layer event type, named as it is.
macro_rules! webview_events {
    ($($name:ident),* $(,)?) => {
        $(
            #[doc = concat!("`caskade_app::events::", stringify!($name), "` for the webview.")]
            #[derive(Debug, Clone, Serialize, Deserialize, Event)]
            #[serde(transparent)]
            pub struct $name(pub app::$name);

            // The app type itself, so the bindings name it and nothing else.
            impl Type for $name {
                fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
                    <app::$name as Type>::definition(types)
                }
            }
        )*

        /// Sends one app layer event to the webview.
        fn emit<R: tauri::Runtime>(
            app: &tauri::AppHandle<R>,
            event: AppEvent,
        ) -> tauri::Result<()> {
            match event {
                $(AppEvent::$name(event) => $name(event).emit(app),)*
            }
        }
    };
}

webview_events!(
    GameStarted,
    GameExited,
    LibraryChanged,
    JobChanged,
    SettingsChanged,
    EmulationChanged,
    EmulationDetectChanged,
    EmulationScanChanged,
    EmulationSetupChanged,
    PluginDisabled,
    PluginsChanged,
    PluginReloaded,
    AccountChanged,
    LifecycleScriptsRan,
    AchievementsChanged,
    BackgroundChanged,
    RommDownloadChanged,
    DiscoveryChanged,
    NotificationsChanged,
    BackupChanged,
    ThemeReloaded,
    PlayingElsewhereChanged,
    RemotePlayRequested,
    StoreAccountsChanged,
    AddonProgress,
    SharesChanged,
    DevicesChanged,
    PairingLinkOpened,
    InstallRequested,
    InstallRefused,
    StreamingPairingFinished,
    LocaleChanged,
    SystemStatsSampled,
    AppUpdateChanged,
    InputActionEvent,
    Hello,
    GamesRefreshChanged,
    SaveVersionsChanged,
);

/// The app layer's [`app::Emit`] sink for this app's webview.
pub struct Webview<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> app::Emit for Webview<R> {
    fn emit(&self, event: AppEvent) -> Result<(), String> {
        emit(&self.0, event).map_err(|error| error.to_string())
    }
}

/// The app layer's events for `app`'s webview.
pub fn sink<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> app::Events {
    app::Events::new(Webview(app))
}

/// Forwards the bus stream to the webview until the bus is gone
/// (`caskade_app::events::bridge`).
pub fn bridge<R: tauri::Runtime>(app: tauri::AppHandle<R>, bus: &caskade_core::bus::Bus) {
    app::bridge(sink(app), bus);
}
