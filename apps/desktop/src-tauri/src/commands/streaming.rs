//! Game streaming (STREAMING-PLAN): hosting on and off, and the host's
//! state. Settings → Streaming (#265) is their UI.

use crate::error::{CommandError, report};
use caskade_core::streaming::{
    HostOutcome, HostStatus, StreamingClient, StreamingHost,
    api::{PairRequest, PairedClient},
    hosts::{StreamHost, StreamHosts},
    pairing::Paired,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event;

/// How long a pairing waits for the code to be entered on the host: its
/// requests expire after five minutes.
const PAIRING_WAIT: Duration = Duration::from_secs(5 * 60);

/// A pairing under way: the code to enter on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairingStarted {
    pub host_id: String,
    pub host_name: String,
    pub code: String,
}

/// How a pairing ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PairingOutcome {
    Paired,
    WrongCode,
    Declined,
    Expired,
    Failed,
}

/// `streaming_pair_with` ended; `message` says why in Caskade's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct StreamingPairingFinished {
    pub host_id: String,
    pub outcome: PairingOutcome,
    pub message: Option<String>,
}

/// The outcome and, when it didn't pair, what to tell people.
pub fn pairing_outcome(
    result: &caskade_core::Result<Paired>,
    host: &str,
) -> (PairingOutcome, Option<String>) {
    match result {
        Ok(Paired::Paired { .. }) => (PairingOutcome::Paired, None),
        Ok(Paired::WrongCode) => (
            PairingOutcome::WrongCode,
            Some(caskade_core::tr!(
                "The code entered on {device} didn't match. Try again with a new code.",
                device = host
            )),
        ),
        Ok(Paired::Declined) => (
            PairingOutcome::Declined,
            Some(caskade_core::tr!("{device} declined.", device = host)),
        ),
        Ok(Paired::Expired) => (
            PairingOutcome::Expired,
            Some(caskade_core::tr!(
                "Nobody entered the code on {device} in time. Try again.",
                device = host
            )),
        ),
        Err(error) => (PairingOutcome::Failed, Some(error.to_string())),
    }
}

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

/// Devices waiting to pair with this host; a request expires after five
/// minutes and is gone from the list.
#[tauri::command]
#[specta::specta]
pub async fn streaming_pair_requests(
    host: State<'_, StreamingHost>,
) -> Result<Vec<PairRequest>, CommandError> {
    host.inner().clone().pair_requests().await.map_err(report)
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
    host.inner()
        .clone()
        .accept_pairing(&id, &code, &name)
        .await
        .map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn streaming_decline_pairing(
    id: String,
    host: State<'_, StreamingHost>,
) -> Result<(), CommandError> {
    host.inner()
        .clone()
        .decline_pairing(&id)
        .await
        .map_err(report)
}

/// The devices paired with this host.
#[tauri::command]
#[specta::specta]
pub async fn streaming_paired_devices(
    host: State<'_, StreamingHost>,
) -> Result<Vec<PairedClient>, CommandError> {
    host.inner().clone().paired_devices().await.map_err(report)
}

/// Removes a device's pairing with this host.
#[tauri::command]
#[specta::specta]
pub async fn streaming_unpair(
    uuid: String,
    host: State<'_, StreamingHost>,
) -> Result<(), CommandError> {
    host.inner().clone().unpair(&uuid).await.map_err(report)
}

/// Forgets every device paired with this host: each needs a code again.
#[tauri::command]
#[specta::specta]
pub async fn streaming_unpair_all(host: State<'_, StreamingHost>) -> Result<(), CommandError> {
    host.inner().clone().unpair_all().await.map_err(report)
}

/// Re-runs hosting's setup (#266): the program's missing files, its
/// configuration and sign-in, and the administrator step's rules, service
/// or firewall entries that went, behind one prompt.
#[tauri::command]
#[specta::specta]
pub async fn streaming_repair(host: State<'_, StreamingHost>) -> Result<HostOutcome, CommandError> {
    host.inner().clone().repair().await.map_err(report)
}

/// Which streaming program: the one hosting here, or the one streaming to
/// this device. People see neither's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum StreamingPart {
    Host,
    Client,
}

