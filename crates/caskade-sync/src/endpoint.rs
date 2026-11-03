//! The endpoint: binding, dialling and the accept loop.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use iroh::Watcher as _;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt as _};
use iroh::endpoint::{Connection, VarInt, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode};

use crate::ALPN;
use crate::device::{DeviceInfo, DeviceKind};
use crate::error::{Error, Result};
use crate::frame::{FrameReader, FrameWriter};
use crate::grant::Grant;
use crate::identity::Identity;
use crate::lan::{Lan, Nearby};
use crate::link::{Link, Links};
use crate::opening::{self, Purpose};

/// QUIC application close codes, with fixed reasons: nothing a peer reads
/// from a close depends on what this device holds.
mod close {
    pub(super) const DONE: u32 = 0;
    pub(super) const REFUSED: u32 = 1;
    pub(super) const REFUSED_REASON: &[u8] = b"refused";
}

/// Which relays an endpoint may use.
#[derive(Debug, Clone)]
pub enum Relays {
    /// n0's relays, used only when nothing direct works.
    N0,
    /// None: Never use relays. Direct paths, over the internet too, still
    /// work; a device only a relay could reach stays offline.
    Never,
    /// A relay of the test's own (iroh's `test_utils::run_relay_server`),
    /// with its self-signed certificate accepted.
    #[cfg(feature = "test-utils")]
    Local(iroh::RelayMap),
}

/// Which IP sockets an endpoint binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ip {
    /// Every interface. `port` fixes the UDP port (for firewall rules; Windows
    /// asks to allow incoming UDP the first time); `None` lets the OS pick.
    Any { port: Option<u16> },
    /// 127.0.0.1 only: nothing bound this way leaves the machine.
    Loopback { port: Option<u16> },
    /// No IP sockets: relays only.
    Off,
}

/// How far an endpoint reaches.
#[derive(Debug, Clone)]
pub struct Reach {
    pub relays: Relays,
    pub ip: Ip,
    /// n0's DNS/pkarr address lookup: publishes this endpoint's addresses
    /// (never its device info) and finds other devices' over the internet.
    pub internet_lookup: bool,
    /// mDNS on the local network under this service name, announcing
    /// [`Config::device`]. `None` turns local discovery off.
    pub lan: Option<String>,
}

impl Reach {
    /// What the app uses: local discovery first, n0's address lookup, and
    /// n0's relays unless `never_relay`.
    pub fn network(never_relay: bool, port: Option<u16>) -> Self {
        Self {
            relays: if never_relay {
                Relays::Never
            } else {
                Relays::N0
            },
            ip: Ip::Any { port },
            internet_lookup: true,
            lan: Some(crate::lan::SERVICE.to_owned()),
        }
    }

    /// One socket on 127.0.0.1, no relays and no address lookup, for tests.
    pub fn loopback() -> Self {
        Self {
            relays: Relays::Never,
            ip: Ip::Loopback { port: None },
            internet_lookup: false,
            lan: None,
        }
    }

    /// Whether this endpoint may use a relay.
    pub fn relays_allowed(&self) -> bool {
        !matches!(self.relays, Relays::Never)
    }
}

/// How an endpoint is set up.
#[derive(Clone)]
pub struct Config {
    /// The identity to present. `None` makes a throwaway one, for tests.
    pub identity: Option<Identity>,
    pub reach: Reach,
    /// This device's name, kind and OS, announced on the local network.
    pub device: DeviceInfo,
    /// How long a peer gets for each step of the opening.
    pub open_timeout: Duration,
    /// iroh-blobs on the same endpoint, under `iroh_blobs::ALPN`. Every ALPN
    /// has to be known at bind time: the router is built once.
    pub blobs: Option<iroh_blobs::BlobsProtocol>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            identity: None,
            reach: Reach::network(false, None),
            device: DeviceInfo::new("Caskade", DeviceKind::Desktop),
            open_timeout: Duration::from_secs(10),
            blobs: None,
        }
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("identity", &self.identity)
            .field("reach", &self.reach)
            .field("device", &self.device)
            .field("open_timeout", &self.open_timeout)
            .field("blobs", &self.blobs.is_some())
            .finish()
    }
}

impl Config {
    /// Loopback only, for tests.
    pub fn loopback() -> Self {
        Self {
            reach: Reach::loopback(),
            ..Self::default()
        }
    }

