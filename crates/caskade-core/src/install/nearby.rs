//! Downloads from another of the user's devices on the local network (#236):
//! when one device has downloaded a file, the others get it from that device
//! instead of the internet, faster and with no internet at all.
//!
//! **Keeping.** A download the installer finished from its source is kept in
//! `<cache>/peer-downloads/<sha256>/<file>` (a hard link where it can be, so
//! no second copy while the install's own download is still there), offered
//! through the copy store as `download:<sha256>`, and listed in `index.json`
//! with its URLs, size, SHA-256 and the source's validator (ETag or
//! Last-Modified). Kept files past [`BUDGET`] go oldest first, and only when a
//! new one is kept, which is after a download from the internet.
//!
//! **Telling.** Each sync exchange's blob round carries a `download` note
//! listing them; this device lets that peer fetch each one (the copy gate).
//! The other device remembers what each peer has.
//!
//! **Finding.** Before the installer downloads, [`PeerDownloads`] (its
//! [`LocalSource`]) looks for the same file on a paired device it reaches on
//! the local network: by SHA-256 where the request names one, otherwise by
//! URL. A URL match is only taken when the source, asked with a `HEAD`,
//! answers with the same validator and size, or can't be reached. The file
//! comes over the direct connection only (game files never relay); the
//! installer then checks it against the request and against what the peer
//! said it was, and downloads from the source when anything fails.
//!
//! `core.sync.peerDownloads` off: nothing is kept, offered or asked for.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use caskade_installer::exec::CancellationToken;
use caskade_installer::fetch::local::LocalFuture;
use caskade_installer::fetch::{DownloadRequest, DownloadedFile, LocalSource, Near};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::devices::{DeviceLink, Devices};
use crate::settings::SettingsService;
use crate::sync::blobs::{BlobKind, BlobWriter};
use crate::sync::policy::{Category, SyncSettings};
use crate::transfer::Transfers;
use crate::{Error, Result, lock};

/// How much kept downloads may take.
pub const BUDGET: u64 = 32 * 1024 * 1024 * 1024;
const INDEX: &str = "index.json";
/// How long the source gets to answer whether a URL still has that file.
const SOURCE_WAIT: Duration = Duration::from_secs(5);

/// A download this device keeps for its others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    sha256: String,
    size: u64,
    /// Its file name.
    name: String,
    /// The URLs it was downloaded from.
    urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    validator: Option<String>,
    /// The copy store's collection holding it.
    root: String,
    #[serde(default)]
    kept_ms: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Note {
    #[serde(default)]
    kept: Vec<Kept>,
}

/// Downloads kept for and fetched from paired devices. Cheap to clone.
#[derive(Clone)]
pub struct PeerDownloads {
    inner: Arc<Inner>,
}

struct Inner {
    transfers: Transfers,
    devices: Devices,
    settings: SettingsService,
    dir: PathBuf,
    budget: u64,
    kept: tokio::sync::Mutex<Vec<Kept>>,
    /// What each paired device said it keeps, by device id.
    heard: Mutex<HashMap<String, Vec<Kept>>>,
    http: reqwest::Client,
}

fn sha_of(request: &DownloadRequest) -> Option<String> {
    let sha = request.sha256.as_deref()?.trim();
    let sha = sha.strip_prefix("sha256:").unwrap_or(sha);
    Some(sha.to_ascii_lowercase())
}

/// A validator in a response, as the installer records one.
fn validator_of(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let header = |name| headers.get(name)?.to_str().ok().map(str::to_owned);
    match header(reqwest::header::ETAG) {
        Some(etag) => (!etag.starts_with("W/")).then_some(etag),
        None => header(reqwest::header::LAST_MODIFIED),
    }
}

impl PeerDownloads {
    /// Keeps downloads in `<cache>/peer-downloads/`.
    pub fn new(
        transfers: Transfers,
        devices: Devices,
        settings: SettingsService,
        cache: &Path,
    ) -> Self {
        Self::with_budget(transfers, devices, settings, cache, BUDGET)
    }

    /// [`new`](Self::new), keeping at most `budget` bytes.
    pub fn with_budget(
        transfers: Transfers,
        devices: Devices,
        settings: SettingsService,
        cache: &Path,
        budget: u64,
    ) -> Self {
        let dir = cache.join("peer-downloads");
        let kept: Vec<Kept> = std::fs::read(dir.join(INDEX))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let kept = kept
            .into_iter()
            .filter(|kept| dir.join(&kept.sha256).join(&kept.name).is_file())
            .collect();
        Self {
            inner: Arc::new(Inner {
                transfers,
                devices,
                settings,
                dir,
                budget,
                kept: tokio::sync::Mutex::new(kept),
                heard: Mutex::default(),
                http: reqwest::Client::builder()
                    .timeout(SOURCE_WAIT)
                    .build()
                    .unwrap_or_default(),
            }),
        }
    }

