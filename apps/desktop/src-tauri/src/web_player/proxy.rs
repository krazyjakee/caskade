//! The game window's proxy for offline bundles: every request the window makes comes
//! here. Capturing, it fetches upstream, verifying TLS itself, and keeps
//! each response in the bundle; playing, it answers from the bundle at the
//! game's own URLs and keeps what the bundle lacks while online; checking,
//! it answers only from the bundle and lists what it couldn't. POSTs are
//! kept by what they posted (`Bundle::put_post`): games load levels that way.
//!
//! The window is told to accept any certificate, which is safe only
//! because every request goes through here: the session's CA is made at
//! start, kept in memory and trusted nowhere else.

use caskade_core::web::{
    block,
    bundle::{self, Bundle, Stored},
};
use http_body_util::{BodyExt, Full};
use hudsucker::{
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse,
    certificate_authority::RcgenAuthority,
    decode_response,
    hyper::body::Bytes,
    hyper::{
        Method, Request, Response, StatusCode,
        header::{self, HeaderMap, HeaderValue},
    },
    rcgen::{
        BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    },
    rustls::crypto::ring,
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
};
use url::Url;

/// What the proxy does with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Fetch everything and keep it.
    Capture,
    /// Answer from the bundle; list what it lacks.
    Check,
    /// Answer from the bundle; fetch and keep what it lacks.
    Play,
}

/// The bundle and what the proxy does with it, shared with the window.
pub struct Session {
    pub bundle: Bundle,
    mode: AtomicU8,
    missing: Mutex<BTreeSet<String>>,
    /// What the capture asked for but couldn't keep (not a GET, or not
    /// 200): never game files, so the check doesn't count them.
    unkept: Mutex<BTreeSet<String>>,
    /// Who asked for each URL this capture kept: its `Referer`, the page,
    /// frame, script or stylesheet that loaded it.
    referers: Mutex<HashMap<String, Option<Url>>>,
}

impl Session {
    pub fn new(bundle: Bundle, mode: Mode) -> Self {
        Self {
            bundle,
            mode: AtomicU8::new(mode as u8),
            missing: Mutex::default(),
            unkept: Mutex::default(),
            referers: Mutex::default(),
        }
    }

    pub fn mode(&self) -> Mode {
        match self.mode.load(Ordering::Relaxed) {
            0 => Mode::Capture,
            1 => Mode::Check,
            _ => Mode::Play,
        }
    }

