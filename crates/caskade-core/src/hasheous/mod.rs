//! Offline Hasheous data dumps (<https://hasheous.org/index.html?page=dumps>).
//!
//! Hasheous publishes one zip per platform, with a JSON file per game: its
//! ROM hashes and the ids other metadata sources (IGDB, RetroAchievements,
//! …) give it. [`Hasheous`] keeps the dumps the library needs in the cache
//! folder, indexes them in a local SQLite file and answers hash and title
//! lookups from it, so matching a ROM needs no online API.
//!
//! [`Hasheous::watch`] keeps them current in the background. It wants the
//! dump of every platform a configured emulator runs ([`PlatformIds`]'s
//! `hasheous`), the Windows dump when the library has PC games and the Mac
//! dump for native games on macOS. Hasheous has no Linux dump.
//!
//! [`PlatformIds`]: crate::emulation::PlatformIds

#[cfg(feature = "net")]
mod download;
mod index;
#[cfg(test)]
mod tests;

use crate::{
    Error, Result,
    bus::{Bus, Event},
    db::LibraryStore,
    emulation::{EmulationService, OperatingSystem},
};
use index::Index;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, OnceCell};

pub const BASE_URL: &str = "https://hasheous.org";
/// PC games of every OS: Hasheous files Linux and most Mac ports here too.
pub const WINDOWS: &str = "Microsoft Windows";
pub const MAC: &str = "Apple Mac";

/// How old the cached dump listing may get before it is fetched again.
const LISTING_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the watcher checks for updated dumps without other triggers.
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);
/// When to try again after a failed download.
const RETRY: Duration = Duration::from_secs(15 * 60);
/// Library imports add many games at once: wait for them to settle.
const SETTLE: Duration = Duration::from_secs(5);

/// Whether `name` can be a dump: a plain file stem, without `.zip`.
pub fn valid_dump(name: &str) -> bool {
    !name.trim().is_empty()
        && name.trim() == name
        && !name.contains(['/', '\\', ':'])
        && !name.contains("..")
        && !name.ends_with(".zip")
        && !name.chars().any(char::is_control)
}

/// One dump in Hasheous's listing (`/api/v1/Dumps/platforms`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    /// The file name, with `.zip`.
    name: String,
    size_bytes: u64,
    #[serde(default)]
    last_modified_utc: Option<String>,
    md5hash: String,
}

impl Listed {
    fn dump(&self) -> Option<&str> {
        self.name.strip_suffix(".zip")
    }
}

/// The listing as last fetched (`listing.json`).
#[derive(Debug, Default, Serialize, Deserialize)]
struct Listing {
    /// Seconds since the Unix epoch.
    fetched: u64,
    dumps: Vec<Listed>,
}

/// An id a metadata source gives a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MetadataId {
    /// `IGDB`, `RetroAchievements`, `TheGamesDb`, …
    pub source: String,
    pub id: String,
    pub link: Option<String>,
}

/// A game in a dump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DumpGame {
    /// The dump (platform) it is in.
    pub dump: String,
    /// Hasheous's id.
    pub id: String,
    pub name: String,
    pub metadata: Vec<MetadataId>,
}

/// A ROM (or disc image) of a game, with the hashes the dump has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DumpRom {
    pub name: String,
    pub size: Option<u64>,
    pub crc: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RomMatch {
    pub game: DumpGame,
    pub rom: DumpRom,
}

/// What is known about a file. The strongest hash given decides the match;
/// a CRC alone also needs the size when one is given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RomHashes {
    pub sha256: Option<String>,
    pub sha1: Option<String>,
    pub md5: Option<String>,
    pub crc: Option<String>,
    pub size: Option<u64>,
}

/// A dump that is downloaded and indexed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LocalDump {
    pub name: String,
    pub md5: String,
    pub size: u64,
    pub modified: Option<String>,
    pub games: u32,
}

/// What one [`Hasheous::sync`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub downloaded: Vec<String>,
    pub indexed: Vec<String>,
    /// Wanted, but Hasheous doesn't have it.
    pub unavailable: Vec<String>,
    /// Dump and error; also each dump without a copy yet while Hasheous
    /// can't be reached.
    pub failed: Vec<(String, String)>,
}

