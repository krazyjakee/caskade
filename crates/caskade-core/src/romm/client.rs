//! The RomM REST client: sign-in, listings and streaming ROM downloads.
//!
//! - `password` servers exchange the username and password for a
//!   short-lived access token (`POST /api/token`, OAuth2 password grant)
//!   and exchange again when it expires. A server without that endpoint
//!   gets HTTP Basic, which every RomM version accepts.
//! - `token` servers send their API token as a bearer token.
//!
//! Downloads resume: a `.part` file left by an interrupted download is
//! continued with an HTTP range request when the server honours it.

use super::{
    RommError, Timeouts,
    api::{
        Heartbeat, RemoteCollection, RemoteFirmware, RemotePlatform, RemoteRom, RomPage,
        TokenResponse,
    },
    settings::{RommAuth, RommServerConfig},
};
use std::{
    collections::HashSet,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// The scopes a library client needs; a viewer account has them all.
const SCOPES: &str = "me.read roms.read platforms.read assets.read collections.read";
/// ROMs per page of `/api/roms`.
const PAGE: u32 = 250;
/// Listings and artwork are small; anything bigger is not what we asked for.
const MAX_JSON: usize = 64 << 20;
const MAX_IMAGE: usize = 16 << 20;
/// BIOS files are KiB to a few MiB; firmware packages run larger.
const MAX_FIRMWARE: usize = 256 << 20;
/// A firmware file's whole request: larger than a listing, still bounded.
const FIRMWARE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Pages of one platform's ROMs, past any real library (2.5 million ROMs).
const MAX_PAGES: u32 = 10_000;

#[derive(Clone)]
enum Credentials {
    None,
    Password { username: String, password: String },
    Token(String),
}

/// An access token and when it expires.
type Token = (String, Instant);

/// Clones share the connection pool and the access token.
#[derive(Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    base: url::Url,
    credentials: Credentials,
    /// The access token and when it expires; `Some(None)` after the server
    /// turned out to have no token endpoint (use Basic).
    token: Arc<tokio::sync::Mutex<Option<Option<Token>>>>,
    /// The whole of a listing, image or sign-in request.
    request_timeout: Duration,
}

/// Only a connection that can't be made (refused, DNS, TLS, the connect
/// timeout) means the server is unreachable. A server that answered and
/// then stalled or broke off is a transient failure of that request.
fn net(error: reqwest::Error) -> RommError {
    // Without the URL: it can carry credentials or query parameters.
    let error = error.without_url();
    if error.is_connect() {
        // The innermost cause says why (refused, no such host, …); reqwest's
        // own text is only "error sending request".
        let mut why = error.to_string();
        let mut source = std::error::Error::source(&error);
        while let Some(cause) = source {
            why = cause.to_string();
            source = cause.source();
        }
        RommError::Unreachable(why)
    } else if error.is_timeout() || error.is_body() || error.is_decode() || error.is_request() {
        RommError::Interrupted(error.to_string())
    } else {
        RommError::Server(error.to_string())
    }
}

/// The server's `detail` message, or the status.
fn failure(status: reqwest::StatusCode, body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("detail")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| status.to_string())
}

pub(crate) fn base_url(url: &str) -> Result<url::Url, RommError> {
    let mut base = url::Url::parse(url.trim())
        .map_err(|_| RommError::Config(crate::tr!("the server address is not a valid URL")))?;
    if !matches!(base.scheme(), "http" | "https") || base.host().is_none() {
        return Err(RommError::Config(crate::tr!(
            "the server address must start with http:// or https://"
        )));
    }
    if !base.username().is_empty() || base.password().is_some() {
        return Err(RommError::Config(crate::tr!(
            "put credentials in the sign-in fields, not the address"
        )));
    }
    base.set_query(None);
    base.set_fragment(None);
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    Ok(base)
}

