//! Which games are running on which paired device (#240): presence from the
//! core's memory, never a wait on the network.
//! Forwarded to `caskade_app::commands::presence`.
use crate::error::CommandError;
use caskade_app::commands::presence as app;
use caskade_core::{db::LibraryStore, devices::Devices, sync::presence::Presence};
use tauri::{AppHandle, Manager, Runtime, State};

pub use app::PlayingElsewhere;

/// The games running on other devices now, by what they last said.
#[tauri::command]
#[specta::specta]
pub async fn devices_playing(
    presence: State<'_, Presence>,
    devices: State<'_, Devices>,
    store: State<'_, LibraryStore>,
) -> Result<Vec<PlayingElsewhere>, CommandError> {
    app::devices_playing(&presence, &devices, &store).await
}

/// Sends what's running elsewhere to the webview whenever it changes, and
/// manages `presence`.
pub fn setup<R: Runtime>(
    app: &AppHandle<R>,
    presence: Presence,
    devices: Devices,
    store: LibraryStore,
) {
    app::setup(crate::events::sink(app.clone()), &presence, devices, store);
    app.manage(presence);
}

#[cfg(test)]
#[path = "presence_tests.rs"]
mod tests;
