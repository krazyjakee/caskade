//! The app's language (#213). The core owns it: it follows the Language
//! setting (`core.general.language`, `system` by default) and resolves it
//! to a language it ships, else English. The webview asks for it before
//! its first render ([`locale_current`]) and follows [`LocaleChanged`], so
//! the two never disagree; the tray follows the core by itself.
pub use app::*;
use caskade_app::commands::i18n as app;
// The webview's event, which the app sends as `caskade_app`'s.
pub use crate::events::LocaleChanged;

use caskade_core::settings::SettingsService;
use tauri::{AppHandle, Runtime};
use tokio::sync::watch;
/// Switches to the Language setting before the window loads, then follows
/// it and tells the webview of each change.
pub fn setup<R: Runtime>(app: &AppHandle<R>, settings: &SettingsService) {
    app::setup(crate::events::sink(app.clone()), settings);
}

/// Tells the webview of each change of `changes`, until the core goes away.
pub async fn announce<R: Runtime>(app: AppHandle<R>, changes: watch::Receiver<String>) {
    app::announce(crate::events::sink(app), changes).await;
}

/// The language in use, what System means here, and the languages there
/// are (the webview's first render).
#[tauri::command]
#[specta::specta]
pub fn locale_current() -> LocaleInfo {
    app::locale_current()
}

#[cfg(test)]
#[path = "i18n_tests.rs"]
mod tests;