    /// How many downloads `device_id` said it keeps, at the last exchange.
    pub fn heard_from(&self, device_id: &str) -> usize {
        lock(&self.inner.heard).get(device_id).map_or(0, Vec::len)
    }

    async fn on(&self) -> bool {
        self.inner
            .settings
            .read_or_default::<SyncSettings>("core.sync")
            .await
            .peer_downloads
    }

    fn save(&self, kept: &[Kept]) {
        let Ok(bytes) = serde_json::to_vec_pretty(kept) else {
            return;
        };
        let path = self.inner.dir.join(INDEX);
        let partial = self.inner.dir.join(".index.json.partial");
        if std::fs::write(&partial, bytes).is_ok() {
            let _ = std::fs::rename(&partial, &path);
        }
    }

    async fn keep_file(&self, request: &DownloadRequest, file: &DownloadedFile) -> Result<()> {
        if !self.on().await {
            return Ok(());
        }
        let url = request.url.to_string();
        let mut kept = self.inner.kept.lock().await;
        if let Some(known) = kept.iter_mut().find(|kept| kept.sha256 == file.sha256) {
            if !known.urls.contains(&url) {
                known.urls.push(url);
            }
            known.validator = file.validator.clone().or(known.validator.take());
            known.kept_ms = crate::sync::clock::now_ms();
            self.save(&kept);
            return Ok(());
        }
        if file.size > self.inner.budget {
            return Ok(());
        }
        let name = file
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Invalid("a download without a file name".into()))?
            .to_owned();
        let folder = self.inner.dir.join(&file.sha256);
        let target = folder.join(&name);
        let source = file.path.clone();
        let linked = target.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            std::fs::create_dir_all(linked.parent().unwrap_or(Path::new(".")))?;
            if std::fs::hard_link(&source, &linked).is_err() {
                std::fs::copy(&source, &linked)?;
            }
            Ok(())
        })
        .await
        .map_err(|error| Error::Invalid(error.to_string()))??;
        let offered = match self
            .inner
            .transfers
            .offer_folder(&folder, &format!("download:{}", file.sha256), |_, _| {})
            .await
        {
            Ok(offered) => offered,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&folder);
                return Err(error);
            }
        };
        kept.push(Kept {
            sha256: file.sha256.clone(),
            size: file.size,
            name,
            urls: vec![url],
            validator: file.validator.clone(),
            root: offered.root,
            kept_ms: crate::sync::clock::now_ms(),
        });
        // Over the budget: the oldest go.
        kept.sort_by_key(|kept| std::cmp::Reverse(kept.kept_ms));
        let mut total = 0;
        let mut gone = Vec::new();
        kept.retain(|kept| {
            total += kept.size;
            let stays = total <= self.inner.budget;
            if !stays {
                gone.push(kept.sha256.clone());
            }
            stays
        });
        self.save(&kept);
        drop(kept);
        for sha in gone {
            let _ = self
                .inner
                .transfers
                .release(&format!("download:{sha}"))
                .await;
            let _ = tokio::fs::remove_dir_all(self.inner.dir.join(sha)).await;
        }
        Ok(())
    }

    /// Whether the source still serves what `kept` was downloaded as, at
    /// `request`'s URL: the same validator and size, or no answer at all.
    async fn source_agrees(&self, request: &DownloadRequest, kept: &Kept) -> bool {
        let mut head = self.inner.http.head(request.url.clone());
        for (name, value) in &request.headers {
            head = head.header(name, value);
        }
        let Ok(response) = head.send().await else {
            return true;
        };
        if !response.status().is_success() {
            return true;
        }
        let headers = response.headers();
        if let (Some(theirs), Some(now)) = (&kept.validator, validator_of(headers))
            && theirs != &now
        {
            return false;
        }
        let length = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok()?.parse::<u64>().ok());
        length.is_none_or(|length| length == kept.size)
    }

    /// Paired devices reached on the local network that keep a file
    /// `request` asks for, with their direct addresses.
    async fn candidates(
        &self,
        request: &DownloadRequest,
    ) -> Vec<(caskade_sync::EndpointId, Vec<SocketAddr>, Kept)> {
        let sha = sha_of(request);
        let url = request.url.to_string();
        let matching: Vec<(String, Kept)> = lock(&self.inner.heard)
            .iter()
            .flat_map(|(peer, kept)| kept.iter().map(move |kept| (peer.clone(), kept.clone())))
            .filter(|(_, kept)| match &sha {
                Some(sha) => &kept.sha256 == sha,
                None => {
                    kept.urls.contains(&url) && request.size.is_none_or(|size| size == kept.size)
                }
            })
            .collect();
        if matching.is_empty() {
            return Vec::new();
        }
        let devices = self.inner.devices.status().await.devices;
        let mut found = Vec::new();
        for (peer, kept) in matching {
            let Some(device) = devices.iter().find(|device| device.id == peer) else {
                continue;
            };
            if device.link != DeviceLink::Lan {
                continue;
            }
            let addrs: Vec<SocketAddr> = device
                .addresses
                .iter()
                .filter_map(|addr| addr.parse().ok())
                .collect();
            let Ok(id) = peer.parse() else { continue };
            if addrs.is_empty() || (sha.is_none() && !self.source_agrees(request, &kept).await) {
                continue;
            }
            found.push((id, addrs, kept));
        }
        found
    }

    async fn fetch(
        &self,
        peer: caskade_sync::EndpointId,
        addrs: &[SocketAddr],
        kept: &Kept,
        dir: &Path,
    ) -> Result<PathBuf> {
        caskade_installer::fetch::safe_file_name(&kept.name)
            .map_err(|_| Error::Invalid("a file name that isn't one".into()))?;
        let tag = format!(
            "near:{}:{}",
            kept.sha256,
            crate::sync::clock::now_ms().unsigned_abs()
        );
        let transfers = &self.inner.transfers;
        let fetched = async {
            transfers
                .fetch_direct(peer, addrs, &kept.root, &tag, |_| {})
                .await?;
            transfers.export(&kept.root, dir).await
        }
        .await;
        let _ = transfers.release(&tag).await;
        fetched?;
        let path = dir.join(&kept.name);
        if path.is_file() {
            Ok(path)
        } else {
            Err(Error::Invalid(
                "the other device sent something else".into(),
            ))
        }
    }

    async fn find_file(
        &self,
        request: &DownloadRequest,
        dir: &Path,
        cancel: &CancellationToken,
    ) -> Option<Near> {
        if !self.on().await {
            return None;
        }
        for (peer, addrs, kept) in self.candidates(request).await {
            let fetched = tokio::select! {
                _ = cancel.cancelled() => return None,
                fetched = self.fetch(peer, &addrs, &kept, dir) => fetched,
            };
            match fetched {
                Ok(path) => {
                    tracing::info!(peer = %peer, size = kept.size, "download from a device nearby");
                    return Some(Near {
                        path,
                        sha256: kept.sha256,
                        size: kept.size,
                        validator: kept.validator,
                    });
                }
                Err(error) => {
                    tracing::info!(peer = %peer, %error, "a device nearby couldn't send a download");
                }
            }
        }
        None
    }
}

