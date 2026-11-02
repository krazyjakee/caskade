//! The endpoint: binding, dialling and the accept loop.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iroh::endpoint::{Connection, VarInt, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode};

use crate::ALPN;
use crate::error::{Error, Result};
use crate::frame::{FrameReader, FrameWriter};
use crate::grant::Grant;
use crate::identity::Identity;
use crate::opening::{self, Purpose};

/// QUIC application close codes, with fixed reasons: nothing a peer reads
/// from a close depends on what this device holds.
mod close {
    pub(super) const DONE: u32 = 0;
    pub(super) const REFUSED: u32 = 1;
    pub(super) const REFUSED_REASON: &[u8] = b"refused";
}

/// Which transports the endpoint has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// What the app uses: n0's address lookup, and n0's relays when `relays`
    /// is true. `port` fixes the UDP port to listen on (for firewall rules);
    /// `None` lets the OS pick.
    Network { relays: bool, port: Option<u16> },
    /// One socket on 127.0.0.1, no relays and no address lookup. Tests use it:
    /// nothing bound this way can leave the machine.
    Loopback,
}

/// How an endpoint is set up.
#[derive(Clone)]
pub struct Config {
    /// The identity to present. `None` makes a throwaway one, for tests.
    pub identity: Option<Identity>,
    pub reach: Reach,
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
            reach: Reach::Network {
                relays: true,
                port: None,
            },
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
            .field("open_timeout", &self.open_timeout)
            .field("blobs", &self.blobs.is_some())
            .finish()
    }
}

impl Config {
    /// Loopback only, for tests.
    pub fn loopback() -> Self {
        Self {
            reach: Reach::Loopback,
            ..Self::default()
        }
    }

    async fn bind(&self) -> Result<Endpoint> {
        let builder = match self.reach {
            Reach::Network { relays, port } => {
                let builder = if relays {
                    Endpoint::builder(presets::N0)
                } else {
                    Endpoint::builder(presets::N0DisableRelay)
                };
                match port {
                    Some(port) => builder
                        .clear_ip_transports()
                        .bind_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port))
                        .and_then(|builder| {
                            builder.bind_addr(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0))
                        })
                        .expect("unspecified addresses are valid bind addresses"),
                    None => builder,
                }
            }
            Reach::Loopback => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Disabled)
                .clear_address_lookup()
                .clear_ip_transports()
                .bind_addr(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
                .expect("127.0.0.1:0 is a valid bind address"),
        };
        // The only place the device key leaves `Identity`: straight into
        // iroh's builder.
        let builder = match &self.identity {
            Some(identity) => builder.secret_key(identity.secret_key().clone()),
            None => builder,
        };
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

    pub fn connection(&self) -> &Connection {
        &self.connection
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

    /// Ends the session normally.
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
}

impl SyncEndpoint {
    /// Binds and starts accepting, on the caller's tokio runtime.
    pub async fn bind(
        config: Config,
        admission: Arc<dyn Admission>,
        sessions: Arc<dyn Sessions>,
    ) -> Result<Self> {
        let endpoint = config.bind().await?;
        let open_timeout = config.open_timeout;
        let mut builder = Router::builder(endpoint).accept(
            ALPN,
            Inbound {
                admission,
                sessions,
                open_timeout,
            },
        );
        if let Some(blobs) = config.blobs {
            builder = builder.accept(iroh_blobs::ALPN, blobs);
        }
        Ok(Self {
            router: builder.spawn(),
            open_timeout,
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
        let exchange = opening::offer(&connection, purpose, grant.as_ref());
        match opening::within(self.open_timeout, exchange).await {
            Ok((writer, reader)) => Ok(Session {
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
    admission: Arc<dyn Admission>,
    sessions: Arc<dyn Sessions>,
    open_timeout: Duration,
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
        let session = Session {
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