    /// Changes the mode; a check starts with nothing missing. Only the
    /// capture bar (Linux) changes it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn set_mode(&self, mode: Mode) {
        if mode == Mode::Check {
            self.missing().clear();
        }
        self.mode.store(mode as u8, Ordering::Relaxed);
    }

    /// What the last check asked for that the bundle doesn't have.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn missing_count(&self) -> usize {
        self.missing().len()
    }

    fn missing(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.missing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the game at `start`, found in the page it was captured in,
    /// needs a URL the bundle kept: the game's own, or one something it
    /// loaded asked for, by origin, as far as it goes. What this capture
    /// didn't see asked for stays.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn needed_by(&self, start: &Url) -> impl Fn(&str) -> bool + use<> {
        let referers = self
            .referers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let needed = needed_by(start, &referers);
        move |url| needed.contains(url) || referers.get(url).is_none_or(Option::is_none)
    }

    fn unkept(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.unkept
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Starts the proxy on loopback, on a thread of its own, and returns its
/// port.
pub fn start(session: Arc<Session>) -> Result<u16, String> {
    let (ready, port) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("web game proxy".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            runtime.block_on(async move {
                let proxy = async {
                    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                        .await
                        .map_err(|error| error.to_string())?;
                    let port = listener
                        .local_addr()
                        .map_err(|error| error.to_string())?
                        .port();
                    let proxy = Proxy::builder()
                        .with_listener(listener)
                        .with_ca(authority()?)
                        .with_rustls_connector(ring::default_provider())
                        .with_http_handler(Handler {
                            session,
                            keep: None,
                            fallback: None,
                            origin: None,
                        })
                        .build()
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>((port, proxy))
                };
                match proxy.await {
                    Ok((port, proxy)) => {
                        let _ = ready.send(Ok(port));
                        if let Err(error) = proxy.start().await {
                            tracing::error!(%error, "the game window's proxy stopped");
                        }
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            });
        })
        .map_err(|error| error.to_string())?;
    port.recv().map_err(|error| error.to_string())?
}

/// The URLs the game at `start` needs, by who asked for each: the game's
/// origin, then every origin of what it loaded, until nothing new joins.
/// Cross-origin requests name only their origin, so origins are what
/// count.
fn needed_by(start: &Url, referers: &HashMap<String, Option<Url>>) -> BTreeSet<String> {
    let origin = |url: &Url| url.origin().ascii_serialization();
    let mut origins = BTreeSet::from([origin(start)]);
    let mut needed = BTreeSet::from([start.to_string()]);
    loop {
        let joined: Vec<&String> = referers
            .iter()
            .filter(|(url, referer)| {
                !needed.contains(*url)
                    && referer
                        .as_ref()
                        .is_some_and(|referer| origins.contains(&origin(referer)))
            })
            .map(|(url, _)| url)
            .collect();
        if joined.is_empty() {
            return needed;
        }
        for url in joined {
            if let Ok(parsed) = Url::parse(url) {
                origins.insert(origin(&parsed));
            }
            needed.insert(url.clone());
        }
    }
}

/// A CA for this session only.
fn authority() -> Result<RcgenAuthority, String> {
    let key = KeyPair::generate().map_err(|error| error.to_string())?;
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Caskade game window");
    params.distinguished_name = name;
    Ok(RcgenAuthority::new(
        Issuer::new(params, key),
        1_000,
        ring::default_provider(),
    ))
}

/// A request whose response is kept: a GET's, or a POST's with what it
/// posted.
#[derive(Clone)]
struct Keep {
    url: Url,
    posted: Option<Bytes>,
}

impl Keep {
    /// Its key in the bundle.
    fn key(&self) -> String {
        match &self.posted {
            Some(posted) => bundle::post_key(&self.url, posted),
            None => self.url.to_string(),
        }
    }
}

#[derive(Clone)]
struct Handler {
    session: Arc<Session>,
    /// The request whose response this request's handler keeps.
    keep: Option<Keep>,
    /// Playing, the answer when a POST can't reach its server: the last
    /// response kept for its URL.
    fallback: Option<Stored>,
    /// The request's `Origin`, for the answer's CORS headers.
    origin: Option<HeaderValue>,
}

impl HttpHandler for Handler {
    async fn handle_request(
        &mut self,
        _ctx: &HttpContext,
        mut request: Request<Body>,
    ) -> RequestOrResponse {
        if request.method() == Method::CONNECT {
            let host = request.uri().host().unwrap_or_default();
            return if block::blocked(host) {
                status(StatusCode::FORBIDDEN).into()
            } else {
                request.into()
            };
        }
        let Ok(url) = Url::parse(&request.uri().to_string()) else {
            return status(StatusCode::BAD_REQUEST).into();
        };
        if url.host_str().is_some_and(block::blocked) {
            return status(StatusCode::FORBIDDEN).into();
        }
        self.origin = request.headers().get(header::ORIGIN).cloned();
        self.fallback = None;
        let mode = self.session.mode();
        let method = request.method().clone();
        // What a POST posted is part of what it asks for.
        let posted = if method == Method::POST {
            let (parts, body) = request.into_parts();
            let Ok(body) = body.collect().await.map(|body| body.to_bytes()) else {
                return status(StatusCode::BAD_REQUEST).into();
            };
            request = Request::from_parts(parts, Body::from(Full::new(body.clone())));
            Some(body)
        } else {
            None
        };
        if mode != Mode::Capture
            && (method == Method::GET || method == Method::HEAD)
            && let Some(stored) = self.session.bundle.get(&url)
        {
            return self.serve(stored, method == Method::HEAD).await.into();
        }
        if mode != Mode::Capture
            && let Some(posted) = &posted
        {
            let bundle = &self.session.bundle;
            if let Some(stored) = bundle.get_post(&url, posted) {
                return self.serve(stored, false).await.into();
            }
            // Checking is offline: the last answer is the one it gets.
            // Playing asks the server, and falls back to it.
            self.fallback = bundle.last_post(&url);
            if mode == Mode::Check
                && let Some(stored) = self.fallback.take()
            {
                return self.serve(stored, false).await.into();
            }
        }
        if mode != Mode::Capture
            && method == Method::OPTIONS
            && (self.session.bundle.get(&url).is_some()
                || self.session.bundle.last_post(&url).is_some())
        {
            return self.preflight(request.headers()).into();
        }
        if mode == Mode::Check {
            let unkept = method != Method::GET
                || url.path().ends_with("/favicon.ico")
                || self.session.unkept().contains(url.as_str());
            if !unkept {
                tracing::info!(
                    host = url.host_str(),
                    path = url.path(),
                    "missing from the offline copy"
                );
                self.session.missing().insert(url.into());
            }
            return status(StatusCode::NOT_FOUND).into();
        }
        if method == Method::GET || posted.is_some() {
            // The whole file, so the bundle can keep it.
            request.headers_mut().remove(header::RANGE);
            let keep = Keep { url, posted };
            if mode == Mode::Capture {
                let referer = request
                    .headers()
                    .get(header::REFERER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| Url::parse(value).ok());
                self.session
                    .referers
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(keep.key(), referer);
            }
            self.keep = Some(keep);
        } else if mode == Mode::Capture {
            tracing::debug!(%method, host = url.host_str(), path = url.path(), "not kept");
            self.session.unkept().insert(url.into());
        }
        request.into()
    }

    async fn handle_response(
        &mut self,
        _ctx: &HttpContext,
        response: Response<Body>,
    ) -> Response<Body> {
        self.fallback = None;
        match self.keep.take() {
            Some(keep) => self.kept(keep, response).await,
            None => isolate(response),
        }
    }

    async fn handle_error(
        &mut self,
        _ctx: &HttpContext,
        error: hudsucker::hyper_util::client::legacy::Error,
    ) -> Response<Body> {
        self.keep = None;
        match self.fallback.take() {
            Some(stored) => self.serve(stored, false).await,
            None => {
                tracing::debug!(%error, "a web game request failed");
                status(StatusCode::BAD_GATEWAY)
            }
        }
    }
}

impl Handler {
    /// Keeps a response in the bundle and passes it on.
    async fn kept(&self, keep: Keep, response: Response<Body>) -> Response<Body> {
        if response.status() != StatusCode::OK {
            tracing::debug!(
                status = response.status().as_u16(),
                host = keep.url.host_str(),
                path = keep.url.path(),
                "not kept: not a 200"
            );
            self.session.unkept().insert(keep.url.into());
            return isolate(response);
        }
        let response = match decode_response(response) {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(%error, "a web game response couldn't be decoded");
                return status(StatusCode::BAD_GATEWAY);
            }
        };
        let (mut parts, body) = response.into_parts();
        let bytes = match body.collect().await {
            Ok(body) => body.to_bytes(),
            Err(error) => {
                tracing::warn!(%error, "a web game response broke off");
                return status(StatusCode::BAD_GATEWAY);
            }
        };
        let content_type = parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let session = Arc::clone(&self.session);
        let kept = bytes.clone();
        let stored = tokio::task::spawn_blocking(move || {
            let content_type = content_type.as_deref();
            match &keep.posted {
                Some(posted) => {
                    session
                        .bundle
                        .put_post(&keep.url, posted, 200, content_type, &kept)
                }
                None => session.bundle.put(&keep.url, 200, content_type, &kept),
            }
        })
        .await;
        if !matches!(stored, Ok(Ok(()))) {
            tracing::warn!("a web game file couldn't be kept");
        }
        // The body is whole now: hyper drops the connection on a response
        // with a length and a chunked encoding both (CloudFront chunks its
        // compressed files over HTTP/1.1).
        parts.headers.remove(header::TRANSFER_ENCODING);
        parts
            .headers
            .insert(header::CONTENT_LENGTH, bytes.len().into());
        isolate(Response::from_parts(parts, Body::from(Full::new(bytes))))
    }

    /// A file from the bundle.
    async fn serve(&self, stored: caskade_core::web::bundle::Stored, head: bool) -> Response<Body> {
        let body = match tokio::fs::read(&stored.path).await {
            Ok(body) => body,
            Err(error) => {
                tracing::warn!(%error, "a web game file is missing from its bundle");
                return status(StatusCode::NOT_FOUND);
            }
        };
        let mut response = Response::builder()
            .status(stored.status)
            .header(header::CONTENT_LENGTH, body.len());
        if let Some(content_type) = &stored.content_type {
            response = response.header(header::CONTENT_TYPE, content_type);
        }
        let body = if head { Vec::new() } else { body };
        let mut response = response
            .body(Body::from(Full::new(body.into())))
            .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR));
        self.cors(response.headers_mut());
        isolate(response)
    }

    /// A CORS preflight for a URL the bundle has: anything goes.
    fn preflight(&self, request: &HeaderMap) -> Response<Body> {
        let mut response = status(StatusCode::NO_CONTENT);
        let headers = response.headers_mut();
        self.cors(headers);
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, HEAD, POST, OPTIONS"),
        );
        if let Some(asked) = request.get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
            headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, asked.clone());
        }
        isolate(response)
    }

    /// The bundle answers for every origin it served, credentials included.
    fn cors(&self, headers: &mut HeaderMap) {
        if let Some(origin) = &self.origin {
            headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
            headers.insert(header::VARY, HeaderValue::from_static("Origin"));
        }
    }
}

