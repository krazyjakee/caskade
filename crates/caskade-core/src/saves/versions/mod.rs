//! Save versions (#239): a game's saves kept after each session and carried
//! to the user's other devices.
//!
//! **A version** is the release's save files at one moment, listed in a
//! file-list blob (each file by its folder name, relative path and SHA-256)
//! whose hash is the version's `content_hash`. Files are kept once by
//! content (`BlobStore`), so versions share what didn't change. Paths are
//! relative to named folders ([`Roots`]): `<winAppData>` and friends from
//! the manifest, `{save_data}` and friends from an emulator definition, so a
//! save made on Windows restores into a Proton prefix and back.
//!
//! **History.** Each version names its parents: the version this device's
//! files matched when it was made (its *head*, per game and account). The
//! versions nothing else descends from are the *tips*. One tip that isn't
//! this device's head came from another device: [`SaveVersions::before_play`]
//! restores it. More than one means both devices played since they last
//! agreed: nothing is restored, and the launch asks which to use; the
//! choice becomes a version descending from all of them.
//!
//! **Sync.** Versions made here travel as `saveVersion` records in the
//! change log (category Saves), their files as blobs on the same session
//! (`sync::blobs`). With Saves set to This device only, neither leaves.
//! Each device keeps `core.saves.keep` versions per game (20), never
//! pruning a tip or a head.

mod blobs;
#[cfg(feature = "net")]
mod net;
mod portable;
mod shared;
mod store;

pub use blobs::{BlobStore, Incoming, hash_bytes, hash_file};
#[cfg(feature = "net")]
pub use net::{SaveBlobs, SyncRefresh};
pub use portable::Roots;
pub(crate) use store::{sync_apply, sync_snapshot};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Saves;
use crate::bus::{Bus, Intercept, Interceptor};
use crate::db::LibraryStore;
use crate::launch::{LaunchAfter, LaunchEvent, SessionStatus};
use crate::settings::SettingsService;
use crate::sync::policy::{Category, SyncSettings};
use crate::{Error, Result};

/// Versions kept per game unless `core.saves.keep` says otherwise.
pub const KEEP: u32 = 20;
/// How long a launch waits for other devices before going ahead.
pub const WAIT: Duration = Duration::from_secs(4);
/// A version holds at most this much; bigger "saves" are refused.
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 20_000;

/// One version of a game's saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveVersion {
    /// The same on every device.
    pub id: String,
    pub game_id: String,
    /// The account that played; empty when none was signed in.
    pub account_id: String,
    /// The device that made it: its sync id, or `local`.
    pub device_id: String,
    pub session_id: Option<String>,
    pub created_at_ms: i64,
    /// `sha256:…` of its file list: equal saves, equal hashes.
    pub content_hash: String,
    pub size_bytes: u64,
    pub file_count: u32,
    pub parents: Vec<String>,
    pub reason: VersionReason,
    /// Made on this device.
    pub local: bool,
}

/// Why a version was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum VersionReason {
    /// After a play session.
    Session,
    /// The files changed outside Caskade, found before a launch.
    Found,
    /// What was there before a restore.
    BeforeRestore,
    /// The user restored an older version.
    Restored,
    /// The user chose between versions both devices made.
    Chosen,
    /// Someone shared it through a link (#248).
    Shared,
}

/// One file of a version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ManifestFile {
    /// The named folder it's under (`<winAppData>`, `{save_data}`), or
    /// empty for an absolute path.
    pub root: String,
    pub path: String,
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    files: Vec<ManifestFile>,
}

impl Manifest {
    fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn hash(&self) -> String {
        hash_bytes(&self.bytes())
    }

    fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

/// A game's versions, as its Saves panel shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveHistory {
    /// The account playing now; empty when none is signed in.
    pub account_id: String,
    /// Every account's versions, newest first.
    pub versions: Vec<VersionState>,
    /// The account's versions both devices made since they last agreed,
    /// when there's more than one: the next Play asks which to use.
    pub conflict: Vec<String>,
    /// Whether Saves sync with other devices here.
    pub syncing: bool,
}

/// One version in a game's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VersionState {
    pub version: SaveVersion,
    /// This device's save files are this version, for the account playing.
    pub current: bool,
    /// Every file of it is on this device, so it can be restored.
    pub complete: bool,
}