    async fn bind(&self) -> Result<Endpoint> {
        let reach = &self.reach;
        // The N0 presets when looking devices up over the internet; Minimal
        // (the crypto provider only) otherwise.
        let mut builder = match (reach.internet_lookup, &reach.relays) {
            (true, Relays::N0) => Endpoint::builder(presets::N0),
            (true, _) => Endpoint::builder(presets::N0DisableRelay),
            (false, _) => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Disabled)
                .clear_address_lookup(),
        };
        #[cfg(feature = "test-utils")]
        if let Relays::Local(map) = &reach.relays {
            builder = builder
                .relay_mode(RelayMode::Custom(map.clone()))
                .ca_tls_config(iroh::tls::CaTlsConfig::insecure_skip_verify());
        }
        builder = match reach.ip {
            Ip::Any { port: None } => builder,
            Ip::Any { port: Some(port) } => builder
                .clear_ip_transports()
                .bind_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port))
                .and_then(|builder| {
                    builder.bind_addr(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0))
                })
                .expect("unspecified addresses are valid bind addresses"),
            Ip::Loopback { port } => builder
                .clear_ip_transports()
                .bind_addr(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port.unwrap_or(0)))
                .expect("127.0.0.1 is a valid bind address"),
            Ip::Off => builder.clear_ip_transports(),
        };
        // The only place the device key leaves `Identity`: straight into
        // iroh's builder.
        if let Some(identity) = &self.identity {
            builder = builder.secret_key(identity.secret_key().clone());
        }
        builder
            .bind()
            .await
            .map_err(|source| Error::Bind { source })
    }
}

/// Who may open a connection for what.
///
/// Called once per connection with the authenticated peer, the purpose it
/// named and the grant it presented, and answers only yes or no: there's no
/// way to return a reason, so a refusal can't disclose one. Called on the
/// accept path with input a stranger chose, so it must be cheap and must not
/// block: check against memory, not the disk. A policy that consumes
/// something (a single-use pairing link) does so here, atomically.
pub trait Admission: Send + Sync + 'static {
    fn admits(&self, peer: EndpointId, purpose: Purpose, grant: Option<&Grant>) -> bool;
}

/// Where accepted sessions go. The future is the session's life: the
/// connection closes when it returns, unless the session was moved somewhere
/// that outlives it.
#[async_trait]
pub trait Sessions: Send + Sync + 'static {
    async fn opened(&self, session: Session);
}

/// A connection whose opening was accepted.
///
/// The peer's id is authenticated by TLS. The opening's stream stays open as
/// [`Session::writer`] and [`Session::reader`] for the purpose's own messages,
/// one JSON line each.
#[derive(Debug)]
pub struct Session {
    local: EndpointId,
    purpose: Purpose,
    grant: Option<Grant>,
    connection: Connection,
    writer: FrameWriter,
    reader: FrameReader,
}

impl Session {
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// The peer's endpoint id: its TLS identity, not a claim it made.
    pub fn peer(&self) -> EndpointId {
        self.connection.remote_id()
    }

    /// The grant presented in the opening: what this side sent when dialling,
    /// what the peer sent when accepting.
    pub fn grant(&self) -> Option<&Grant> {
        self.grant.as_ref()
    }