impl LocalSource for PeerDownloads {
    fn find<'a>(
        &'a self,
        request: &'a DownloadRequest,
        dir: &'a Path,
        cancel: &'a CancellationToken,
    ) -> LocalFuture<'a, Option<Near>> {
        Box::pin(self.find_file(request, dir, cancel))
    }

    fn keep<'a>(
        &'a self,
        request: &'a DownloadRequest,
        file: &'a DownloadedFile,
    ) -> LocalFuture<'a, ()> {
        Box::pin(async move {
            if let Err(error) = self.keep_file(request, file).await {
                tracing::warn!(%error, "couldn't keep a download for other devices");
            }
        })
    }
}

/// The note on each exchange; no blobs travel as `download` itself.
#[async_trait]
impl BlobKind for PeerDownloads {
    fn name(&self) -> &'static str {
        "download"
    }

    fn category(&self) -> Category {
        Category::Library
    }

    fn own_setting(&self) -> bool {
        true
    }

    async fn announce(&self, peer: &str) -> Option<Value> {
        if !self.on().await {
            return None;
        }
        let id = peer.parse().ok()?;
        let kept = self.inner.kept.lock().await.clone();
        let kept: Vec<Kept> = kept
            .into_iter()
            .filter(|kept| self.inner.transfers.allow(id, &kept.root).is_ok())
            .collect();
        if kept.is_empty() {
            return None;
        }
        serde_json::to_value(Note { kept }).ok()
    }

    async fn heard(&self, peer: &str, note: Value) {
        let Ok(note) = serde_json::from_value::<Note>(note) else {
            return;
        };
        lock(&self.inner.heard).insert(peer.to_owned(), note.kept);
    }

    async fn wanted(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn has(&self, _hash: &str) -> bool {
        false
    }

    async fn read(&self, _hash: &str, _offset: u64, _len: usize) -> Result<Vec<u8>> {
        Err(Error::Invalid("downloads travel as copies".into()))
    }

    async fn receive(&self, _hash: &str) -> Result<Box<dyn BlobWriter>> {
        Err(Error::Invalid("downloads travel as copies".into()))
    }
}