/// Cross-origin isolation for every response, so games that need
/// `SharedArrayBuffer` get it; CORP lets the isolated page embed the rest.
fn isolate(mut response: Response<Body>) -> Response<Body> {
    let headers = response.headers_mut();
    headers.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "cross-origin-embedder-policy",
        HeaderValue::from_static("require-corp"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("cross-origin"),
    );
    response
}

fn status(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_found_keeps_what_it_loaded_and_not_its_portal() {
        let url = |text: &str| Url::parse(text).unwrap();
        let game = url("https://g1.gdn.poki.com/v2/index.html?country=GB");
        let referers: HashMap<String, Option<Url>> = [
            ("https://poki.com/en/g/robo", None),
            ("https://a.poki-cdn.com/app.js", Some("https://poki.com/")),
            (
                "https://a.poki-cdn.com/font.woff2",
                Some("https://a.poki-cdn.com/app.css"),
            ),
            (game.as_str(), Some("https://games.poki.com/")),
            ("https://g1.gdn.poki.com/v2/game.js", Some(game.as_str())),
            (
                "https://game-cdn.poki.com/sdk.js",
                Some("https://g1.gdn.poki.com/"),
            ),
            (
                "https://game-cdn.poki.com/sdk.wasm",
                Some("https://game-cdn.poki.com/sdk.js"),
            ),
        ]
        .into_iter()
        .map(|(kept, referer)| (kept.to_owned(), referer.map(url)))
        .collect();
        assert_eq!(
            needed_by(&game, &referers),
            BTreeSet::from([
                game.to_string(),
                "https://g1.gdn.poki.com/v2/game.js".to_owned(),
                "https://game-cdn.poki.com/sdk.js".to_owned(),
                "https://game-cdn.poki.com/sdk.wasm".to_owned(),
            ])
        );
    }

    #[test]
    fn a_kept_response_is_sent_with_its_length_alone() {
        let dir = tempfile::tempdir().unwrap();
        let url = Url::parse("https://cache.example/ruffle/core.js").unwrap();
        let session = Arc::new(Session::new(
            Bundle::create(dir.path(), &url).unwrap(),
            Mode::Capture,
        ));
        let handler = Handler {
            session: Arc::clone(&session),
            keep: None,
            fallback: None,
            origin: None,
        };
        let upstream = Response::builder()
            .header(header::TRANSFER_ENCODING, "chunked")
            .header(header::CONTENT_TYPE, "text/javascript")
            .body(Body::from(Full::new("ruffle();".into())))
            .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let keep = Keep {
            url: url.clone(),
            posted: None,
        };
        let response = runtime.block_on(handler.kept(keep, upstream));

        assert!(!response.headers().contains_key(header::TRANSFER_ENCODING));
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "9");
        assert!(session.bundle.has(&url));

        // A POST's response is kept by what it posted.
        let level = Url::parse("https://games.example/get_level.hw").unwrap();
        let keep = Keep {
            url: level.clone(),
            posted: Some("level_id=1".into()),
        };
        assert_eq!(
            keep.key(),
            caskade_core::web::bundle::post_key(&level, b"level_id=1")
        );
        let upstream = Response::new(Body::from(Full::new("<level/>".into())));
        runtime.block_on(handler.kept(keep, upstream));
        assert!(session.bundle.get_post(&level, b"level_id=1").is_some());
        assert!(session.bundle.get(&level).is_none(), "not for a GET");
    }

    /// rustls runs on ring alone. aws-lc's C build cost every cold build
    /// ~36 s; two vendored crates (`vendor/hudsucker-0.25.0`,
    /// `vendor/librqbit-sha1-wrapper-9.0.1`) keep it out. An upgrade that
    /// brings it back fails here: patch the new version the same way.
    #[test]
    fn the_lockfile_has_no_aws_lc() {
        let lock =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../Cargo.lock"))
                .expect("the workspace's Cargo.lock");
        for name in ["aws-lc-rs", "aws-lc-sys"] {
            assert!(
                !lock.contains(&format!("name = \"{name}\"")),
                "{name} is back in Cargo.lock",
            );
        }
    }
}
