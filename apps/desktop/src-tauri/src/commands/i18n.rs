//! The app's language (#213). The core owns it: it follows the Language
//! setting (`core.general.language`, `system` by default) and resolves it
//! to a language it ships, else English. The webview asks for it before
//! its first render ([`locale_current`]) and follows [`LocaleChanged`], so
//! the two never disagree; the tray follows the core by itself.

use caskade_core::i18n;
use caskade_core::settings::SettingsService;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Runtime};
use tauri_specta::Event as _;
use tokio::sync::watch;

/// The core's language changed: the webview loads its catalogs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct LocaleChanged {
    /// The language in use: a BCP 47 tag, `en` without a catalog.
    pub locale: String,
}

/// The language in use, and what the Language setting can offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LocaleInfo {
    /// The language in use.
    pub locale: String,
    /// What `system` resolves to: the OS's language, or English when
    /// Caskade doesn't ship it.
    pub system: String,
    /// English, then every language with a catalog.
    pub available: Vec<String>,
}

/// Switches to the Language setting before the window loads, then follows
/// it and tells the webview of each change.
pub fn setup<R: Runtime>(app: &AppHandle<R>, settings: &SettingsService) {
    let locale = tauri::async_runtime::block_on(i18n::apply_setting(settings));
    tracing::info!(%locale, "language");
    // From here on: the webview asks for the language it starts in.
    let changes = i18n::subscribe();
    tauri::async_runtime::spawn(i18n::follow_setting(settings.clone()));
    tauri::async_runtime::spawn(announce(app.clone(), changes));
}

/// Emits [`LocaleChanged`] for each change of `changes`, until the core
/// goes away.
pub async fn announce<R: Runtime>(app: AppHandle<R>, mut changes: watch::Receiver<String>) {
    while changes.changed().await.is_ok() {
        let locale = changes.borrow_and_update().clone();
        if let Err(error) = (LocaleChanged { locale }).emit(&app) {
            tracing::warn!(%error, "couldn't tell the webview of the language");
        }
    }
}

/// The language in use, what System means here, and the languages there
/// are (the webview's first render).
#[tauri::command]
#[specta::specta]
pub fn locale_current() -> LocaleInfo {
    LocaleInfo {
        locale: i18n::locale(),
        system: i18n::resolve(i18n::SYSTEM),
        available: i18n::available().into_iter().map(String::from).collect(),
    }
}

#[cfg(test)]
#[path = "i18n_tests.rs"]
mod tests;
