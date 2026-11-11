//! Devices asking to stream games from this one (#265, #108): whether it
//! hosts, the requests waiting, and entering a request's code or declining
//! it, through the app layer's commands as the webview's
//! `api/streaming.ts` does. The host behind it is a trait, so flows' tests
//! answer for it without a streaming server.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use caskade_app::commands::streaming;
use caskade_core::accounts::{AccountRole, AccountService};
use caskade_core::error::ErrorReport;
use caskade_streaming::StreamingHost;

/// A device waiting to pair, as the host lists it.
pub use caskade_streaming::api::PairRequest;
/// The host's state, and the streaming types flows show.
pub use caskade_streaming::{HostState, HostStatus};

type Result<T> = std::result::Result<T, ErrorReport>;
type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// What the requests come from: the app's streaming host, or a test's.
pub trait PairRequestHost: Send + Sync + 'static {
    /// Whether it's hosting and answering.
    fn ready(&self) -> bool;
    fn requests(&self) -> Pending<'_, Vec<PairRequest>>;
    fn accept<'a>(&'a self, id: &'a str, code: &'a str, name: &'a str) -> Pending<'a, ()>;
    fn decline<'a>(&'a self, id: &'a str) -> Pending<'a, ()>;
}

impl PairRequestHost for StreamingHost {
    fn ready(&self) -> bool {
        streaming::streaming_host_status(self).state == HostState::Ready
    }

    fn requests(&self) -> Pending<'_, Vec<PairRequest>> {
        Box::pin(streaming::streaming_pair_requests(self))
    }

    fn accept<'a>(&'a self, id: &'a str, code: &'a str, name: &'a str) -> Pending<'a, ()> {
        Box::pin(streaming::streaming_accept_pairing(
            id.to_owned(),
            code.to_owned(),
            name.to_owned(),
            self,
        ))
    }

    fn decline<'a>(&'a self, id: &'a str) -> Pending<'a, ()> {
        Box::pin(streaming::streaming_decline_pairing(id.to_owned(), self))
    }
}

/// What the stream pairing prompt reads and changes.
#[derive(Clone)]
pub struct StreamPairing {
    host: Arc<dyn PairRequestHost>,
    accounts: AccountService,
}

impl StreamPairing {
    pub fn new(host: Arc<dyn PairRequestHost>, accounts: AccountService) -> Self {
        Self { host, accounts }
    }

    /// The running app's; `None` where this system has no streaming host.
    pub fn of(services: &caskade_app::start::Services) -> Option<Self> {
        Some(Self::new(
            Arc::new(services.streaming_host.clone()?),
            services.accounts.clone(),
        ))
    }

    /// Whether this device hosts and the host answers: only then do
    /// requests arrive.
    pub fn hosting(&self) -> bool {
        self.host.ready()
    }

    /// The devices waiting; a request expires after five minutes.
    pub async fn requests(&self) -> Result<Vec<PairRequest>> {
        self.host.requests().await
    }

    /// Enters the code `id` shows, naming the device `name`.
    pub async fn accept(&self, id: &str, code: &str, name: &str) -> Result<()> {
        self.host.accept(id, code, name).await
    }

    pub async fn decline(&self, id: &str) -> Result<()> {
        self.host.decline(id).await
    }

    /// Whether the signed-in account may let devices play: an admin.
    pub async fn is_admin(&self) -> bool {
        match self.accounts.active().await {
            Ok(Some(account)) => account.role == AccountRole::Admin,
            Ok(None) | Err(_) => false,
        }
    }
}