impl Client {
    pub(crate) fn new(
        config: &RommServerConfig,
        secret: Option<String>,
        timeouts: Timeouts,
    ) -> Result<Self, RommError> {
        let base = base_url(&config.url)?;
        let credentials = match (config.auth, secret) {
            (RommAuth::Token, Some(token)) => Credentials::Token(token),
            (RommAuth::Password, Some(password)) if !config.username.trim().is_empty() => {
                Credentials::Password {
                    username: config.username.trim().to_owned(),
                    password,
                }
            }
            _ => Credentials::None,
        };
        let http = reqwest::Client::builder()
            .connect_timeout(timeouts.connect)
            // ROMs reach gigabytes: no total timeout on the client, only a
            // stall one. Every other request sets `request_timeout`.
            .read_timeout(timeouts.read)
            // Requests carry the password, a token or a session: redirects
            // stay on the server's origin, or move it from HTTP to HTTPS
            // (#157). reqwest keeps a 307's body and, on the same host and
            // port, the `Authorization` header.
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let next = attempt.url();
                let same_server = attempt.previous().first().is_some_and(|first| {
                    next.host_str() == first.host_str()
                        && (next.origin() == first.origin()
                            || (first.scheme() == "http" && next.scheme() == "https"))
                });
                if attempt.previous().len() > 5 {
                    attempt.error("too many redirects")
                } else if same_server {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .danger_accept_invalid_certs(config.allow_invalid_certs)
            .user_agent(concat!("Caskade/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(net)?;
        Ok(Self {
            http,
            base,
            credentials,
            token: Arc::default(),
            request_timeout: timeouts.request,
        })
    }

    /// Whether `url` is on this server: its images may be on the LAN,
    /// unlike images from elsewhere (CSK-11).
    pub(crate) fn same_origin(&self, url: &url::Url) -> bool {
        url.origin() == self.base.origin()
    }

    fn url(&self, path: &str) -> Result<url::Url, RommError> {
        self.base
            .join(path.trim_start_matches('/'))
            .map_err(|error| RommError::Server(error.to_string()))
    }

    /// The server's version (`/api/heartbeat`, no sign-in needed).
    pub(crate) async fn heartbeat(&self) -> Result<Option<String>, RommError> {
        let response = self
            .http
            .get(self.url("api/heartbeat")?)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(net)?;
        let status = response.status();
        let body = read(response, MAX_JSON).await?;
        if !status.is_success() {
            return Err(RommError::Server(crate::tr!(
                "this doesn't look like a RomM server ({status})",
                status = status
            )));
        }
        let heartbeat: Heartbeat = serde_json::from_slice(&body)
            .map_err(|_| RommError::Server(crate::tr!("this doesn't look like a RomM server")))?;
        Ok(heartbeat.system.version)
    }

    async fn access_token(&self, renew: bool) -> Result<Option<String>, RommError> {
        let Credentials::Password { username, password } = &self.credentials else {
            return Ok(None);
        };
        let mut token = self.token.lock().await;
        match &*token {
            Some(None) => return Ok(None),
            Some(Some((value, expires))) if !renew && *expires > Instant::now() => {
                return Ok(Some(value.clone()));
            }
            _ => {}
        }
        let response = self
            .http
            .post(self.url("api/token")?)
            .timeout(self.request_timeout)
            .form(&[
                ("grant_type", "password"),
                ("username", username.as_str()),
                ("password", password.as_str()),
                ("scope", SCOPES),
            ])
            .send()
            .await
            .map_err(net)?;
        let status = response.status();
        let body = read(response, MAX_JSON).await?;
        match status.as_u16() {
            200..=299 => {
                let issued: TokenResponse = serde_json::from_slice(&body)
                    .map_err(|_| RommError::Server(crate::tr!("unexpected token response")))?;
                // Renew a little early.
                let lifetime = issued.expires.unwrap_or(900).saturating_sub(30).max(30);
                let expires = Instant::now() + Duration::from_secs(lifetime);
                *token = Some(Some((issued.access_token.clone(), expires)));
                Ok(Some(issued.access_token))
            }
            400 | 401 | 403 => Err(RommError::Auth(failure(status, &body))),
            // No token endpoint (an older server): Basic on every request.
            404 | 405 => {
                *token = Some(None);
                Ok(None)
            }
            _ => Err(RommError::Server(failure(status, &body))),
        }
    }

    async fn authorized(
        &self,
        request: reqwest::RequestBuilder,
        renew: bool,
    ) -> Result<reqwest::RequestBuilder, RommError> {
        Ok(match &self.credentials {
            Credentials::None => request,
            Credentials::Token(token) => request.bearer_auth(token),
            Credentials::Password { username, password } => match self.access_token(renew).await? {
                Some(token) => request.bearer_auth(token),
                None => request.basic_auth(username, Some(password)),
            },
        })
    }

    /// Sends an authorized request; an expired token is renewed once.
    async fn send(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, RommError> {
        let mut renew = false;
        loop {
            let response = self
                .authorized(build(), renew)
                .await?
                .send()
                .await
                .map_err(net)?;
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED
                && !renew
                && matches!(self.credentials, Credentials::Password { .. })
            {
                renew = true;
                continue;
            }
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                let body = read(response, MAX_JSON).await.unwrap_or_default();
                return Err(RommError::Auth(
                    if matches!(self.credentials, Credentials::None) {
                        crate::tr!("this server needs a username and password or an API token")
                    } else {
                        failure(status, &body)
                    },
                ));
            }
            return Ok(response);
        }
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, url: url::Url) -> Result<T, RommError> {
        let response = self
            .send(|| self.http.get(url.clone()).timeout(self.request_timeout))
            .await?;
        let status = response.status();
        let body = read(response, MAX_JSON).await?;
        if !status.is_success() {
            return Err(RommError::Server(failure(status, &body)));
        }
        serde_json::from_slice(&body).map_err(|error| {
            RommError::Server(crate::tr!("unexpected response: {error}", error = error))
        })
    }

    pub(crate) async fn platforms(&self) -> Result<Vec<RemotePlatform>, RommError> {
        self.json(self.url("api/platforms")?).await
    }

    /// The BIOS and firmware files the server keeps for `platform`.
    pub(crate) async fn firmware(&self, platform: u32) -> Result<Vec<RemoteFirmware>, RommError> {
        let mut url = self.url("api/firmware")?;
        url.query_pairs_mut()
            .append_pair("platform_id", &platform.to_string());
        let firmware: Vec<RemoteFirmware> = self.json(url).await?;
        // Older servers ignore the filter.
        Ok(firmware
            .into_iter()
            .filter(|file| file.platform_id == platform)
            .collect())
    }

    /// A firmware file's bytes (`/api/firmware/{id}/content/{file_name}`).
    pub(crate) async fn firmware_content(
        &self,
        firmware: &RemoteFirmware,
    ) -> Result<Vec<u8>, RommError> {
        let mut url = self.url("api/firmware")?;
        url.path_segments_mut()
            .map_err(|()| RommError::Config(crate::tr!("the server address cannot be a base")))?
            .pop_if_empty()
            .push(&firmware.id.to_string())
            .push("content")
            .push(&firmware.file_name);
        let response = self
            .send(|| self.http.get(url.clone()).timeout(FIRMWARE_TIMEOUT))
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(RommError::Server(crate::tr!(
                "firmware: {status}",
                status = status
            )));
        }
        read(response, MAX_FIRMWARE).await
    }

    pub(crate) async fn collections(&self) -> Result<Vec<RemoteCollection>, RommError> {
        self.json(self.url("api/collections")?).await
    }

    /// Every ROM of a platform, page by page. Stops at a short page, at
    /// the server's `total`, or at a page with nothing new (a server that
    /// ignores `offset` and sends no `total` would repeat itself forever).
    pub(crate) async fn roms(&self, platform: u32) -> Result<Vec<RemoteRom>, RommError> {
        let mut roms = Vec::new();
        let mut ids = HashSet::new();
        let mut offset = 0u32;
        for _ in 0..MAX_PAGES {
            let mut url = self.url("api/roms")?;
            url.query_pairs_mut()
                .append_pair("platform_ids", &platform.to_string())
                .append_pair("platform_id", &platform.to_string())
                .append_pair("limit", &PAGE.to_string())
                .append_pair("offset", &offset.to_string())
                .append_pair("order_by", "id")
                .append_pair("order_dir", "asc")
                .append_pair("with_files", "true")
                .append_pair("with_char_index", "false")
                .append_pair("with_filter_values", "false")
                .append_pair("with_rom_id_index", "false");
            let page: RomPage = self.json(url).await?;
            let count = u32::try_from(page.items.len()).unwrap_or(u32::MAX);
            let before = ids.len();
            // Older servers ignore `platform_ids` and use `platform_id`.
            roms.extend(
                page.items
                    .into_iter()
                    .filter(|rom| ids.insert(rom.id) && rom.platform_id == platform),
            );
            offset = offset.saturating_add(count);
            let done = count < PAGE || page.total.is_some_and(|total| u64::from(offset) >= total);
            if count == 0 || done {
                return Ok(roms);
            }
            if ids.len() == before {
                tracing::warn!(
                    platform,
                    offset,
                    "RomM sent the same ROMs again; paging stopped"
                );
                return Ok(roms);
            }
        }
        // Stop the sync rather than take the rest of the list as deleted.
        Err(RommError::Server(crate::tr!(
            "the server's ROM list for platform {platform} doesn't end",
            platform = platform
        )))
    }

    /// An image on the server: a cover or screenshot from its resources
    /// (`/assets/romm/resources/…`), or a user's screenshot (`/api/raw/…`).
    /// RomM sends resource paths with that prefix and a `?ts=` query; a
    /// bare `roms/…` path gets the prefix added.
    pub(crate) async fn resource(&self, path: &str) -> Result<Vec<u8>, RommError> {
        const RESOURCES: &str = "assets/romm/resources/";
        let path = path.trim_start_matches('/');
        let url = if path.starts_with(RESOURCES) || path.starts_with("api/") {
            self.url(path)?
        } else {
            self.url(&format!("{RESOURCES}{path}"))?
        };
        let response = self
            .send(|| self.http.get(url.clone()).timeout(self.request_timeout))
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(RommError::Server(crate::tr!(
                "artwork: {status}",
                status = status
            )));
        }
        read(response, MAX_IMAGE).await
    }

    /// Streams a ROM's content (`/api/roms/{id}/content/{fs_name}`; a zip
    /// for multi-file ROMs) into `part`, continuing an earlier partial
    /// download when the server allows ranges. Returns the file's length.
    /// `room` runs once the server has answered with the file, before its
    /// body: a server that can't be reached or refuses frees nothing.
    /// Cancelling stops at once; the part written so far stays for a resume.
    pub(crate) async fn download(
        &self,
        rom: &RemoteRom,
        part: &Path,
        cancel: &CancellationToken,
        room: impl AsyncFnOnce() -> Result<(), RommError>,
        progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<u64, RommError> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(RommError::Cancelled),
            result = self.download_to(rom, part, room, progress) => result,
        }
    }

