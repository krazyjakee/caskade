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