/// What happened before a launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SaveCheck {
    /// No version of this game's saves exists for the account.
    NoSaves,
    /// This device already has the newest save.
    UpToDate,
    /// A newer save from another device was put in place.
    Restored { version: SaveVersion },
    /// A newer save exists on another device, but its files haven't
    /// arrived: the game starts with the save here.
    NotArrived { version: SaveVersion },
    /// Both devices played since they last agreed: choose one
    /// ([`SaveVersions::choose`]). Newest first.
    Conflict { versions: Vec<SaveVersion> },
}

/// The check before a launch, and whether other devices could be asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BeforePlay {
    pub check: SaveCheck,
    /// `false` when a paired device couldn't be reached in time: there may
    /// be a newer save this device hasn't seen.
    pub checked: bool,
    pub warnings: Vec<String>,
}

/// Asks other devices for what they have.
#[async_trait]
pub trait RemoteSaves: Send + Sync {
    /// Syncs with every reachable paired device, waiting up to `within`.
    /// `false` when some couldn't be reached or didn't finish in time.
    async fn refresh(&self, within: Duration) -> bool;
}

type Lookup = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Save versions for the library. Cheap to clone.
#[derive(Clone)]
pub struct SaveVersions {
    inner: Arc<Inner>,
}

struct Inner {
    saves: Saves,
    blobs: BlobStore,
    settings: OnceLock<SettingsService>,
    device: RwLock<Lookup>,
    account: RwLock<Lookup>,
    remote: OnceLock<Arc<dyn RemoteSaves>>,
    locks: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    wait: RwLock<Duration>,
    /// Games whose versions or current save changed here.
    changed: tokio::sync::broadcast::Sender<String>,
}