impl PartialEq for StreamPairing {
    /// One host per app: a flow's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

// ── Settings → Streaming (#265, #105) ──

pub use caskade_app::commands::streaming::{
    PairingOutcome, PairingStarted, StreamingPairingFinished, StreamingPart, StreamingProgram,
};
pub use caskade_core::settings::StreamQuality;
pub use caskade_streaming::HostOutcome;
pub use caskade_streaming::api::PairedClient;
pub use caskade_streaming::hosts::{HostReach, StreamHost};

use caskade_app::events::{Event, Events};
use caskade_core::managed::ManagedTools;
use caskade_core::settings::{SettingsService, SettingsSnapshot};
use caskade_streaming::StreamingClient;
use caskade_streaming::hosts::StreamHosts;
use caskade_streaming::linked::LinkedPairing;
use tokio::sync::broadcast;

/// Everything Settings → Streaming reads and does: hosting here, the
/// devices this one streams from, the programs' updates and the quality.
pub trait StreamingBackend: PairRequestHost {
    fn status(&self) -> HostStatus;
    fn set_hosting(&self, on: bool) -> Pending<'_, HostOutcome>;
    fn repair(&self) -> Pending<'_, HostOutcome>;
    fn use_existing<'a>(&'a self, user: &'a str, password: &'a str) -> Pending<'a, HostOutcome>;
    fn paired_devices(&self) -> Pending<'_, Vec<PairedClient>>;
    fn unpair<'a>(&'a self, uuid: &'a str) -> Pending<'a, ()>;
    fn unpair_all(&self) -> Pending<'_, ()>;
    fn programs(&self) -> Pending<'_, Vec<StreamingProgram>>;
    fn update(&self, part: StreamingPart) -> Pending<'_, HostOutcome>;
    fn skip(&self, part: StreamingPart) -> Pending<'_, ()>;
    /// The devices to stream from, as last found.
    fn hosts(&self) -> Vec<StreamHost>;
    fn look_again(&self) -> Pending<'_, Vec<StreamHost>>;
    fn add_host<'a>(&'a self, address: &'a str) -> Pending<'a, Vec<StreamHost>>;
    fn forget_host<'a>(&'a self, id: &'a str) -> Pending<'a, ()>;
    /// Starts pairing with `host_id`; how it ends comes as
    /// [`Event::StreamingPairingFinished`] on [`Self::events`].
    fn pair_with<'a>(&'a self, host_id: &'a str) -> Pending<'a, PairingStarted>;
    fn events(&self) -> broadcast::Receiver<Event>;
    fn set_quality(&self, quality: StreamQuality) -> Pending<'_, SettingsSnapshot>;
}

fn unavailable() -> ErrorReport {
    ErrorReport::from(caskade_core::tr!(
        "This part of Caskade isn't available right now."
    ))
}

fn missing<'a, T: Send + 'a>() -> Pending<'a, T> {
    Box::pin(async { Err(unavailable()) })
}

/// The app's streaming: each part absent where this system has none.
#[derive(Clone)]
pub struct AppStreaming {
    pub host: Option<StreamingHost>,
    pub hosts: Option<StreamHosts>,
    pub client: Option<StreamingClient>,
    pub tools: Option<ManagedTools>,
    pub linked: LinkedPairing,
    pub settings: SettingsService,
    /// Where `pair_with` says how it ended.
    pub sink: Events,
    /// The app's events, for hearing it.
    pub events: broadcast::Sender<Event>,
}

impl PairRequestHost for AppStreaming {
    fn ready(&self) -> bool {
        self.host.as_ref().is_some_and(PairRequestHost::ready)
    }

    fn requests(&self) -> Pending<'_, Vec<PairRequest>> {
        match &self.host {
            Some(host) => host.requests(),
            None => missing(),
        }
    }

    fn accept<'a>(&'a self, id: &'a str, code: &'a str, name: &'a str) -> Pending<'a, ()> {
        match &self.host {
            Some(host) => host.accept(id, code, name),
            None => missing(),
        }
    }

    fn decline<'a>(&'a self, id: &'a str) -> Pending<'a, ()> {
        match &self.host {
            Some(host) => host.decline(id),
            None => missing(),
        }
    }
}

impl StreamingBackend for AppStreaming {
    fn status(&self) -> HostStatus {
        match &self.host {
            Some(host) => streaming::streaming_host_status(host),
            None => HostStatus {
                state: HostState::Unsupported,
                version: None,
                problem: None,
                warning: None,
                unreachable: false,
            },
        }
    }

