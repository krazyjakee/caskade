//! Game streaming (STREAMING-PLAN): hosting on and off, and the host's
//! state. Settings → Streaming (#265) is their UI.

use crate::error::{CommandError, report};
use caskade_core::streaming::{
    HostOutcome, HostStatus, StreamingHost,
    hosts::{StreamHost, StreamHosts},
};
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

/// Hosts through the streaming someone already set up on this device
/// (after `streaming_set_hosting` answered `existingFound`), with its web
/// user name and password, which go to the keychain once they work. Nothing
/// is installed, and only Caskade's own games change in it.
#[tauri::command]
#[specta::specta]
pub async fn streaming_use_existing(
    user: String,
    password: String,
    host: State<'_, StreamingHost>,
) -> Result<HostOutcome, CommandError> {
    let host = host.inner().clone();
    host.use_existing(caskade_core::streaming::secrets::Credentials { user, password })
        .await
        .map_err(report)
}

/// The devices this one can stream from: found on the network, added by
/// address or paired, each ready, busy, asleep or unreachable.
#[tauri::command]
#[specta::specta]
pub fn streaming_hosts(hosts: State<'_, StreamHosts>) -> Vec<StreamHost> {
    hosts.hosts()
}

/// Adds a device to stream from by its address (a name or an IP), for
/// networks where it isn't found by itself, and looks for it at once.
#[tauri::command]
#[specta::specta]
pub async fn streaming_add_host(
    address: String,
    hosts: State<'_, StreamHosts>,
) -> Result<Vec<StreamHost>, CommandError> {
    let hosts = hosts.inner().clone();
    hosts.add_address(&address).await.map_err(report)?;
    hosts.refresh().await.map_err(report)?;
    Ok(hosts.hosts())
}

/// Looks for devices to stream from while Caskade runs (#262).
pub fn watch_hosts<R: Runtime>(app: &AppHandle<R>, hosts: StreamHosts) {
    app.manage(hosts.clone());
    crate::background::supervise("streaming hosts", move || hosts.watch());
}

/// Runs the host at start when hosting is on, and keeps the library's
/// games on it (#259) as the library and the signed-in account change.
pub fn setup<R: Runtime>(
    app: &AppHandle<R>,
    host: StreamingHost,
    store: caskade_core::db::LibraryStore,
    bus: &caskade_core::bus::Bus,
    artwork: Option<caskade_core::artwork::ArtworkCache>,
    paths: &caskade_core::AppPaths,
) {
    app.manage(host.clone());
    let accounts = app
        .try_state::<caskade_core::accounts::AccountService>()
        .map(|accounts| accounts.inner().clone());
    match caskade_core::streaming::publish::Publisher::new(
        store,
        std::sync::Arc::new(host.clone()),
        accounts,
        artwork,
        paths,
    ) {
        Ok(publisher) => {
            let bus = bus.clone();
            crate::background::supervise("streaming games", move || publisher.watch(&bus));
        }
        Err(error) => tracing::warn!(%error, "games won't be offered for streaming"),
    }
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
