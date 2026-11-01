//! Sends a backend's `wasi:http` request to an address its grant allows
//! (CSK-03). `wasmtime_wasi_http::default_send_request` resolves and
//! connects by name, so a granted name that resolves to a local service
//! (DNS rebinding) would reach it. Here the host resolves the name itself,
//! keeps the addresses `capabilities::address_allowed` lets through, and
//! connects to one of those; the rest follows `default_send_request`
//! (rustls with webpki roots, HTTP/1.1, the request's timeouts).

use crate::capabilities::address_allowed;
use http::uri::Scheme;
use http_body::Body;
use std::{
    future::poll_fn,
    io,
    net::{IpAddr, SocketAddr},
    pin::{Pin, pin},
    sync::Arc,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::rustls::{self, pki_types::ServerName};
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, io::TokioIo};

/// Looks a host name up: the system resolver, or a fixed answer in tests.
pub type Resolve = fn(&str, u16) -> io::Result<Vec<SocketAddr>>;

/// The system resolver.
pub fn system_resolve(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    std::net::ToSocketAddrs::to_socket_addrs(&(host, port)).map(Iterator::collect)
}

/// A response, and the future that finishes its connection.
pub(crate) type Sent = (
    http::Response<WasiBody>,
    Box<dyn Future<Output = Result<(), Error>> + Send>,
);

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Sends `request`, whose host and port the grant already allows, to an
/// address the grant allows.
pub(crate) async fn send(
    plugin: String,
    allowlist: Arc<[String]>,
    resolve: Resolve,
    mut request: http::Request<WasiBody>,
    options: Option<RequestOptions>,
) -> Result<Sent, Error> {
    let uri = request.uri();
    let host = uri.host().ok_or(Error::HttpRequestUriInvalid)?;
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let tls = uri.scheme() == Some(&Scheme::HTTPS);
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    let connect_timeout = options
        .and_then(|o| o.connect_timeout)
        .unwrap_or(DEFAULT_TIMEOUT);
    let first_byte_timeout = options
        .and_then(|o| o.first_byte_timeout)
        .unwrap_or(DEFAULT_TIMEOUT);
    let between_bytes_timeout = options
        .and_then(|o| o.between_bytes_timeout)
        .unwrap_or(DEFAULT_TIMEOUT);

    let lookup = host.clone();
    let found = tokio::time::timeout(
        connect_timeout,
        tokio::task::spawn_blocking(move || resolve(&lookup, port)),
    )
    .await
    .map_err(|_| Error::DnsTimeout)?
    .map_err(|_| dns_error())?
    .map_err(|_| dns_error())?;
    let addresses: Vec<SocketAddr> = found
        .into_iter()
        .filter(|address| address_allowed(&allowlist, *address))
        .collect();
    if addresses.is_empty() {
        // Only the host is logged; paths and queries may carry secrets.
        tracing::warn!(
            plugin = %plugin,
            host,
            port,
            "plugin HTTP request refused: no address the grant allows"
        );
        return Err(Error::DestinationIpProhibited);
    }

    let stream = tokio::time::timeout(connect_timeout, connect(&addresses))
        .await
        .map_err(|_| Error::ConnectionTimeout)?
        .map_err(Error::Connect)?;

    // The request line carries only the path; the host goes in `Host`.
    *request.uri_mut() = http::Uri::builder()
        .path_and_query(
            request
                .uri()
                .path_and_query()
                .map_or("/", |path| path.as_str()),
        )
        .build()
        .map_err(|_| Error::HttpRequestUriInvalid)?;

    let timeouts = (connect_timeout, first_byte_timeout, between_bytes_timeout);
    if tls {
        let name = match host.parse::<IpAddr>() {
            Ok(ip) => ServerName::from(ip),
            Err(_) => ServerName::try_from(host.as_str())?.to_owned(),
        };
        let stream = tokio::time::timeout(connect_timeout, tls_connector()?.connect(name, stream))
            .await
            .map_err(|_| Error::ConnectionTimeout)?
            .map_err(Error::Tls)?;
        exchange(stream, request, timeouts).await
    } else {
        exchange(stream, request, timeouts).await
    }
}

fn dns_error() -> Error {
    Error::DnsError {
        rcode: None,
        info_code: None,
    }
}

/// Connects to the first address that answers.
async fn connect(addresses: &[SocketAddr]) -> io::Result<TcpStream> {
    let mut last = None;
    for address in addresses {
        match TcpStream::connect(address).await {
            Ok(stream) => return Ok(stream),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::from(io::ErrorKind::AddrNotAvailable)))
}

fn tls_connector() -> Result<tokio_rustls::TlsConnector, Error> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| Error::Tls(io::Error::other(error)))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

/// One HTTP/1.1 exchange over `stream`, as `default_send_request` does it.
async fn exchange<S>(
    stream: S,
    request: http::Request<WasiBody>,
    (connect_timeout, first_byte_timeout, between_bytes_timeout): (Duration, Duration, Duration),
) -> Result<Sent, Error>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    use http_body_util::BodyExt;

    let (mut sender, conn) = tokio::time::timeout(
        connect_timeout,
        hyper::client::conn::http1::Builder::new().handshake(TokioIo::new(stream)),
    )
    .await
    .map_err(|_| Error::ConnectionTimeout)??;

    let send = async move {
        let response = tokio::time::timeout(first_byte_timeout, sender.send_request(request))
            .await
            .map_err(|_| Error::ConnectionReadTimeout)?
            .map_err(Error::from)?;
        // Tokio panics on a zero interval.
        let between = between_bytes_timeout.max(Duration::from_nanos(1));
        let mut timeout = tokio::time::interval(between);
        timeout.reset();
        Ok::<_, Error>(response.map(|incoming| TimedBody { incoming, timeout }))
    };
    let mut send = pin!(send);
    let mut conn = Some(conn);
    // Wait for the response while driving the connection.
    let response = poll_fn(|cx| match send.as_mut().poll(cx) {
        Poll::Ready(result) => Poll::Ready(result),
        Poll::Pending => {
            let Some(running) = conn.as_mut() else {
                return Poll::Pending;
            };
            let result = ready!(Pin::new(running).poll(cx));
            conn = None;
            match result {
                Ok(()) => send.as_mut().poll(cx),
                Err(error) => Poll::Ready(Err(Error::from(error))),
            }
        }
    })
    .await?;
    let io = async move {
        match conn {
            Some(conn) => conn.await.map_err(|error| {
                if error.is_timeout() {
                    Error::HttpResponseTimeout
                } else {
                    Error::from(error)
                }
            }),
            None => Ok(()),
        }
    };
    Ok((
        response.map(BodyExt::boxed_unsync),
        Box::new(io) as Box<dyn Future<Output = _> + Send>,
    ))
}

/// A response body that fails when no frame comes for the request's
/// between-bytes timeout.
struct TimedBody {
    incoming: hyper::body::Incoming,
    timeout: tokio::time::Interval,
}

impl Body for TimedBody {
    type Data = <hyper::body::Incoming as Body>::Data;
    type Error = Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        match Pin::new(&mut self.incoming).poll_frame(cx) {
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(if error.is_timeout() {
                Error::HttpResponseTimeout
            } else {
                Error::from(error)
            }))),
            Poll::Ready(Some(Ok(frame))) => {
                self.timeout.reset();
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Pending => {
                ready!(self.timeout.poll_tick(cx));
                Poll::Ready(Some(Err(Error::ConnectionReadTimeout)))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.incoming.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.incoming.size_hint()
    }
}