    fn set_hosting(&self, on: bool) -> Pending<'_, HostOutcome> {
        match &self.host {
            Some(host) => Box::pin(streaming::streaming_set_hosting(on, host)),
            None => missing(),
        }
    }

    fn repair(&self) -> Pending<'_, HostOutcome> {
        match &self.host {
            Some(host) => Box::pin(streaming::streaming_repair(host)),
            None => missing(),
        }
    }

    fn use_existing<'a>(&'a self, user: &'a str, password: &'a str) -> Pending<'a, HostOutcome> {
        match &self.host {
            Some(host) => Box::pin(streaming::streaming_use_existing(
                user.to_owned(),
                password.to_owned(),
                host,
            )),
            None => missing(),
        }
    }

    fn paired_devices(&self) -> Pending<'_, Vec<PairedClient>> {
        match &self.host {
            Some(host) => Box::pin(streaming::streaming_paired_devices(host)),
            None => missing(),
        }
    }

    fn unpair<'a>(&'a self, uuid: &'a str) -> Pending<'a, ()> {
        match &self.host {
            Some(host) => Box::pin(streaming::streaming_unpair(uuid.to_owned(), host)),
            None => missing(),
        }
    }

    fn unpair_all(&self) -> Pending<'_, ()> {
        match &self.host {
            Some(host) => Box::pin(streaming::streaming_unpair_all(host)),
            None => missing(),
        }
    }

    fn programs(&self) -> Pending<'_, Vec<StreamingProgram>> {
        match &self.tools {
            Some(tools) => Box::pin(streaming::streaming_programs(tools)),
            None => Box::pin(async { Ok(Vec::new()) }),
        }
    }

    fn update(&self, part: StreamingPart) -> Pending<'_, HostOutcome> {
        match (&self.host, &self.client) {
            (Some(host), Some(client)) => Box::pin(streaming::streaming_update(part, host, client)),
            _ => missing(),
        }
    }

    fn skip(&self, part: StreamingPart) -> Pending<'_, ()> {
        match &self.tools {
            Some(tools) => Box::pin(streaming::streaming_skip(part, tools)),
            None => missing(),
        }
    }

    fn hosts(&self) -> Vec<StreamHost> {
        self.hosts
            .as_ref()
            .map(streaming::streaming_hosts)
            .unwrap_or_default()
    }

    fn look_again(&self) -> Pending<'_, Vec<StreamHost>> {
        match &self.hosts {
            Some(hosts) => Box::pin(streaming::streaming_look_again(hosts)),
            None => Box::pin(async { Ok(Vec::new()) }),
        }
    }

    fn add_host<'a>(&'a self, address: &'a str) -> Pending<'a, Vec<StreamHost>> {
        match &self.hosts {
            Some(hosts) => Box::pin(streaming::streaming_add_host(address.to_owned(), hosts)),
            None => missing(),
        }
    }

    fn forget_host<'a>(&'a self, id: &'a str) -> Pending<'a, ()> {
        match (&self.client, &self.hosts) {
            (Some(client), Some(hosts)) => Box::pin(streaming::streaming_forget_host(
                id.to_owned(),
                client,
                hosts,
            )),
            _ => missing(),
        }
    }

    fn pair_with<'a>(&'a self, host_id: &'a str) -> Pending<'a, PairingStarted> {
        match (&self.hosts, &self.client) {
            (Some(hosts), Some(client)) => Box::pin(streaming::streaming_pair_with(
                self.sink.clone(),
                Some(&self.linked),
                hosts,
                client,
                host_id,
            )),
            _ => missing(),
        }
    }

    fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    fn set_quality(&self, quality: StreamQuality) -> Pending<'_, SettingsSnapshot> {
        Box::pin(streaming::streaming_set_quality(&self.settings, quality))
    }
}

/// What Settings → Streaming reads and changes.
#[derive(Clone)]
pub struct Streaming {
    backend: Arc<dyn StreamingBackend>,
    accounts: AccountService,
}

impl Streaming {
    pub fn new(backend: Arc<dyn StreamingBackend>, accounts: AccountService) -> Self {
        Self { backend, accounts }
    }

    /// The running app's: `sink` and `events` are the app's events.
    pub fn of(
        services: &caskade_app::start::Services,
        sink: Events,
        events: broadcast::Sender<Event>,
    ) -> Self {
        Self::new(
            Arc::new(AppStreaming {
                host: services.streaming_host.clone(),
                hosts: services.stream_hosts.clone(),
                client: services.streaming_client.clone(),
                tools: services.tools.clone(),
                linked: services.linked_pairing.clone(),
                settings: services.settings.clone(),
                sink,
                events,
            }),
            services.accounts.clone(),
        )
    }

    /// The same host, for the requests' forms.
    pub fn pairing(&self) -> StreamPairing {
        StreamPairing::new(self.backend.clone(), self.accounts.clone())
    }

    pub fn backend(&self) -> &dyn StreamingBackend {
        &*self.backend
    }

    /// Whether the signed-in account is an admin: only an admin hosts or
    /// accepts pairings (the core refuses the rest).
    pub async fn is_admin(&self) -> bool {
        self.pairing().is_admin().await
    }
}

impl PartialEq for Streaming {
    /// One app's: a panel's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