impl StreamingPart {
    fn tool(self) -> &'static str {
        match self {
            Self::Host => caskade_core::streaming::host::TOOL,
            Self::Client => caskade_core::streaming::client::TOOL,
        }
    }
}

/// A streaming program installed here, and whether a newer one waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamingProgram {
    pub part: StreamingPart,
    /// What it is, in Caskade's words.
    pub label: String,
    pub installed: Option<String>,
    pub latest: Option<String>,
    /// A newer version was found and not skipped.
    pub update: bool,
}

/// The streaming programs installed here (#266), for Update and Skip.
#[tauri::command]
#[specta::specta]
pub async fn streaming_programs(
    tools: State<'_, caskade_core::managed::ManagedTools>,
) -> Result<Vec<StreamingProgram>, CommandError> {
    let mut programs = Vec::new();
    for part in [StreamingPart::Host, StreamingPart::Client] {
        if let Some(managed) = tools.managed(part.tool()).await.map_err(report)? {
            programs.push(StreamingProgram {
                part,
                label: caskade_core::managed::tool_label(part.tool()),
                update: managed.update_available(),
                installed: managed
                    .installed
                    .as_deref()
                    .map(caskade_core::managed::version_label),
                latest: managed
                    .latest
                    .as_deref()
                    .map(caskade_core::managed::version_label),
            });
        }
    }
    Ok(programs)
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
    match part {
        StreamingPart::Host => host.inner().clone().update().await.map_err(report),
        StreamingPart::Client => client
            .inner()
            .clone()
            .update(&caskade_core::managed::CancellationToken::new())
            .await
            .map(|()| HostOutcome::On)
            .map_err(report),
    }
}

/// Stops offering the newest version of a streaming program; a newer one
/// is offered again.
#[tauri::command]
#[specta::specta]
pub async fn streaming_skip(
    part: StreamingPart,
    tools: State<'_, caskade_core::managed::ManagedTools>,
) -> Result<(), CommandError> {
    tools
        .skip(part.tool(), None)
        .await
        .map(|_| ())
        .map_err(report)
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
    let host = hosts
        .hosts()
        .into_iter()
        .find(|host| host.id.eq_ignore_ascii_case(&host_id))
        .ok_or_else(|| {
            report(caskade_core::Error::Invalid(caskade_core::tr!(
                "that device isn't on the network"
            )))
        })?;
    let address = host.address.clone().ok_or_else(|| {
        report(caskade_core::Error::Invalid(caskade_core::tr!(
            "that device isn't on the network"
        )))
    })?;
    let code = caskade_core::streaming::pairing::new_code().map_err(report)?;
    let started = PairingStarted {
        host_id: host.id.clone(),
        host_name: host.name.clone(),
        code: code.clone(),
    };
    let (client, hosts) = (client.inner().clone(), hosts.inner().clone());
    tauri::async_runtime::spawn(async move {
        let result = client
            .pair(
                &address,
                &code,
                PAIRING_WAIT,
                &caskade_core::managed::CancellationToken::new(),
            )
            .await;
        let (outcome, message) = pairing_outcome(&result, &host.name);
        let _ = hosts.refresh().await;
        let _ = StreamingPairingFinished {
            host_id: host.id,
            outcome,
            message,
        }
        .emit(&app);
    });
    Ok(started)
}

/// Forgets a paired device on this device.
#[tauri::command]
#[specta::specta]
pub async fn streaming_forget_host(
    host_id: String,
    client: State<'_, StreamingClient>,
    hosts: State<'_, StreamHosts>,
) -> Result<(), CommandError> {
    client.inner().forget(&host_id).await.map_err(report)?;
    hosts.inner().refresh().await.map_err(report)
}

/// Looks for devices to stream from while Caskade runs (#262).
pub fn watch_hosts<R: Runtime>(app: &AppHandle<R>, hosts: StreamHosts, client: StreamingClient) {
    app.manage(hosts.clone());
    app.manage(client);
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