    async fn download_to(
        &self,
        rom: &RemoteRom,
        part: &Path,
        room: impl AsyncFnOnce() -> Result<(), RommError>,
        progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<u64, RommError> {
        let mut url = self.url("api/roms")?;
        url.path_segments_mut()
            .map_err(|()| RommError::Config(crate::tr!("the server address cannot be a base")))?
            .pop_if_empty()
            .push(&rom.id.to_string())
            .push("content")
            .push(&rom.fs_name);
        let mut have = tokio::fs::metadata(part)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let response = self
            .send(|| {
                let request = self.http.get(url.clone());
                if have > 0 {
                    request.header(reqwest::header::RANGE, format!("bytes={have}-"))
                } else {
                    request
                }
            })
            .await?;
        let status = response.status();
        let mut response = if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            // The part is complete already, or stale: start over.
            have = 0;
            self.send(|| self.http.get(url.clone())).await?
        } else {
            response
        };
        let status = response.status();
        if !status.is_success() {
            let body = read(response, MAX_JSON).await.unwrap_or_default();
            return Err(match status.as_u16() {
                404 => RommError::Server(crate::tr!("the server no longer has this ROM's files")),
                _ => RommError::Server(failure(status, &body)),
            });
        }
        let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT && have > 0;
        if !resumed {
            have = 0;
        }
        let total = response.content_length().map(|length| length + have);
        room().await?;
        if let Some(parent) = part.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(part)
            .await?;
        let mut written = have;
        progress(written, total);
        while let Some(chunk) = response.chunk().await.map_err(net)? {
            file.write_all(&chunk).await?;
            written += chunk.len() as u64;
            progress(written, total);
        }
        file.flush().await?;
        file.sync_all().await?;
        if total.is_some_and(|total| total != written) {
            return Err(RommError::Server(crate::tr!("the download ended early")));
        }
        Ok(written)
    }
}

async fn read(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>, RommError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(net)? {
        if chunk.len() > max - bytes.len() {
            return Err(RommError::Server(crate::tr!("the response is too large")));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
