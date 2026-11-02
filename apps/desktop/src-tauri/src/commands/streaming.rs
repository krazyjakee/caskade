//! Game streaming (STREAMING-PLAN): hosting on and off, and the host's
//! state. Settings → Streaming (#265) is their UI.

use crate::error::{CommandError, report};
use caskade_core::streaming::{HostOutcome, HostStatus, StreamingHost};
use tauri::{AppHandle, Manager, Runtime, State};

/// The host's state: off, setting up, starting, ready or needing attention.
#[tauri::command]
#[specta::specta]
pub fn streaming_host_status(host: State<'_, StreamingHost>) -> HostStatus {
    host.status()
}

/// Lets other devices stream games from this one, or stops it. Switching on
/// downloads and sets up the host with one administrator prompt; switching
/// off removes it with one. A dismissed prompt changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn streaming_set_hosting(
    on: bool,
    host: State<'_, StreamingHost>,
) -> Result<HostOutcome, CommandError> {
    let host = host.inner().clone();
    let outcome = if on {
        host.enable().await
    } else {
        host.disable().await
    };
    outcome.map_err(report)
}

/// Runs the host at start when hosting is on.
pub fn setup<R: Runtime>(app: &AppHandle<R>, host: StreamingHost) {
    app.manage(host.clone());
    tauri::async_runtime::spawn(async move { host.start().await });
}

/// When Caskade quits, the host stops with it. Closing the window to the
/// tray isn't quitting, so the host keeps running then.
pub fn on_exit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(host) = app.try_state::<StreamingHost>() {
        host.shutdown();
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "streaming_tests.rs"]
mod tests;
