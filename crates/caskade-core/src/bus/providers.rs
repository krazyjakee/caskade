//! Provider registries: contracts the core calls.
//! Built-in providers and WASM plugins (adapted by `caskade-plugin-host`)
//! register here.

use async_trait::async_trait;
use caskade_discovery::DetectedGame;
use std::{collections::BTreeMap, path::PathBuf};
use url::Url;

/// Library sources whose id starts with this come from plugins (#126);
/// library scans import them after the built-in sources.
pub const PLUGIN_SOURCE_PREFIX: &str = "plugin:";

/// `library-source`: games from a launcher, store or folder.
#[async_trait]
pub trait LibrarySource: Send + Sync {
    fn id(&self) -> &str;
    /// The games for a library update to import, or none for a source that
    /// [imports itself](Self::imports_itself).
    async fn scan(&self) -> Result<Vec<DetectedGame>, String>;
    /// `true` for a source that imports its own games during `scan` and
    /// returns none, because it reconciles entries the library import
    /// doesn't know about. The ROM scanners do (#135): ROM files, disc sets
    /// and emulator launches. Such sources run after every library update.
    fn imports_itself(&self) -> bool {
        false
    }
}

/// What an install handler is asked about (#55): the input and the
/// installer's probe of it. Handlers read the input; they never write.
#[derive(Debug, Clone)]
pub struct InstallInput {
    /// The local file or folder (a URL input is already downloaded).
    pub path: PathBuf,
    pub artifact: caskade_installer::ArtifactNode,
}

/// `install-handler`: teaches the installer an input it doesn't know. The
/// answer is plan steps; the installer runs them.
#[async_trait]
pub trait InstallHandler: Send + Sync {
    fn id(&self) -> &str;
    /// A candidate for the input, or `None` when the handler doesn't claim it.
    async fn propose(
        &self,
        input: &InstallInput,
    ) -> Result<Option<caskade_installer::Candidate>, String>;
}

/// A download a URL resolver found; the installer fetches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub url: Url,
    /// Sent with the download only (e.g. a store's API key).
    pub headers: BTreeMap<String, String>,
    pub file_name: Option<String>,
    pub size: Option<u64>,
    /// Lowercase hex.
    pub sha256: Option<String>,
    /// The resolving plugin's network grant: the download and its
    /// redirects stay on these hosts and off local and private addresses
    /// the grant doesn't name (CSK-03). `None` for Caskade's own.
    pub allowed_hosts: Option<Vec<String>>,
}

/// `url-resolver`: turns a page URL into downloads.
#[async_trait]
pub trait UrlResolver: Send + Sync {
    fn id(&self) -> &str;
    fn matches(&self, url: &Url) -> bool;
    async fn resolve(&self, url: &Url) -> Result<Vec<DownloadRequest>, String>;
}
