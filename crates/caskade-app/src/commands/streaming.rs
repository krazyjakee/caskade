//! Game streaming: hosting on and off, and the host's
//! state. Settings → Streaming (#265) is their UI.

use crate::error::{CommandError, report};
use caskade_core::settings::{SettingsService, SettingsSnapshot, StreamQuality};
use caskade_streaming::{
    HostOutcome, HostStatus, StreamingClient, StreamingHost,
    api::{PairRequest, PairedClient},
    hosts::{StreamHost, StreamHosts},
    pairing::Paired,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{Event, Events};
use std::time::Duration;
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
    /// The host is one of this user's devices paired through sync, and it
    /// entered the code itself (#267): nothing to type.
    pub automatic: bool,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
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
pub fn streaming_host_status(host: &StreamingHost) -> HostStatus {
    host.status()
}

/// Lets other devices stream games from this one, or stops it. Switching on
/// downloads and sets up the host with one administrator prompt; switching
/// off removes it with one. A dismissed prompt changes nothing.
pub async fn streaming_set_hosting(
    on: bool,
    host: &StreamingHost,
) -> Result<HostOutcome, CommandError> {
    let host = host.clone();
    // Boxed: the host's setup is a deep future, and a shell's command
    // around it would overflow the compiler's layout limit.
    let outcome = if on {
        Box::pin(host.enable()).await
    } else {
        Box::pin(host.disable()).await
    };
    outcome.map_err(report)
}

/// Sets how games streamed to this device look. Any account may: unlike
/// the hosting beside it in `core.streaming`, which is an admin's (#66).
pub async fn streaming_set_quality(
    settings: &SettingsService,
    quality: StreamQuality,
) -> Result<SettingsSnapshot, CommandError> {
    settings.set_stream_quality(&quality).await.map_err(report)
}

/// Hosts through the streaming someone already set up on this device
/// (after `streaming_set_hosting` answered `existingFound`), with its web
/// user name and password, which go to the keychain once they work. Nothing
/// is installed, and only Caskade's own games change in it.
pub async fn streaming_use_existing(
    user: String,
    password: String,
    host: &StreamingHost,
) -> Result<HostOutcome, CommandError> {
    let host = host.clone();
    host.use_existing(caskade_streaming::secrets::Credentials { user, password })
        .await
        .map_err(report)
}

/// The devices this one can stream from: found on the network, added by
/// address or paired, each ready, busy, asleep or unreachable.
pub fn streaming_hosts(hosts: &StreamHosts) -> Vec<StreamHost> {
    hosts.hosts()
}

/// Looks for devices now, and asks the paired ones for their games.
pub async fn streaming_look_again(hosts: &StreamHosts) -> Result<Vec<StreamHost>, CommandError> {
    let hosts = hosts.clone();
    hosts.look_again().await.map_err(report)?;
    Ok(hosts.hosts())
}

/// Adds a device to stream from by its address (a name or an IP), for
/// networks where it isn't found by itself, and looks for it at once.
pub async fn streaming_add_host(
    address: String,
    hosts: &StreamHosts,
) -> Result<Vec<StreamHost>, CommandError> {
    let hosts = hosts.clone();
    hosts.add_address(&address).await.map_err(report)?;
    hosts.look_again().await.map_err(report)?;
    Ok(hosts.hosts())
}

/// Devices waiting to pair with this host; a request expires after five
/// minutes and is gone from the list.
pub async fn streaming_pair_requests(
    host: &StreamingHost,
) -> Result<Vec<PairRequest>, CommandError> {
    host.clone().pair_requests().await.map_err(report)
}

/// Enters the code a device shows on this host, naming the device.
pub async fn streaming_accept_pairing(
    id: String,
    code: String,
    name: String,
    host: &StreamingHost,
) -> Result<(), CommandError> {
    host.clone()
        .accept_pairing(&id, &code, &name)
        .await
        .map_err(report)
}

pub async fn streaming_decline_pairing(
    id: String,
    host: &StreamingHost,
) -> Result<(), CommandError> {
    host.clone().decline_pairing(&id).await.map_err(report)
}

/// The devices paired with this host.
pub async fn streaming_paired_devices(
    host: &StreamingHost,
) -> Result<Vec<PairedClient>, CommandError> {
    host.clone().paired_devices().await.map_err(report)
}

/// Removes a device's pairing with this host.
pub async fn streaming_unpair(uuid: String, host: &StreamingHost) -> Result<(), CommandError> {
    host.clone().unpair(&uuid).await.map_err(report)
}

/// Forgets every device paired with this host: each needs a code again.
pub async fn streaming_unpair_all(host: &StreamingHost) -> Result<(), CommandError> {
    host.clone().unpair_all().await.map_err(report)
}

/// Re-runs hosting's setup (#266): the program's missing files, its
/// configuration and sign-in, and the administrator step's rules, service
/// or firewall entries that went, behind one prompt.
pub async fn streaming_repair(host: &StreamingHost) -> Result<HostOutcome, CommandError> {
    host.clone().repair().await.map_err(report)
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
            Self::Host => caskade_streaming::host::TOOL,
            Self::Client => caskade_streaming::client::TOOL,
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
pub async fn streaming_programs(
    tools: &caskade_core::managed::ManagedTools,
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
pub async fn streaming_update(
    part: StreamingPart,
    host: &StreamingHost,
    client: &StreamingClient,
) -> Result<HostOutcome, CommandError> {
    match part {
        StreamingPart::Host => host.clone().update().await.map_err(report),
        StreamingPart::Client => client
            .clone()
            .update(&caskade_core::managed::CancellationToken::new())
            .await
            .map(|()| HostOutcome::On)
            .map_err(report),
    }
}

/// Stops offering the newest version of a streaming program; a newer one
/// is offered again.
pub async fn streaming_skip(
    part: StreamingPart,
    tools: &caskade_core::managed::ManagedTools,
) -> Result<(), CommandError> {
    tools
        .skip(part.tool(), None)
        .await
        .map(|_| ())
        .map_err(report)
}

/// The OS's name for this computer.
fn sysinfo_host_name() -> Option<String> {
    caskade_streaming::pairing::this_host_name()
}

/// Forgets a paired device on this device.
pub async fn streaming_forget_host(
    host_id: String,
    client: &StreamingClient,
    hosts: &StreamHosts,
) -> Result<(), CommandError> {
    client.forget(&host_id).await.map_err(report)?;
    hosts.refresh().await.map_err(report)
}

/// Pairs this device with host `host_id`: the answer is the code to enter
/// on the host; `StreamingPairingFinished` follows on `events`. `linked` is
/// pairing through sync (#267), when paired devices are set up.
pub async fn streaming_pair_with(
    events: Events,
    linked: Option<&caskade_streaming::linked::LinkedPairing>,
    hosts: &StreamHosts,
    client: &StreamingClient,
    host_id: &str,
) -> Result<PairingStarted, CommandError> {
    let host = hosts
        .hosts()
        .into_iter()
        .find(|host| host.id.eq_ignore_ascii_case(host_id))
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
    let code = caskade_streaming::pairing::new_code().map_err(report)?;
    // Devices paired through sync (#267) take the code without typing.
    let linked = linked.cloned();
    let name = match &linked {
        Some(linked) => linked.device_name().await,
        None => caskade_streaming::pairing::device_name(&sysinfo_host_name().unwrap_or_default()),
    };
    let (client, hosts) = (client.clone(), hosts.clone());
    let pairing = {
        let (address, code, name) = (address.clone(), code.clone(), name.clone());
        let host = host.clone();
        crate::runtime::spawn(async move {
            let result = client
                .pair_as(
                    &address,
                    &code,
                    &name,
                    PAIRING_WAIT,
                    &caskade_core::managed::CancellationToken::new(),
                )
                .await;
            let (outcome, message) = pairing_outcome(&result, &host.name);
            events.send(Event::StreamingPairingFinished(StreamingPairingFinished {
                host_id: host.id,
                outcome,
                message,
            }));
            // Fetching the host's games can take 25 s, too long to leave
            // someone on "Waiting for…" after the host said yes.
            let _ = hosts.look_again().await;
        })
    };
    drop(pairing);
    let automatic = match linked {
        Some(linked) => linked.hand_over(&host.id, &name, &code, &name).await,
        None => false,
    };
    Ok(PairingStarted {
        host_id: host.id.clone(),
        host_name: host.name.clone(),
        code,
        automatic,
    })
}

/// Keeps the streaming hosts on the network up to date.
pub fn watch_hosts(hosts: StreamHosts) {
    crate::background::supervise("streaming hosts", move || hosts.watch());
}

/// Runs the host at start when hosting is on, and keeps the library's
/// games on it (#259) as the library and the signed-in account change.
pub fn setup(
    host: StreamingHost,
    store: caskade_core::db::LibraryStore,
    accounts: Option<caskade_core::accounts::AccountService>,
    bus: &caskade_core::bus::Bus,
    artwork: Option<caskade_core::artwork::ArtworkCache>,
    paths: &caskade_core::AppPaths,
) {
    match caskade_streaming::publish::Publisher::new(
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
    crate::runtime::spawn(async move { host.start().await });
}