/// The dumps' cache folder and index.
#[derive(Clone)]
pub struct Hasheous {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    base: String,
    index: OnceCell<Index>,
    /// One sync at a time.
    sync: Mutex<()>,
    /// Devices nearby that may have a dump already (#277).
    #[cfg(feature = "net")]
    near: std::sync::Mutex<Option<crate::install::nearby::PeerDownloads>>,
}

impl Hasheous {
    /// Keeps the dumps, their listing and the index in `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_base_url(dir, BASE_URL)
    }

    /// Downloads from another server (tests).
    pub fn with_base_url(dir: impl Into<PathBuf>, base: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                dir: dir.into(),
                base: base.into().trim_end_matches('/').to_owned(),
                index: OnceCell::new(),
                sync: Mutex::new(()),
                #[cfg(feature = "net")]
                near: std::sync::Mutex::default(),
            }),
        }
    }

    /// Dumps are asked of devices nearby first, and kept for them (#277):
    /// a device that has never been online gets them from one that has.
    #[cfg(feature = "net")]
    pub fn set_nearby(&self, near: crate::install::nearby::PeerDownloads) {
        *crate::lock(&self.inner.near) = Some(near);
    }

    /// The dump file `name` from a device nearby, at `zip`, and its MD5:
    /// the listing's size and MD5 when there is a listing.
    #[cfg(feature = "net")]
    async fn fetch_nearby(
        &self,
        name: &str,
        zip: &std::path::Path,
        listed: Option<&Listed>,
    ) -> Option<String> {
        let near = crate::lock(&self.inner.near).clone()?;
        let url = download::dump_url(&self.inner.base, name).ok()?;
        let cancel = tokio_util::sync::CancellationToken::new();
        if !near.fetch_to(&url, zip, &cancel, &|_, _| {}).await {
            return None;
        }
        let md5 = md5_file(zip.to_owned()).await.ok()?;
        let size = tokio::fs::metadata(zip).await.ok()?.len();
        if listed.is_some_and(|listed| {
            listed.size_bytes != size || !listed.md5hash.eq_ignore_ascii_case(&md5)
        }) {
            let _ = tokio::fs::remove_file(zip).await;
            return None;
        }
        tracing::info!(dump = name, "Hasheous dump from a device nearby");
        Some(md5.to_ascii_lowercase())
    }

    #[cfg(not(feature = "net"))]
    async fn fetch_nearby(
        &self,
        _name: &str,
        _zip: &std::path::Path,
        _listed: Option<&Listed>,
    ) -> Option<String> {
        None
    }

    /// Keeps a dump downloaded from Hasheous for the other devices.
    #[cfg(feature = "net")]
    async fn keep_nearby(&self, name: &str, zip: &std::path::Path) {
        let near = crate::lock(&self.inner.near).clone();
        if let (Some(near), Ok(url)) = (near, download::dump_url(&self.inner.base, name)) {
            near.keep_path(&url, zip, None).await;
        }
    }

    async fn index(&self) -> Result<&Index> {
        self.inner
            .index
            // A damaged cache is started again, not an error forever (#160).
            .get_or_try_init(|| {
                crate::cache_db::open(self.inner.dir.join("index.sqlite"), "Hasheous", Index::open)
            })
            .await
    }

    fn zip(&self, dump: &str) -> PathBuf {
        self.inner.dir.join("dumps").join(format!("{dump}.zip"))
    }

    /// The ROMs with these hashes, strongest hash first.
    pub async fn lookup(&self, hashes: &RomHashes) -> Result<Vec<RomMatch>> {
        self.index().await?.lookup(hashes).await
    }

    /// Games named `title` (or with it as an alias), ignoring case, in
    /// `dumps` (every dump when empty).
    pub async fn search(&self, title: &str, dumps: &[String], limit: u32) -> Result<Vec<DumpGame>> {
        self.index().await?.search(title, dumps, limit).await
    }

    /// The dumps downloaded and indexed.
    pub async fn dumps(&self) -> Result<Vec<LocalDump>> {
        self.index().await?.dumps().await
    }

    /// Brings `wanted` up to date: downloads missing and changed dumps and
    /// indexes them. A copy already on disk is indexed without downloading
    /// it again. Dumps no longer wanted are kept.
    pub async fn sync(&self, wanted: &BTreeSet<String>) -> Result<SyncReport> {
        let _sync = self.inner.sync.lock().await;
        let mut report = SyncReport::default();
        if wanted.is_empty() {
            return Ok(report);
        }
        let index = self.index().await?;
        let (listing, listing_error) = self.listing().await;
        for dump in wanted {
            if !valid_dump(dump) {
                report
                    .failed
                    .push((dump.clone(), crate::tr!("not a dump name")));
                continue;
            }
            let listed = listing.as_ref().and_then(|listing| {
                listing
                    .dumps
                    .iter()
                    .find(|listed| listed.dump() == Some(dump))
            });
            let zip = self.zip(dump);
            let indexed = index.md5(dump).await?;
            let present = tokio::fs::try_exists(&zip).await?;
            match (listed, &indexed) {
                (Some(listed), Some(md5))
                    if present && listed.md5hash.eq_ignore_ascii_case(md5) =>
                {
                    continue;
                }
                // Offline, or Hasheous dropped it: keep what we have.
                (None, Some(_)) if present => continue,
                _ => {}
            }
            let local = if present {
                Some(md5_file(zip.clone()).await?)
            } else {
                None
            };
            let current = match (listed, &local) {
                (Some(listed), Some(md5)) => listed.md5hash.eq_ignore_ascii_case(md5),
                (None, Some(_)) => true,
                _ => false,
            };
            let md5 = if current {
                local.unwrap_or_default()
            } else if let Some(listed) = listed {
                let _activity = caskade_jobs::global()
                    .activity(crate::tr!("Downloading {dump} database", dump = dump));
                if let Some(md5) = self.fetch_nearby(&listed.name, &zip, Some(listed)).await {
                    report.downloaded.push(dump.clone());
                    md5
                } else {
                    match download::fetch(&self.inner.base, listed, &zip).await {
                        Ok(()) => {
                            tracing::info!(
                                dump,
                                bytes = listed.size_bytes,
                                "Hasheous dump downloaded"
                            );
                            #[cfg(feature = "net")]
                            self.keep_nearby(&listed.name, &zip).await;
                            report.downloaded.push(dump.clone());
                            listed.md5hash.to_ascii_lowercase()
                        }
                        Err(error) => {
                            tracing::warn!(dump, %error, "Hasheous dump download failed");
                            report.failed.push((dump.clone(), error.to_string()));
                            continue;
                        }
                    }
                }
            } else if listing.is_some() {
                tracing::info!(dump, "Hasheous has no such dump");
                report.unavailable.push(dump.clone());
                continue;
            } else if let Some(md5) = self.fetch_nearby(&format!("{dump}.zip"), &zip, None).await {
                // Offline, but a device nearby has it.
                report.downloaded.push(dump.clone());
                md5
            } else {
                // Offline with no copy yet: a failure, so the watch tries
                // again soon.
                let error = listing_error
                    .clone()
                    .unwrap_or_else(|| crate::tr!("Hasheous can't be reached"));
                report.failed.push((dump.clone(), error));
                continue;
            };
            let size = tokio::fs::metadata(&zip).await?.len();
            let modified = listed.and_then(|listed| listed.last_modified_utc.clone());
            match index.import(dump, &md5, size, modified, zip).await {
                Ok(games) => {
                    tracing::info!(dump, games, "Hasheous dump indexed");
                    report.indexed.push(dump.clone());
                }
                Err(error) => {
                    tracing::warn!(dump, %error, "Hasheous dump could not be indexed");
                    report.failed.push((dump.clone(), error.to_string()));
                }
            }
        }
        Ok(report)
    }

    /// The cached listing, fetched again once it is older than a day. When
    /// that fails (offline), the stale copy, and why it failed.
    async fn listing(&self) -> (Option<Listing>, Option<String>) {
        let path = self.inner.dir.join("listing.json");
        let cached: Option<Listing> = tokio::fs::read(&path)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let now = now();
        if cached
            .as_ref()
            .is_some_and(|cached| now.saturating_sub(cached.fetched) < LISTING_TTL.as_secs())
        {
            return (cached, None);
        }
        match download::listing(&self.inner.base).await {
            Ok(dumps) => {
                let listing = Listing {
                    fetched: now,
                    dumps,
                };
                if let Err(error) =
                    write_atomic(&path, &serde_json::to_vec(&listing).unwrap_or_default()).await
                {
                    tracing::warn!(%error, "Hasheous listing not cached");
                }
                (Some(listing), None)
            }
            Err(error) => {
                tracing::warn!(%error, "Hasheous listing unavailable");
                (cached, Some(error.to_string()))
            }
        }
    }

    /// Syncs [`wanted`] dumps now and again: at start, when emulators are
    /// configured, after library changes and every few hours. Failures,
    /// being offline without a copy of a dump among them, retry after
    /// [`RETRY`].
    pub fn watch(
        &self,
        bus: &Bus,
        emulation: EmulationService,
        store: LibraryStore,
    ) -> tokio::task::JoinHandle<()> {
        use tokio::sync::broadcast::error::{RecvError, TryRecvError};
        let this = self.clone();
        let mut events = bus.stream();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(RECHECK);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = timer.tick() => {}
                    event = events.recv() => match event {
                        Ok(event) if relevant(&event) => tokio::time::sleep(SETTLE).await,
                        Ok(_) => continue,
                        Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => break,
                    },
                }
                // One sync covers every event that arrived meanwhile.
                loop {
                    match events.try_recv() {
                        Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Closed) => return,
                    }
                }
                let outcome = match wanted(&emulation, &store).await {
                    Ok(dumps) => this.sync(&dumps).await,
                    Err(error) => Err(error),
                };
                if let Err(error) = &outcome {
                    tracing::warn!(%error, "Hasheous dumps not updated");
                }
                if retry_soon(&outcome) {
                    timer.reset_after(RETRY);
                }
            }
        })
    }
}