impl SaveVersions {
    /// Versions of `saves`' releases, their files kept in `blob_dir`.
    pub fn new(saves: Saves, blob_dir: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                saves,
                blobs: BlobStore::new(blob_dir),
                settings: OnceLock::new(),
                device: RwLock::new(Arc::new(|| None)),
                account: RwLock::new(Arc::new(|| None)),
                remote: OnceLock::new(),
                locks: std::sync::Mutex::default(),
                wait: RwLock::new(WAIT),
                changed: tokio::sync::broadcast::channel(64).0,
            }),
        }
    }

    /// Reads `core.saves.keep` and whether Saves sync (`core.sync`).
    pub fn set_settings(&self, settings: SettingsService) {
        let _ = self.inner.settings.set(settings);
    }

    /// This device's sync id, once it has one.
    pub fn set_device(&self, device: impl Fn() -> Option<String> + Send + Sync + 'static) {
        *self.inner.device.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(device);
    }

    /// The account playing now.
    pub fn set_account(&self, account: impl Fn() -> Option<String> + Send + Sync + 'static) {
        *self
            .inner
            .account
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Arc::new(account);
    }

    /// How a launch asks other devices for newer saves.
    pub fn set_remote(&self, remote: Arc<dyn RemoteSaves>) {
        let _ = self.inner.remote.set(remote);
    }

    /// How long a launch waits for other devices (tests shorten it).
    pub fn set_wait(&self, wait: Duration) {
        *self.inner.wait.write().unwrap_or_else(|e| e.into_inner()) = wait;
    }

    /// Game ids, as their versions or the save in place change here
    /// (versions from other devices arrive through sync instead).
    pub fn changes(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.inner.changed.subscribe()
    }

    fn announce(&self, game_id: &str) {
        let _ = self.inner.changed.send(game_id.to_owned());
    }

    /// Makes a version after every play session (`launch.after`).
    pub fn attach(&self, bus: &Bus) {
        bus.intercept::<LaunchAfter>("caskade.saves", 0, Arc::new(AfterSession(self.clone())));
    }

    pub fn store(&self) -> &LibraryStore {
        &self.inner.saves.store
    }

    /// Where the releases' saves are on this device.
    pub fn saves(&self) -> &Saves {
        &self.inner.saves
    }

    pub fn blobs(&self) -> &BlobStore {
        &self.inner.blobs
    }

    fn account(&self) -> String {
        (self.inner.account.read().unwrap_or_else(|e| e.into_inner()))().unwrap_or_default()
    }

    fn device(&self) -> String {
        (self.inner.device.read().unwrap_or_else(|e| e.into_inner()))()
            .unwrap_or_else(|| "local".into())
    }

    fn wait(&self) -> Duration {
        *self.inner.wait.read().unwrap_or_else(|e| e.into_inner())
    }

    async fn keep(&self) -> u32 {
        match self.inner.settings.get() {
            Some(settings) => {
                settings
                    .read_or_default::<crate::settings::SavesSettings>("core.saves")
                    .await
                    .keep
            }
            None => KEEP,
        }
        .max(1)
    }

    /// Whether Saves sync with other devices here.
    pub async fn syncing(&self) -> bool {
        match self.inner.settings.get() {
            Some(settings) => settings
                .read_or_default::<SyncSettings>(SyncSettings::NAMESPACE)
                .await
                .allows(Category::Saves),
            None => true,
        }
    }

    fn lock(&self, game_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.inner
            .locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(game_id.to_owned())
            .or_default()
            .clone()
    }

    async fn game_of(&self, entry_id: &str) -> Result<String> {
        Ok(self
            .store()
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| crate::db::missing("entry", entry_id))?
            .game_id)
    }

    /// The release's save files here, as a file list (hashed, not kept).
    async fn on_disk(
        &self,
        entry_id: &str,
    ) -> Result<(Manifest, Vec<(PathBuf, usize)>, Roots, Vec<String>)> {
        let (state, roots) = self.inner.saves.resolve(entry_id).await?;
        let mut paths: Vec<String> = state
            .locations
            .iter()
            .flat_map(|location| location.files.iter().cloned())
            .collect();
        paths.sort();
        paths.dedup();
        if paths.len() > MAX_FILES {
            return Err(Error::Invalid(crate::tr!(
                "This game's save folders hold too many files to keep versions of."
            )));
        }
        let scan_roots = roots.clone();
        let (manifest, sources) = tokio::task::spawn_blocking(move || {
            let mut files: Vec<(ManifestFile, PathBuf)> = Vec::new();
            for path in paths {
                let path = PathBuf::from(path);
                // Gone or unreadable since it was found: not part of the save.
                let Ok((hash, size)) = hash_file(&path) else {
                    continue;
                };
                let (root, relative) = scan_roots.portable(&path.to_string_lossy());
                let file = ManifestFile {
                    root,
                    path: relative,
                    hash,
                    size,
                };
                files.push((file, path));
            }
            files.sort();
            files.dedup_by(|a, b| a.0.root == b.0.root && a.0.path == b.0.path);
            let sources = files
                .iter()
                .enumerate()
                .map(|(index, (_, path))| (path.clone(), index))
                .collect::<Vec<_>>();
            let manifest = Manifest {
                files: files.into_iter().map(|(file, _)| file).collect(),
            };
            (manifest, sources)
        })
        .await?;
        if manifest.size() > MAX_BYTES {
            return Err(Error::Invalid(crate::tr!(
                "This game's saves are too large to keep versions of."
            )));
        }
        Ok((manifest, sources, roots, state.warnings))
    }

    /// Keeps the release's save files as a new version, unless they're what
    /// this device's head already holds. Returns the version made.
    pub async fn capture(
        &self,
        entry_id: &str,
        session_id: Option<&str>,
        reason: VersionReason,
    ) -> Result<Option<SaveVersion>> {
        let game_id = self.game_of(entry_id).await?;
        let lock = self.lock(&game_id);
        let _held = lock.lock().await;
        self.capture_locked(entry_id, &game_id, session_id, reason)
            .await
    }

    async fn capture_locked(
        &self,
        entry_id: &str,
        game_id: &str,
        session_id: Option<&str>,
        reason: VersionReason,
    ) -> Result<Option<SaveVersion>> {
        let account = self.account();
        let (manifest, sources, _, _) = self.on_disk(entry_id).await?;
        if manifest.files.is_empty() {
            return Ok(None);
        }
        let content = manifest.hash();
        let head = self.store().save_head(game_id, &account).await?;
        if head.as_ref().is_some_and(|(_, disk)| *disk == content) {
            return Ok(None);
        }
        // The same save as a tip (another device's, say): this device has it.
        let tips = self.tips(game_id, &account).await?;
        if let Some(tip) = tips.iter().find(|tip| tip.content_hash == content) {
            self.store()
                .set_save_head(game_id, &account, &tip.id, &content)
                .await?;
            return Ok(None);
        }
        let blobs = self.inner.blobs.clone();
        let kept = manifest.clone();
        tokio::task::spawn_blocking(move || {
            for (source, index) in sources {
                let (hash, _) = blobs.put_file(&source)?;
                // Changed while it was read: the list says what was kept.
                if hash != kept.files[index].hash {
                    return Err(Error::Invalid(crate::tr!(
                        "A save file changed while it was being kept. Try again once the game has closed."
                    )));
                }
            }
            blobs.put_bytes(&kept.bytes())
        })
        .await??;
        let version = SaveVersion {
            id: uuid::Uuid::now_v7().to_string(),
            game_id: game_id.to_owned(),
            account_id: account.clone(),
            device_id: self.device(),
            session_id: session_id.map(str::to_owned),
            created_at_ms: crate::now_ms(),
            content_hash: content.clone(),
            size_bytes: manifest.size(),
            file_count: u32::try_from(manifest.files.len()).unwrap_or(u32::MAX),
            parents: head.map(|(id, _)| vec![id]).unwrap_or_default(),
            reason,
            local: true,
        };
        self.store().insert_save_version(&version).await?;
        self.store()
            .set_save_head(game_id, &account, &version.id, &content)
            .await?;
        self.prune(game_id).await?;
        self.announce(game_id);
        Ok(Some(version))
    }

    /// The account's versions nothing descends from, newest first, one per
    /// content.
    async fn tips(&self, game_id: &str, account: &str) -> Result<Vec<SaveVersion>> {
        let versions = self.store().save_versions(game_id, Some(account)).await?;
        let parents: HashSet<&str> = versions
            .iter()
            .flat_map(|version| version.parents.iter().map(String::as_str))
            .collect();
        let mut contents = HashSet::new();
        Ok(versions
            .iter()
            .filter(|version| !parents.contains(version.id.as_str()))
            .filter(|version| contents.insert(version.content_hash.clone()))
            .cloned()
            .collect())
    }

    /// Whether every file of `version` is here.
    pub fn complete(&self, version: &SaveVersion) -> bool {
        self.manifest(&version.content_hash)
            .is_some_and(|manifest| {
                manifest
                    .files
                    .iter()
                    .all(|file| self.inner.blobs.has(&file.hash))
            })
    }

    fn manifest(&self, hash: &str) -> Option<Manifest> {
        manifest(&self.inner.blobs, hash)
    }

    /// Before a launch: keeps what changed here, then puts a newer save from
    /// another device in place, or says both played. Waits for other
    /// devices for a few seconds at most.
    pub async fn before_play(&self, entry_id: &str) -> Result<BeforePlay> {
        let game_id = self.game_of(entry_id).await?;
        let account = self.account();
        let checked = match (self.inner.remote.get(), self.syncing().await) {
            (Some(remote), true) => remote.refresh(self.wait()).await,
            _ => true,
        };
        let lock = self.lock(&game_id);
        let _held = lock.lock().await;
        let mut warnings = Vec::new();
        if let Err(error) = self
            .capture_locked(entry_id, &game_id, None, VersionReason::Found)
            .await
        {
            warnings.push(error.to_string());
        }
        let tips = self.tips(&game_id, &account).await?;
        let head = self.store().save_head(&game_id, &account).await?;
        let check = match tips.as_slice() {
            [] => SaveCheck::NoSaves,
            [tip] if head.as_ref().is_some_and(|(id, _)| *id == tip.id) => SaveCheck::UpToDate,
            // The same save under a newer version (a choice made elsewhere).
            [tip]
                if head
                    .as_ref()
                    .is_some_and(|(_, disk)| *disk == tip.content_hash) =>
            {
                self.store()
                    .set_save_head(&game_id, &account, &tip.id, &tip.content_hash)
                    .await?;
                SaveCheck::UpToDate
            }
            [tip] => {
                if self.complete(tip) {
                    warnings.extend(self.put_in_place(entry_id, &game_id, &account, tip).await?);
                    SaveCheck::Restored {
                        version: tip.clone(),
                    }
                } else {
                    SaveCheck::NotArrived {
                        version: tip.clone(),
                    }
                }
            }
            _ => SaveCheck::Conflict {
                versions: tips.clone(),
            },
        };
        Ok(BeforePlay {
            check,
            checked,
            warnings,
        })
    }

    /// Writes `version`'s files into the release's save folders here and
    /// makes it this device's head. Returns warnings for files it couldn't
    /// place.
    async fn put_in_place(
        &self,
        entry_id: &str,
        game_id: &str,
        account: &str,
        version: &SaveVersion,
    ) -> Result<Vec<String>> {
        let manifest = self
            .manifest(&version.content_hash)
            .ok_or_else(|| Error::Invalid(crate::tr!("A save file hasn't arrived yet.")))?;
        self.inner.saves.ensure_prefix(entry_id).await?;
        let (_, roots) = self.inner.saves.resolve(entry_id).await?;
        let blobs = self.inner.blobs.clone();
        let warnings = tokio::task::spawn_blocking(move || {
            let mut warnings = Vec::new();
            for file in &manifest.files {
                match roots.place(&file.root, &file.path) {
                    Some(target) => blobs.restore(&file.hash, &target)?,
                    None => warnings.push(crate::tr!(
                        "{path} has no place on this device, so it wasn't restored.",
                        path = format!("{}/{}", file.root, file.path)
                    )),
                }
            }
            Ok::<_, Error>(warnings)
        })
        .await??;
        let (disk, _, _, _) = self.on_disk(entry_id).await?;
        self.store()
            .set_save_head(game_id, account, &version.id, &disk.hash())
            .await?;
        self.announce(game_id);
        Ok(warnings)
    }

    /// Uses `version_id` for the release's saves: what's there now is kept
    /// as a version first, then the chosen files are put in place as a new
    /// version descending from every tip, so the choice settles a conflict
    /// on every device. `reason` is `Chosen` at a conflict, `Restored` for
    /// an older version.
    pub async fn use_version(
        &self,
        entry_id: &str,
        version_id: &str,
        reason: VersionReason,
    ) -> Result<SaveVersion> {
        let game_id = self.game_of(entry_id).await?;
        let account = self.account();
        let lock = self.lock(&game_id);
        let _held = lock.lock().await;
        let chosen = self
            .store()
            .save_version(version_id)
            .await?
            .filter(|version| version.game_id == game_id)
            .ok_or_else(|| store::missing(version_id))?;
        if !self.complete(&chosen) {
            return Err(Error::Invalid(crate::tr!(
                "That save's files haven't arrived on this device yet."
            )));
        }
        self.capture_locked(entry_id, &game_id, None, VersionReason::BeforeRestore)
            .await?;
        let mut parents: Vec<String> = self
            .tips(&game_id, &account)
            .await?
            .into_iter()
            .map(|tip| tip.id)
            .collect();
        if !parents.contains(&chosen.id) {
            parents.push(chosen.id.clone());
        }
        let version = SaveVersion {
            id: uuid::Uuid::now_v7().to_string(),
            game_id: game_id.clone(),
            account_id: account.clone(),
            device_id: self.device(),
            session_id: None,
            created_at_ms: crate::now_ms(),
            content_hash: chosen.content_hash.clone(),
            size_bytes: chosen.size_bytes,
            file_count: chosen.file_count,
            parents,
            reason,
            local: true,
        };
        self.store().insert_save_version(&version).await?;
        self.put_in_place(entry_id, &game_id, &account, &version)
            .await?;
        self.prune(&game_id).await?;
        Ok(version)
    }

    /// Chooses between versions both devices made (a [`SaveCheck::Conflict`]).
    pub async fn choose(&self, entry_id: &str, version_id: &str) -> Result<SaveVersion> {
        self.use_version(entry_id, version_id, VersionReason::Chosen)
            .await
    }

    /// Puts an older version's files in place (#241): what's there now is
    /// kept as a version first, so nothing is lost.
    pub async fn restore(&self, entry_id: &str, version_id: &str) -> Result<SaveVersion> {
        self.use_version(entry_id, version_id, VersionReason::Restored)
            .await
    }

    /// A game's versions, newest first.
    pub async fn versions(&self, game_id: &str) -> Result<Vec<SaveVersion>> {
        self.store().save_versions(game_id, None).await
    }

    /// A game's versions for its Saves panel (#241): every account's, newest
    /// first, with the one this device's files match for the account playing
    /// now, and whether each one's files are here.
    pub async fn history(&self, game_id: &str) -> Result<SaveHistory> {
        let account = self.account();
        let head = self.store().save_head(game_id, &account).await?;
        let tips = self.tips(game_id, &account).await?;
        let conflict = if tips.len() > 1 {
            tips.into_iter().map(|tip| tip.id).collect()
        } else {
            Vec::new()
        };
        let versions = self
            .versions(game_id)
            .await?
            .into_iter()
            .map(|version| VersionState {
                current: head.as_ref().is_some_and(|(id, _)| *id == version.id),
                complete: self.complete(&version),
                version,
            })
            .collect();
        Ok(SaveHistory {
            account_id: account,
            versions,
            conflict,
            syncing: self.syncing().await,
        })
    }

    /// Keeps the newest versions of a game, every tip and every head, and
    /// drops the files nothing kept names.
    async fn prune(&self, game_id: &str) -> Result<()> {
        let keep = self.keep().await as usize;
        let versions = self.store().save_versions(game_id, None).await?;
        if versions.len() <= keep {
            return Ok(());
        }
        let mut protected: HashSet<String> = self
            .store()
            .save_head_versions()
            .await?
            .into_iter()
            .collect();
        let accounts: HashSet<&str> = versions.iter().map(|v| v.account_id.as_str()).collect();
        for account in accounts {
            protected.extend(
                self.tips(game_id, account)
                    .await?
                    .into_iter()
                    .map(|tip| tip.id),
            );
        }
        let drop: Vec<String> = versions
            .iter()
            .skip(keep)
            .filter(|version| !protected.contains(&version.id))
            .map(|version| version.id.clone())
            .collect();
        if drop.is_empty() {
            return Ok(());
        }
        self.store().delete_save_versions(&drop).await?;
        let referenced = self.referenced().await?;
        let blobs = self.inner.blobs.clone();
        tokio::task::spawn_blocking(move || blobs.collect_garbage(&referenced)).await??;
        Ok(())
    }

    /// Every blob a version here names: file lists and their files.
    async fn referenced(&self) -> Result<HashSet<String>> {
        let contents = self.store().save_content_hashes().await?;
        let blobs = self.inner.blobs.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let mut referenced = HashSet::new();
            for content in contents {
                if let Some(manifest) = manifest(&blobs, &content) {
                    referenced.extend(manifest.files.into_iter().map(|file| file.hash));
                }
                referenced.insert(content);
            }
            referenced
        })
        .await?)
    }

    /// Blobs the versions here name and this device lacks: a file list,
    /// then once it's here, its files.
    pub async fn wanted(&self) -> Result<Vec<String>> {
        let contents = self.store().save_content_hashes().await?;
        let blobs = self.inner.blobs.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let mut wanted = Vec::new();
            for content in contents {
                match manifest(&blobs, &content) {
                    Some(manifest) => wanted.extend(
                        manifest
                            .files
                            .into_iter()
                            .map(|file| file.hash)
                            .filter(|hash| !blobs.has(hash)),
                    ),
                    None => wanted.push(content),
                }
            }
            wanted.sort();
            wanted.dedup();
            wanted
        })
        .await?)
    }
}

/// The file list stored as `hash`, if it is here and intact.
fn manifest(blobs: &BlobStore, hash: &str) -> Option<Manifest> {
    let bytes = blobs.get_bytes(hash).ok()?;
    (hash_bytes(&bytes) == hash)
        .then(|| serde_json::from_slice(&bytes).ok())
        .flatten()
}

/// Keeps a version after every play session.
struct AfterSession(SaveVersions);

#[async_trait]
impl Interceptor<LaunchAfter> for AfterSession {
    async fn intercept(
        &self,
        event: LaunchEvent,
    ) -> std::result::Result<Intercept<LaunchEvent>, String> {
        if matches!(
            event.status,
            SessionStatus::Exited | SessionStatus::Crashed | SessionStatus::Interrupted
        ) {
            // In the background: hooks are time-boxed, and hashing a large
            // save mustn't hold up the session's end.
            let versions = self.0.clone();
            let (entry, session) = (event.entry_id.clone(), event.session_id.clone());
            tokio::spawn(async move {
                if let Err(error) = versions
                    .capture(&entry, Some(&session), VersionReason::Session)
                    .await
                {
                    tracing::warn!(%error, "the game's saves weren't kept as a version");
                }
            });
        }
        Ok(Intercept::Continue(event))
    }
}
