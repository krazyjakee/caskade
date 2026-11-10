//! A snapshot plus revisioned events: reloads and missed events cannot leave a
//! stale spinner, and fast worker transitions need no UI polling.

// Supervision and the panic log are the app layer's.
pub use caskade_app::background::*;

#[tauri::command]
#[specta::specta]
pub fn background_status<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> AppStatus {
    caskade_app::background::background_status(app.package_info().version.to_string())
}