    /// The peer's IP addresses this connection knows (the ones it uses or
    /// could): where a direct connection can reach it. A relay isn't one.
    pub fn direct_addrs(&self) -> Vec<std::net::SocketAddr> {
        let mut addrs: Vec<std::net::SocketAddr> = self
            .connection
            .paths()
            .iter()
            .filter_map(|path| match path.remote_addr() {
                iroh::TransportAddr::Ip(addr) => Some(*addr),
                _ => None,
            })
            .collect();
        addrs.sort();
        addrs.dedup();
        addrs
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// This device's endpoint id.
    pub fn local_id(&self) -> EndpointId {
        self.local
    }

    /// Whether this side dialled the connection.
    pub fn is_dialler(&self) -> bool {
        self.connection.side() == iroh::endpoint::Side::Client
    }

    /// Both halves of the stream at once, for reading and writing
    /// concurrently.
    pub fn split(&mut self) -> (&mut FrameWriter, &mut FrameReader) {
        (&mut self.writer, &mut self.reader)
    }

    pub fn writer(&mut self) -> &mut FrameWriter {
        &mut self.writer
    }

    pub fn reader(&mut self) -> &mut FrameReader {
        &mut self.reader
    }

    /// The peer's address as observed on this connection's paths, for
    /// dialling it again.
    pub fn addr(&self) -> EndpointAddr {
        EndpointAddr::from_parts(
            self.peer(),
            self.connection
                .paths()
                .iter()
                .map(|path| path.remote_addr().clone()),
        )
    }

    /// Ends the session normally once the peer has everything written: the
    /// stream is finished and its data acknowledged (waiting at most 2 s),
    /// then the connection closes. A plain [`Session::close`] can discard a
    /// last message still in flight.
    pub async fn end(self) {
        let Self {
            writer, connection, ..
        } = self;
        let _ = tokio::time::timeout(Duration::from_secs(2), writer.finish()).await;
        connection.close(VarInt::from_u32(close::DONE), b"done");
    }

    /// Ends the session normally, at once.
    pub fn close(self) {
        self.connection
            .close(VarInt::from_u32(close::DONE), b"done");
    }
}

/// A bound endpoint speaking `caskade/sync/1` (and iroh-blobs when
/// configured).
///
/// Dropping it aborts the accept loop (iroh's `Router` aborts on drop), so
/// the app holds one for the process lifetime and calls
/// [`SyncEndpoint::shutdown`] at exit.
#[derive(Debug, Clone)]
pub struct SyncEndpoint {
    router: Router,
    open_timeout: Duration,
    relays_allowed: bool,
    links: Links,
    lan: Option<Lan>,
}

impl SyncEndpoint {
    /// Binds and starts accepting, on the caller's tokio runtime.
    pub async fn bind(
        config: Config,
        admission: Arc<dyn Admission>,
        sessions: Arc<dyn Sessions>,
    ) -> Result<Self> {
        let endpoint = config.bind().await?;
        let links = Links::new(&endpoint);
        let lan = match &config.reach.lan {
            Some(service) => {
                match Lan::start(&endpoint, service, config.device.clone(), links.clone()).await {
                    Ok(lan) => Some(lan),
                    Err(error) => {
                        // A network that allows neither IPv4 nor IPv6 multicast:
                        // pairing links and typed addresses still work.
                        tracing::warn!(%error, "no local network discovery");
                        None
                    }
                }
            }
            None => None,
        };
        let open_timeout = config.open_timeout;
        let local = endpoint.id();
        let mut builder = Router::builder(endpoint).accept(
            ALPN,
            Inbound {
                local,
                admission,
                sessions,
                open_timeout,
                links: links.clone(),
            },
        );
        if let Some(blobs) = config.blobs {
            builder = builder.accept(iroh_blobs::ALPN, blobs);
        }
        Ok(Self {
            router: builder.spawn(),
            open_timeout,
            relays_allowed: config.reach.relays_allowed(),
            links,
            lan,
        })
    }

    /// This device's endpoint id.
    pub fn id(&self) -> EndpointId {
        self.endpoint().id()
    }

    /// Where this endpoint can be reached right now.
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint().addr()
    }

