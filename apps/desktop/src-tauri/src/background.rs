//! A snapshot plus revisioned events: reloads and missed events cannot leave a
//! stale spinner, and fast worker transitions need no UI polling.
use caskade_core::i18n::translated_tasks;
use caskade_core::jobs::{Snapshot, global};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

// Supervision and the panic log are the app layer's.
pub use caskade_app::background::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct BackgroundChanged(pub Snapshot);

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

pub fn bridge<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    let mut updates = global().subscribe();
    caskade_app::runtime::spawn(async move {
        while updates.changed().await.is_ok() {
            // Coalesce bursts from scans/imports. Progress must not flood the
            // webview with a snapshot for every file or game in a large library.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let snapshot = translated_tasks(updates.borrow_and_update().clone());
            if let Err(error) = BackgroundChanged(snapshot).emit(&app) {
                tracing::warn!(%error, "background activity event not delivered");
            }
        }
    });
}