/// Whether a sync failed, so the watch tries again after [`RETRY`] rather
/// than [`RECHECK`].
fn retry_soon(outcome: &Result<SyncReport>) -> bool {
    outcome
        .as_ref()
        .map_or(true, |report| !report.failed.is_empty())
}

/// Events that can change which dumps are wanted.
fn relevant(event: &Event) -> bool {
    match event {
        Event::EmulationChanged { scope } => scope == "emulators",
        Event::LibraryScanCompleted { .. } | Event::LibraryGameAdded { .. } => true,
        _ => false,
    }
}

/// The dumps the library needs: each platform a configured emulator runs,
/// Windows for PC games and Mac for native games on macOS.
pub async fn wanted(
    emulation: &EmulationService,
    store: &LibraryStore,
) -> Result<BTreeSet<String>> {
    let catalog = emulation.catalog();
    let mut wanted: BTreeSet<String> = emulation
        .emulators()
        .await?
        .iter()
        .flat_map(|view| &view.profiles)
        .flat_map(|profile| &profile.platforms)
        .filter_map(|platform| catalog.platform(platform)?.ids.hasheous.clone())
        .collect();
    let runtimes = store.pc_runtimes().await?;
    if !runtimes.is_empty() {
        wanted.insert(WINDOWS.to_owned());
    }
    if emulation.os() == OperatingSystem::Macos
        && runtimes.iter().any(|runtime| runtime == "native")
    {
        wanted.insert(MAC.to_owned());
    }
    Ok(wanted)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

async fn md5_file(path: PathBuf) -> Result<String> {
    caskade_jobs::blocking(
        crate::tr!("Checking game database"),
        move || -> Result<String> {
            use md5::Digest;
            let mut file = std::fs::File::open(path)?;
            let mut hasher = md5::Md5::new();
            std::io::copy(&mut file, &mut hasher)?;
            Ok(hex(&hasher.finalize()))
        },
    )
    .await
    .map_err(|error| Error::Io(std::io::Error::other(error)))?
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = path.with_extension("part");
    tokio::fs::write(&part, bytes).await?;
    tokio::fs::rename(&part, path).await?;
    Ok(())
}

/// Builds without `net` index and query the dumps already on disk.
#[cfg(not(feature = "net"))]
mod download {
    use super::Listed;
    use crate::{Error, Result};
    use std::path::Path;

    fn offline() -> Error {
        Error::Io(std::io::Error::other(crate::tr!(
            "built without network support"
        )))
    }

    pub(super) async fn listing(_base: &str) -> Result<Vec<Listed>> {
        Err(offline())
    }

    pub(super) async fn fetch(_base: &str, _listed: &Listed, _to: &Path) -> Result<()> {
        Err(offline())
    }
}