    /// This endpoint's address for a pairing link or a share: every local
    /// address, plus the relay once there is one.
    ///
    /// Waits at most `wait`. Local addresses come first and are enough on
    /// the local network, so this never waits for the internet the way
    /// `Endpoint::online` would (and with no internet, a relay never comes).
    /// When relays are allowed but none answered in time, the address has
    /// none; mint again later to include it.
    pub async fn shareable_addr(&self, wait: Duration) -> EndpointAddr {
        let mut watcher = self.endpoint().watch_addr();
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let addr = watcher.get();
            let has_ip = addr.ip_addrs().next().is_some();
            let has_relay = addr.relay_urls().next().is_some();
            if has_ip && (has_relay || !self.relays_allowed) {
                return addr;
            }
            match tokio::time::timeout_at(deadline, watcher.updated()).await {
                Ok(Ok(_)) => {}
                _ => return watcher.get(),
            }
        }
    }

    /// Whether this device reaches the internet, as far as it can tell: a
    /// relay answers. `None` with Never use relays on (nothing to ask).
    pub fn internet(&self) -> Option<bool> {
        if !self.relays_allowed {
            return None;
        }
        let status = self.endpoint().home_relay_status().get();
        Some(status.iter().any(|relay| relay.is_connected()))
    }

    /// Relay connections coming and going (so [`SyncEndpoint::internet`]
    /// may have changed). One watcher for the whole loop: iroh hands out
    /// clones of a stored watcher, and a fresh clone reports every change
    /// since that watcher was made, so asking anew each time never waits.
    pub fn internet_changes(&self) -> InternetChanges {
        let updates = self.endpoint().home_relay_status().stream_updates_only();
        InternetChanges(Box::pin(updates.map(|_| ())))
    }

    /// Whether this endpoint may use relays (Never use relays is off).
    pub fn relays_allowed(&self) -> bool {
        self.relays_allowed
    }

    /// Every connection's link state, and its changes.
    pub fn links(&self) -> &Links {
        &self.links
    }

    /// The link state of one device: [`Link::Offline`] with no connection.
    pub fn link(&self, peer: EndpointId) -> Link {
        self.links.get(peer)
    }

    /// The link `peer` had over its last open connection, if it ever had
    /// one since this endpoint started.
    pub fn last_link(&self, peer: EndpointId) -> Option<Link> {
        self.links.last(peer)
    }

    /// Local network discovery, when it's on and the network allows it.
    pub fn lan(&self) -> Option<&Lan> {
        self.lan.as_ref()
    }

    /// The Caskade devices announcing themselves on this network.
    pub fn nearby(&self) -> Vec<Nearby> {
        self.lan.as_ref().map(Lan::nearby).unwrap_or_default()
    }

    /// The iroh endpoint, for protocols stacked on it (iroh-blobs'
    /// downloader takes one).
    pub fn endpoint(&self) -> &Endpoint {
        self.router.endpoint()
    }

    /// Dials `peer` and opens the connection for `purpose`, presenting
    /// `grant`. [`Error::Refused`] when the peer refuses, for any reason.
    pub async fn connect(
        &self,
        peer: impl Into<EndpointAddr>,
        purpose: Purpose,
        grant: Option<Grant>,
    ) -> Result<Session> {
        let peer = peer.into();
        let id = peer.id;
        let connection = opening::within(self.open_timeout, async {
            self.endpoint()
                .connect(peer, ALPN)
                .await
                .map_err(|source| Error::Dial { peer: id, source })
        })
        .await?;
        self.links.track(&connection);
        let exchange = opening::offer(&connection, purpose, grant.as_ref());
        match opening::within(self.open_timeout, exchange).await {
            Ok((writer, reader)) => Ok(Session {
                local: self.id(),
                purpose,
                grant,
                connection,
                writer,
                reader,
            }),
            Err(error) => {
                connection.close(VarInt::from_u32(close::REFUSED), close::REFUSED_REASON);
                Err(error)
            }
        }
    }

    /// Stops accepting and closes the endpoint. Call this rather than
    /// dropping the endpoint; it's idempotent.
    pub async fn shutdown(&self) -> Result<()> {
        self.router
            .shutdown()
            .await
            .map_err(|source| Error::Shutdown {
                source: Box::new(source),
            })
    }
}

/// The accept side of `caskade/sync/1`.
struct Inbound {
    local: EndpointId,
    admission: Arc<dyn Admission>,
    sessions: Arc<dyn Sessions>,
    open_timeout: Duration,
    links: Links,
}

/// Written by hand: a derived `Debug` would ask the policy to print itself,
/// and iroh logs handlers.
impl fmt::Debug for Inbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inbound")
            .field("open_timeout", &self.open_timeout)
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for Inbound {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let peer = connection.remote_id();
        let admission = &self.admission;
        let received = opening::answer(&connection, self.open_timeout, |purpose, grant| {
            admission.admits(peer, purpose, grant)
        })
        .await;
        let Some(received) = received else {
            // `Ok`: refusing a stranger is this handler working.
            connection.close(VarInt::from_u32(close::REFUSED), close::REFUSED_REASON);
            return Ok(());
        };
        self.links.track(&connection);
        let session = Session {
            local: self.local,
            purpose: received.purpose,
            grant: received.grant,
            connection,
            writer: received.writer,
            reader: received.reader,
        };
        self.sessions.opened(session).await;
        Ok(())
    }
}

/// Relay connections coming and going, from
/// [`SyncEndpoint::internet_changes`].
pub struct InternetChanges(Pin<Box<dyn Stream<Item = ()> + Send>>);

impl InternetChanges {
    /// Waits for the next change; `false` once the endpoint is gone.
    pub async fn next(&mut self) -> bool {
        self.0.next().await.is_some()
    }
}
