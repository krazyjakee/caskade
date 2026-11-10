//! A snapshot plus revisioned events: reloads and missed events cannot leave a
//! stale spinner, and fast worker transitions need no UI polling.
use caskade_core::i18n::translated_tasks;
use caskade_core::jobs::{Snapshot, global};
use serde::{Deserialize, Serialize};
use specta::Type;

// Supervision and the panic log are the app layer's.
pub use caskade_app::background::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub version: String,
    pub background: Snapshot,
}

#[tauri::command]
#[specta::specta]
pub fn background_status<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> AppStatus {
    AppStatus {
        version: app.package_info().version.to_string(),
        background: translated_tasks(global().snapshot()),
    }
}

/// Sends Activity's background snapshot to the webview as it changes.
pub fn bridge<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    caskade_app::events::bridge_background(crate::events::sink(app));
}
