use std::{collections::BTreeMap, future::Future, path::PathBuf, pin::Pin, time::Duration};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Question, host::HostCaps};

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("download cancelled")]
    Cancelled,
    #[error("only HTTP and HTTPS download URLs are supported")]
    UnsupportedScheme,
    #[error("invalid download request: {message}")]
    InvalidRequest { message: String },
    #[error("URL resolver {id} is already registered")]
    DuplicateResolver { id: String },
    #[error("download HTTP request failed: {message}")]
    Http { message: String },
    #[error("download server returned HTTP {status}")]
    Status { status: u16 },
    #[error("this link is a web page, not a download")]
    Html,
    #[error("download redirect limit exceeded (10)")]
    RedirectLimit,
    #[error("refusing HTTPS to HTTP redirect")]
    InsecureRedirect,
    /// A plugin's download left its network grant, or reached a local or
    /// private address the grant doesn't name (CSK-03).
    #[error("the plugin's network permission doesn't cover {host}")]
    NotGranted { host: String },
    #[error("invalid download response: {message}")]
    InvalidResponse { message: String },
    #[error("download SHA-256 checksum does not match")]
    ChecksumMismatch,
    #[error("download exceeds the {limit}-byte limit")]
    TooLarge { limit: u64 },
    #[error("only {available} bytes free; {required} required")]
    DiskFull { available: u64, required: u64 },
    #[error("unsafe download name or directory: {0}")]
    Safety(#[from] crate::safety::SafetyError),
    #[error("download filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("download state is invalid: {0}")]
    State(#[from] serde_json::Error),
    /// Torrent metadata Caskade can't use: malformed, BitTorrent v2 only,
    /// symbolic links, or the engine refused it.
    #[error("unusable torrent: {message}")]
    Torrent { message: String },
    /// No BitTorrent peer answered: for a magnet link's metadata, or with
    /// data for `secs` seconds while downloading.
    #[error(
        "no BitTorrent peers {} for {secs} s ({peers} connected)",
        peers_sent(.metadata)
    )]
    NoPeers {
        metadata: bool,
        secs: u64,
        peers: u32,
    },
}

fn peers_sent(metadata: &bool) -> &'static str {
    if *metadata {
        "sent the torrent's metadata"
    } else {
        "sent data"
    }
}

impl FetchError {
    pub(super) fn http(error: reqwest::Error) -> Self {
        Self::Http {
            message: describe_http_error(error),
        }
    }
    /// Network or server trouble that may pass: what the engine retries
    /// itself, and what a caller may retry later (#158).
    pub fn is_transient(&self) -> bool {
        self.retryable()
    }
    pub(super) fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Http { .. }
                | Self::NoPeers { .. }
                | Self::Status {
                    status: 408 | 429 | 500..=599
                }
        )
    }
}

/// A transport error with its causes ("error sending request: client error
/// (Connect): tcp connect error: Connection refused"), led by what it means
/// for the user when the server could not be reached or stopped answering.
/// The URL is left out: it may carry tokens.
pub(crate) fn describe_http_error(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        let text = cause.to_string();
        if !message.contains(&text) {
            message.push_str(": ");
            message.push_str(&text);
        }
        source = cause.source();
    }
    if error.is_timeout() {
        format!("the server timed out ({message})")
    } else if error.is_connect() {
        format!("the server could not be reached ({message})")
    } else {
        message
    }
}

pub type Result<T> = std::result::Result<T, FetchError>;
pub type ResolveFuture<'a> = Pin<Box<dyn Future<Output = Result<Resolution>> + Send + 'a>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub url: Url,
    pub headers: BTreeMap<String, String>,
    pub file_name: Option<String>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    /// A plugin's network grant, for a download the plugin asked for: the
    /// URL and every redirect must be on these hosts, and names must
    /// resolve to public addresses unless the grant names the address
    /// (CSK-03).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_hosts: Option<Vec<String>>,
}

impl DownloadRequest {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            headers: BTreeMap::new(),
            file_name: None,
            size: None,
            sha256: None,
            allowed_hosts: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Resolution {
    Downloads(Vec<DownloadRequest>),
    NeedsUser(Question),
}

/// Providers return download data, never write files. Specific providers are
/// consulted before the built-in direct-download fallback.
pub trait UrlResolver: Send + Sync {
    fn id(&self) -> &str;
    fn claims(&self, url: &Url) -> bool;
    fn resolve<'a>(&'a self, url: &'a Url, host: &'a HostCaps) -> ResolveFuture<'a>;
}

#[derive(Debug, Clone)]
pub struct FetchConfig {
    pub concurrent_downloads: usize,
    pub keep_downloads: bool,
    /// Retries after failed attempts in a row. An attempt that got further
    /// into the file than any before it starts the count again.
    pub retries: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    /// How long connecting to a server (TCP and TLS) may take.
    pub connect_timeout: Duration,
    /// How long a request may wait for the server's response or its next
    /// bytes. A download may take as long as it needs while bytes arrive.
    pub stall_timeout: Duration,
    pub disk_margin_bytes: u64,
    pub max_bytes: u64,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            concurrent_downloads: 2,
            keep_downloads: false,
            retries: 3,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(8),
            connect_timeout: Duration::from_secs(20),
            stall_timeout: Duration::from_secs(60),
            disk_margin_bytes: 16 * 1024 * 1024,
            max_bytes: 100 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedFile {
    pub path: PathBuf,
    pub source_url: Url,
    pub final_url: Url,
    pub size: u64,
    pub sha256: String,
    /// Plain HTTP is accepted for explicit user input and recorded for manifests.
    pub insecure_transport: bool,
    /// The source's strong validator for what was downloaded: its ETag, or
    /// a Last-Modified date the source's clock vouches for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<String>,
}
