//! Game streaming: hosting on and off, and the host's
//! state. Settings → Streaming (#265) is their UI.
pub use app::*;
use caskade_app::commands::streaming as app;

use crate::error::CommandError;
use caskade_core::settings::{SettingsService, SettingsSnapshot, StreamQuality};
use caskade_streaming::{
    HostOutcome, HostStatus, StreamingClient, StreamingHost,
    api::{PairRequest, PairedClient},
    hosts::{StreamHost, StreamHosts},
};
use tauri::{AppHandle, Manager, Runtime, State};
/// The host's state: off, setting up, starting, ready or needing attention.
#[tauri::command]
#[specta::specta]
pub fn streaming_host_status(host: State<'_, StreamingHost>) -> HostStatus {
    app::streaming_host_status(&host)
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
    app::streaming_set_hosting(on, &host).await
}

/// Sets how games streamed to this device look. Any account may: unlike
/// the hosting beside it in `core.streaming`, which is an admin's (#66).
#[tauri::command]
#[specta::specta]
pub async fn streaming_set_quality(
    settings: State<'_, SettingsService>,
    quality: StreamQuality,
) -> Result<SettingsSnapshot, CommandError> {
    app::streaming_set_quality(&settings, quality).await
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
    app::streaming_use_existing(user, password, &host).await
}

/// The devices this one can stream from: found on the network, added by
/// address or paired, each ready, busy, asleep or unreachable.
#[tauri::command]
#[specta::specta]
pub fn streaming_hosts(hosts: State<'_, StreamHosts>) -> Vec<StreamHost> {
    app::streaming_hosts(&hosts)
}

/// Looks for devices now, and asks the paired ones for their games.
#[tauri::command]
#[specta::specta]
pub async fn streaming_look_again(
    hosts: State<'_, StreamHosts>,
) -> Result<Vec<StreamHost>, CommandError> {
    app::streaming_look_again(&hosts).await
}

/// Adds a device to stream from by its address (a name or an IP), for
/// networks where it isn't found by itself, and looks for it at once.
#[tauri::command]
#[specta::specta]
pub async fn streaming_add_host(
    address: String,
    hosts: State<'_, StreamHosts>,
) -> Result<Vec<StreamHost>, CommandError> {
    app::streaming_add_host(address, &hosts).await
}

/// Devices waiting to pair with this host; a request expires after five
/// minutes and is gone from the list.
#[tauri::command]
#[specta::specta]
pub async fn streaming_pair_requests(
    host: State<'_, StreamingHost>,
) -> Result<Vec<PairRequest>, CommandError> {
    app::streaming_pair_requests(&host).await
}

/// Enters the code a device shows on this host, naming the device.
#[tauri::command]
#[specta::specta]
pub async fn streaming_accept_pairing(
    id: String,
    code: String,
    name: String,
    host: State<'_, StreamingHost>,
) -> Result<(), CommandError> {
    app::streaming_accept_pairing(id, code, name, &host).await
}

#[tauri::command]
#[specta::specta]
pub async fn streaming_decline_pairing(
    id: String,
    host: State<'_, StreamingHost>,
) -> Result<(), CommandError> {
    app::streaming_decline_pairing(id, &host).await
}

/// The devices paired with this host.
#[tauri::command]
#[specta::specta]
pub async fn streaming_paired_devices(
    host: State<'_, StreamingHost>,
) -> Result<Vec<PairedClient>, CommandError> {
    app::streaming_paired_devices(&host).await
}

/// Removes a device's pairing with this host.
#[tauri::command]
#[specta::specta]
pub async fn streaming_unpair(
    uuid: String,
    host: State<'_, StreamingHost>,
) -> Result<(), CommandError> {
    app::streaming_unpair(uuid, &host).await
}

/// Forgets every device paired with this host: each needs a code again.
#[tauri::command]
#[specta::specta]
pub async fn streaming_unpair_all(host: State<'_, StreamingHost>) -> Result<(), CommandError> {
    app::streaming_unpair_all(&host).await
}

/// Re-runs hosting's setup (#266): the program's missing files, its
/// configuration and sign-in, and the administrator step's rules, service
/// or firewall entries that went, behind one prompt.
#[tauri::command]
#[specta::specta]
pub async fn streaming_repair(host: State<'_, StreamingHost>) -> Result<HostOutcome, CommandError> {
    app::streaming_repair(&host).await
}

/// The streaming programs installed here (#266), for Update and Skip.
#[tauri::command]
#[specta::specta]
pub async fn streaming_programs(
    tools: State<'_, caskade_core::managed::ManagedTools>,
) -> Result<Vec<StreamingProgram>, CommandError> {
    app::streaming_programs(&tools).await
}

/// Installs the newest version of a streaming program (#266), never during
/// a stream: hosting restarts with its pairings and settings, and one
/// administrator prompt where Windows needs the new copy put in place.
#[tauri::command]
#[specta::specta]
pub async fn streaming_update(
    part: StreamingPart,
    host: State<'_, StreamingHost>,
    client: State<'_, StreamingClient>,
) -> Result<HostOutcome, CommandError> {
    app::streaming_update(part, &host, &client).await
}

/// Stops offering the newest version of a streaming program; a newer one
/// is offered again.
#[tauri::command]
#[specta::specta]
pub async fn streaming_skip(
    part: StreamingPart,
    tools: State<'_, caskade_core::managed::ManagedTools>,
) -> Result<(), CommandError> {
    app::streaming_skip(part, &tools).await
}

/// Pairs this device with host `host_id`: the answer is the code to enter
/// on the host; `StreamingPairingFinished` follows.
#[tauri::command]
#[specta::specta]
pub async fn streaming_pair_with<R: Runtime>(
    app: AppHandle<R>,
    host_id: String,
    hosts: State<'_, StreamHosts>,
    client: State<'_, StreamingClient>,
) -> Result<PairingStarted, CommandError> {
    app::streaming_pair_with(
        crate::events::sink(app.clone()),
        app.try_state::<caskade_streaming::linked::LinkedPairing>()
            .as_deref(),
        &hosts,
        &client,
        &host_id,
    )
    .await
}

/// Forgets a paired device on this device.
#[tauri::command]
#[specta::specta]
pub async fn streaming_forget_host(
    host_id: String,
    client: State<'_, StreamingClient>,
    hosts: State<'_, StreamHosts>,
) -> Result<(), CommandError> {
    app::streaming_forget_host(host_id, &client, &hosts).await
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
