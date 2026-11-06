//! A way through two routers for game files (#17).
//!
//! Hole punching opens a path between two devices' endpoints: a router lets
//! in what answers the port that wrote out. Game files never travel on those
//! endpoints, though, since they have a relay transport. They travel on
//! endpoints with none, and no router lets anything in to their ports. So when
//! a plain dial fails, each end opens a UDP port of its own for the transfer:
//!
//! 1. A throwaway endpoint ([`open_port`], with a key of its own, so the
//!    device's connection to its relay stays undisturbed) asks a relay what
//!    public address this port has, and closes. A router that gives a port one
//!    mapping whoever it writes to keeps that mapping for the next socket on
//!    the port. One that maps per destination (symmetric) doesn't, and neither
//!    does the punching behind the device's own link: such a device stays
//!    behind a relay.
//! 2. A relay-free endpoint binds the port: [`crate::copy::DirectDialer`] or
//!    [`crate::swarm::SwarmDialer`] on the side receiving, [`FileServer`] on
//!    the side sending. The two exchange addresses over their session, which
//!    may pass through a relay: it carries no game data.
//! 3. Both dial. Each dial's first packet leaves a hole in its own router, so
//!    the receiver's gets through once the sender's has gone out.
//!
//! Nothing that moves files has a relay transport, whichever way the session
//! that set it up runs.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;

use iroh::endpoint::{Builder, PortmapperConfig, presets};
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, TransportAddr, Watcher as _};

use crate::copy::BlobsProtocol;
use crate::endpoint::Relays;
use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::link::is_local;

/// How long a relay gets to say what it sees.
const ASK_WAIT: Duration = Duration::from_secs(5);
/// How long the sending side keeps writing to the receiver's port.
const KNOCK_WAIT: Duration = Duration::from_secs(20);
/// How many of the receiver's addresses it writes to. A share's recipient is
/// a stranger with a link, and these are addresses it chose.
const MAX_KNOCKED: usize = 16;

/// A UDP port on this device, and where the internet sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPort {
    pub port: u16,
    pub public: Vec<SocketAddr>,
}

/// Picks a free UDP port and asks one of `relays` what public address it has.
/// `None` when there's no relay to ask, or it saw none.
pub async fn open_port(relays: &Relays) -> Option<OpenPort> {
    let builder = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_address_lookup()
        .portmapper_config(PortmapperConfig::Disabled)
        .clear_ip_transports()
        .bind_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        .expect("0.0.0.0 is a valid bind address");
    let endpoint = relays.on(builder)?.bind().await.ok()?;
    let port = endpoint.bound_sockets().first()?.port();
    let mut addrs = endpoint.watch_addr();
    let deadline = tokio::time::Instant::now() + ASK_WAIT;
    let public = loop {
        let public: Vec<SocketAddr> = addrs
            .get()
            .ip_addrs()
            .copied()
            .filter(|addr| !is_local(addr.ip()))
            .collect();
        if !public.is_empty() {
            break public;
        }
        if !matches!(
            tokio::time::timeout_at(deadline, addrs.updated()).await,
            Ok(Ok(_))
        ) {
            endpoint.close().await;
            return None;
        }
    };
    endpoint.close().await;
    Some(OpenPort { port, public })
}

/// A builder for an endpoint with this device's key and no relay transport,
/// on `port` over IPv4 (a router's mapping is for that port) and any over
/// IPv6. Keep it the only way a transfer's endpoint is made.
pub(crate) fn on_port(identity: &Identity, port: u16) -> Builder {
    Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_address_lookup()
        .clear_ip_transports()
        .bind_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port))
        .and_then(|builder| builder.bind_addr(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0)))
        .expect("unspecified addresses are valid bind addresses")
        .secret_key(identity.secret_key().clone())
}

/// Binds `builder` on a port an endpoint just closed. The system may take a
/// moment to let it go.
pub(crate) async fn bind_again(builder: impl Fn() -> Builder) -> Result<Endpoint> {
    let mut tries = 0;
    loop {
        match builder().bind().await {
            Ok(endpoint) => return Ok(endpoint),
            Err(source) if tries == 5 => return Err(Error::Bind { source }),
            Err(_) => tries += 1,
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// What to tell the other device about `endpoint` on `open`: where the
/// internet sees it, and its own addresses, for an IPv6 one the router's
/// firewall has to be opened to just the same.
pub(crate) async fn addrs_of(endpoint: &Endpoint, open: &OpenPort) -> Vec<SocketAddr> {
    let mut addrs = endpoint.watch_addr();
    let mut own: Vec<SocketAddr> = addrs.get().ip_addrs().copied().collect();
    if own.is_empty() {
        let _ = tokio::time::timeout(Duration::from_secs(1), addrs.updated()).await;
        own = addrs.get().ip_addrs().copied().collect();
    }
    own.extend(open.public.iter().copied());
    own.sort();
    own.dedup();
    own
}

/// A relay-free endpoint serving `blobs` on a port of a transfer's own. The
/// provider and its gate are the device's own, so what it may serve to whom
/// is decided as for every copy.
#[derive(Debug)]
pub struct FileServer {
    router: Router,
    knocking: Option<tokio::task::JoinHandle<()>>,
    addrs: Vec<SocketAddr>,
}

impl FileServer {
    /// Serves on `open`'s port and writes to `peer` at `theirs`, which leaves
    /// a hole in this device's router for the dial that comes from there.
    pub async fn bind(
        identity: &Identity,
        open: &OpenPort,
        blobs: BlobsProtocol,
        peer: EndpointId,
        theirs: &[SocketAddr],
    ) -> Result<Self> {
        let endpoint = bind_again(|| on_port(identity, open.port)).await?;
        let addrs = addrs_of(&endpoint, open).await;
        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();
        let theirs = theirs.iter().take(MAX_KNOCKED);
        let target = EndpointAddr::from_parts(peer, theirs.map(|addr| TransportAddr::Ip(*addr)));
        // Nothing answers this dial: only its packets matter. Its dialler
        // refuses or never hears it.
        let knocking = tokio::spawn(async move {
            let _ =
                tokio::time::timeout(KNOCK_WAIT, endpoint.connect(target, iroh_blobs::ALPN)).await;
        });
        Ok(Self {
            router,
            knocking: Some(knocking),
            addrs,
        })
    }

    /// Where the other device dials it.
    pub fn addrs(&self) -> Vec<SocketAddr> {
        self.addrs.clone()
    }

    /// Stops serving and writing.
    pub async fn close(mut self) {
        if let Some(knocking) = self.knocking.take() {
            knocking.abort();
        }
        let _ = self.router.shutdown().await;
    }
}

impl Drop for FileServer {
    fn drop(&mut self) {
        if let Some(knocking) = self.knocking.take() {
            knocking.abort();
        }
    }
}
