use super::*;
use crate::{
    AppPaths,
    db::LibraryStore,
    launch::LaunchService,
    model::{GameEntry, InstallLocation, StorageRoot, new_id},
    settings::{SectionDefinition, SettingsService},
};
use caskade_installer::manifest::{
    INSTALL_MANIFEST_PATH, InstallManifest, RelocationVerdict, RootManifest,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use tokio::sync::Mutex;

/// Capabilities measured by the last full probe, keyed by root id. The path
/// is part of the key: a root seen at another path is probed again.
type ProbeCache = HashMap<String, (PathBuf, RootCapabilities)>;
/// What the storage list needs from a managed game's `install.json`, keyed by
/// the manifest path and valid while the file's stamp is unchanged.
type ManifestCache = HashMap<PathBuf, ManifestFacts>;
#[derive(Clone)]
struct ManifestFacts {
    stamp: FileStamp,
    /// `None` when the manifest did not parse.
    parsed: Option<ParsedManifest>,
}
#[derive(Clone, Copy)]
struct ParsedManifest {
    size: u64,
    /// Pinned or delegated to a native package manager.
    native: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
    inode: u64,
}

#[derive(Clone)]
pub struct StorageService {
    pub(super) store: LibraryStore,
    pub(super) settings: SettingsService,
    pub(super) launch: LaunchService,
    pub(super) paths: AppPaths,
    pub(super) probe: Arc<dyn RootProbe>,
    pub(super) mutation: Arc<Mutex<()>>,
    probed: Arc<std::sync::Mutex<ProbeCache>>,
    manifests: Arc<std::sync::Mutex<ManifestCache>>,
}
impl StorageService {
    pub fn new(
        store: LibraryStore,
        settings: SettingsService,
        launch: LaunchService,
    ) -> Result<Self> {
        if !settings
            .sections()?
            .iter()
            .any(|s| s.namespace == "storage.roots")
        {
            settings.register(SectionDefinition::new("storage.roots",1,json!({"type":"object","properties":{"templates":{"type":"object","additionalProperties":{"type":"string","maxLength":128}}},"required":["templates"],"additionalProperties":false}),json!({"templates":{}}))?)?;
        }
        Ok(Self {
            store,
            settings,
            launch,
            paths: AppPaths::standard()?,
            probe: Arc::new(FileSystemProbe),
            mutation: Arc::default(),
            probed: Arc::default(),
            manifests: Arc::default(),
        })
    }
    pub fn with_paths(mut self, paths: AppPaths) -> Self {
        self.paths = paths;
        self
    }
    pub fn with_probe(mut self, probe: Arc<dyn RootProbe>) -> Self {
        self.probe = probe;
        crate::lock(&self.probed).clear();
        self
    }
    /// Poisons both cache locks, as a panic while holding them would.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn poison_caches(&self) {
        let (probed, manifests) = (self.probed.clone(), self.manifests.clone());
        let _ = std::thread::spawn(move || {
            let _probed = probed.lock();
            let _manifests = manifests.lock();
            panic!("poisons the storage caches");
        })
        .join();
        assert!(self.probed.is_poisoned() && self.manifests.is_poisoned());
    }
    pub async fn restore_roots(&self) -> Result<usize> {
        let _guard = self.mutation.lock().await;
        let config = self.paths.config.clone();
        let mut roots = tokio::task::spawn_blocking(move || super::catalogue::read(&config))
            .await
            .map_err(|e| StorageError::Worker(e.to_string()))??;
        if roots.is_empty() {
            return Ok(0);
        }
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        if roots.iter().any(|r| {
            !r.path.is_absolute() || !ids.insert(r.id.clone()) || !paths.insert(r.path.clone())
        }) {
            return Err(StorageError::Invalid(crate::tr!(
                "duplicate or relative root catalogue record"
            )));
        }
        let default = roots.iter().position(|r| r.is_default).unwrap_or(0);
        for (i, root) in roots.iter_mut().enumerate() {
            root.is_default = i == default;
            root.validate()?;
            self.store.upsert_root(root).await?;
        }
        Ok(roots.len())
    }
    async fn persist(&self, roots: Vec<StorageRoot>) -> Result<()> {
        let config = self.paths.config.clone();
        tokio::task::spawn_blocking(move || super::catalogue::write(&config, &roots))
            .await
            .map_err(|e| StorageError::Worker(e.to_string()))?
    }
    /// Register the physical identity, never infer identity from its host path.
    /// #54 imports manifests after this succeeds; core has no source-layer dependency.
    pub async fn register_root(&self, path: PathBuf, label: Option<String>) -> Result<RootSummary> {
        let _guard = self.mutation.lock().await;
        validate_label(label.as_deref())?;
        if !path.is_absolute() {
            return Err(StorageError::Invalid(crate::tr!(
                "choose an absolute storage folder"
            )));
        }
        let path = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&path)?;
            std::fs::canonicalize(path)
        })
        .await
        .map_err(|e| StorageError::Worker(e.to_string()))??;
        let roots = self.store.roots().await?;
        for root in &roots {
            if root.path != path && (path.starts_with(&root.path) || root.path.starts_with(&path)) {
                return Err(StorageError::Invalid(crate::tr!(
                    "storage locations must not overlap"
                )));
            }
        }
        let candidate = path.clone();
        let probe = self.probe.clone();
        let (identity, caps) = tokio::task::spawn_blocking(move || -> Result<_> {
            let caps = probe.probe(&candidate)?;
            if !caps.writable {
                return Err(StorageError::ReadOnly);
            }
            let manifest = match RootManifest::read(&candidate) {
                Ok(manifest) => manifest,
                Err(caskade_installer::manifest::ManifestError::Io(e))
                    if e.kind() == std::io::ErrorKind::NotFound =>
                {
                    let manifest = RootManifest::new(new_id())?;
                    manifest.write(&candidate)?;
                    manifest
                }
                Err(e) => return Err(e.into()),
            };
            Ok((manifest, caps))
        })
        .await
        .map_err(|e| StorageError::Worker(e.to_string()))??;
        if let Some(old) = roots.iter().find(|r| r.id == identity.root_id())
            && old.path != path
            && RootManifest::read(&old.path).is_ok_and(|m| m.root_id() == old.id)
        {
            return Err(StorageError::Invalid(crate::tr!(
                "this root identity is still online at another location; choose the authoritative copy"
            )));
        }
        let previous = roots.iter().find(|r| r.id == identity.root_id());
        let root = StorageRoot {
            id: identity.root_id().into(),
            path,
            is_default: previous.is_some_and(|r| r.is_default) || roots.is_empty(),
            label: label.or_else(|| previous.and_then(|r| r.label.clone())),
        };
        let mut next = roots
            .into_iter()
            .filter(|r| r.id != root.id)
            .collect::<Vec<_>>();
        next.push(root.clone());
        self.persist(next).await?;
        self.store.upsert_root(&root).await?;
        // Registration just measured the volume: the summary reuses it.
        crate::lock(&self.probed).insert(root.id.clone(), (root.path.clone(), caps));
        self.summary(root).await
    }
    /// Registers `AppPaths::games` when no storage location exists yet, so
    /// installs work on first start. `None` when one already exists.
    pub async fn ensure_default_root(&self) -> Result<Option<RootSummary>> {
        if !self.store.roots().await?.is_empty() {
            return Ok(None);
        }
        let path = self.paths.games.clone();
        self.register_root(path, None).await.map(Some)
    }
    pub async fn list_roots(&self) -> Result<Vec<RootSummary>> {
        let mut summaries = vec![];
        for root in self.store.roots().await? {
            summaries.push(self.summary(root).await?)
        }
        Ok(summaries)
    }
    pub async fn resolve(&self, root_id: Option<&str>) -> Result<StorageRoot> {
        let roots = self.store.roots().await?;
        let root = roots
            .into_iter()
            .find(|r| root_id.map_or(r.is_default, |id| r.id == id))
            .ok_or_else(|| {
                StorageError::Invalid(crate::tr!(
                    "add a storage location in Settings before installing"
                ))
            })?;
        let (status, _) = self.inspect(&root).await?;
        match status {
            RootStatus::Online => Ok(root),
            RootStatus::ReadOnly => Err(StorageError::ReadOnly),
            _ => Err(StorageError::Offline),
        }
    }
    pub async fn set_default(&self, id: &str) -> Result<RootSummary> {
        let _guard = self.mutation.lock().await;
        let mut root = self.resolve(Some(id)).await?;
        root.is_default = true;
        let mut roots = self.store.roots().await?;
        for item in &mut roots {
            item.is_default = item.id == id;
        }
        self.persist(roots).await?;
        self.store.upsert_root(&root).await?;
        self.summary(root).await
    }
    pub async fn edit_root(
        &self,
        id: &str,
        label: Option<String>,
        template: String,
    ) -> Result<RootSummary> {
        let _guard = self.mutation.lock().await;
        validate_label(label.as_deref())?;
        validate_template(&template)?;
        let mut root = self
            .store
            .get_root(id)
            .await?
            .ok_or_else(|| StorageError::Invalid(crate::tr!("unknown storage location")))?;
        root.label = label;
        self.forget_probe(id);
        let latest = self.settings.get("storage.roots").await?;
        let mut value = latest.value;
        value["templates"][id] = json!(template);
        self.settings
            .set("storage.roots", value, Some(latest.revision))
            .await?;
        let mut roots = self.store.roots().await?;
        for item in &mut roots {
            if item.id == id {
                *item = root.clone();
            }
        }
        self.persist(roots).await?;
        self.store.upsert_root(&root).await?;
        self.summary(root).await
    }
    /// Explicit keep-files removal. The location's copies leave the library
    /// with their play sessions, and a game with no other copy goes too,
    /// with its favourite, notes and collections. Nothing on disk changes:
    /// adding the folder again adopts its games, as new library games.
    /// Native/delegated installs are not deleted.
    pub async fn remove_keep_files(&self, id: &str) -> Result<()> {
        self.remove_root(id, false).await
    }
    /// Removes a location its games were moved off (#64). One that gained a
    /// game meanwhile stays.
    pub async fn remove_emptied(&self, id: &str) -> Result<()> {
        self.remove_root(id, true).await
    }
    async fn remove_root(&self, id: &str, only_if_empty: bool) -> Result<()> {
        let _guard = self.mutation.lock().await;
        let root = self
            .store
            .get_root(id)
            .await?
            .ok_or_else(|| StorageError::Invalid(crate::tr!("unknown storage location")))?;
        if root.is_default {
            return Err(StorageError::DefaultRoot);
        }
        let entries = self.store.entries_for_root(id).await?;
        if only_if_empty && !entries.is_empty() {
            return Err(StorageError::Invalid(crate::tr!(
                "it has games that aren't moved yet"
            )));
        }
        let mut leases = vec![];
        let mut games = BTreeSet::new();
        for entry in &entries {
            if games.insert(entry.game_id.clone()) {
                leases.push(
                    self.launch
                        .try_game_lease(&entry.game_id)
                        .map_err(|_| StorageError::Running)?,
                );
            }
        }
        if self
            .store
            .in_flight_sessions()
            .await?
            .iter()
            .any(|s| games.contains(&s.game_id))
        {
            return Err(StorageError::Running);
        }
        for entry in entries {
            self.store.delete_entry(&entry.id).await?;
        }
        for id in games {
            if self.store.entries_for_game(&id).await?.is_empty() {
                self.store.delete_game(&id).await?;
            }
        }
        let roots = self
            .store
            .roots()
            .await?
            .into_iter()
            .filter(|r| r.id != id)
            .collect();
        self.persist(roots).await?;
        self.store.delete_root(id).await?;
        self.forget_probe(id);
        drop(leases);
        Ok(())
    }
    fn forget_probe(&self, id: &str) {
        crate::lock(&self.probed).remove(id);
    }
    /// The root's status and capabilities. The identity file is read every
    /// time; the volume is probed (files written, a process started) only
    /// when nothing is cached for this root id and path, after the root was
    /// offline or mismatched, or when the cheap refresh sees the mount change.
    async fn inspect(&self, root: &StorageRoot) -> Result<(RootStatus, RootCapabilities)> {
        let probe = self.probe.clone();
        let physical = root.clone();
        let cached = crate::lock(&self.probed)
            .get(&root.id)
            .filter(|(path, _)| *path == root.path)
            .map(|(_, caps)| caps.clone());
        let (status, caps, keep) = tokio::task::spawn_blocking(move || {
            let status = |caps: &RootCapabilities| {
                if caps.writable {
                    RootStatus::Online
                } else {
                    RootStatus::ReadOnly
                }
            };
            match RootManifest::read(&physical.path) {
                Ok(m) if m.root_id() == physical.id => {
                    if let Some(caps) = cached.and_then(|c| probe.refresh(&physical.path, &c)) {
                        return (status(&caps), caps, true);
                    }
                    match probe.probe(&physical.path) {
                        Ok(caps) => (status(&caps), caps, true),
                        Err(_) => (RootStatus::ReadOnly, RootCapabilities::default(), false),
                    }
                }
                Ok(_) => (
                    RootStatus::IdentityMismatch,
                    RootCapabilities::default(),
                    false,
                ),
                Err(_) => (RootStatus::Offline, RootCapabilities::default(), false),
            }
        })
        .await
        .map_err(|e| StorageError::Worker(e.to_string()))?;
        let mut probed = crate::lock(&self.probed);
        if keep {
            probed.insert(root.id.clone(), (root.path.clone(), caps.clone()));
        } else {
            probed.remove(&root.id);
        }
        Ok((status, caps))
    }
    pub async fn capabilities(&self, id: &str) -> Result<RootCapabilities> {
        let root = self
            .store
            .get_root(id)
            .await?
            .ok_or_else(|| StorageError::Invalid(crate::tr!("unknown storage root")))?;
        let (status, caps) = self.inspect(&root).await?;
        if matches!(status, RootStatus::Offline | RootStatus::IdentityMismatch) {
            return Err(StorageError::Offline);
        }
        Ok(caps)
    }
    async fn template(&self, id: &str) -> Result<String> {
        let templates = self.settings.get("storage.roots").await?;
        Ok(templates.value["templates"][id]
            .as_str()
            .unwrap_or("{title}")
            .to_owned())
    }
    pub(super) async fn summary(&self, root: StorageRoot) -> Result<RootSummary> {
        let (status, caps) = self.inspect(&root).await?;
        let folder_template = self.template(&root.id).await?;
        let entries = self.store.entries_with_titles_for_root(&root.id).await?;
        let manifests = self.manifests.clone();
        let root_path = root.path.clone();
        // Directory checks and manifest reads can block on a slow or hung
        // volume: keep them off the async workers.
        let (games, games_bytes) = tokio::task::spawn_blocking(move || {
            summarize_games(&root_path, status, entries, &manifests)
        })
        .await
        .map_err(|e| StorageError::Worker(e.to_string()))?;
        let other_bytes = caps
            .total_bytes
            .zip(caps.free_bytes)
            .map(|(total, free)| total.saturating_sub(free).saturating_sub(games_bytes));
        Ok(RootSummary {
            root,
            status,
            capabilities: caps,
            folder_template,
            games,
            games_bytes,
            other_bytes,
        })
    }
    pub async fn folder_template(&self, id: &str) -> Result<String> {
        if self.store.get_root(id).await?.is_none() {
            return Err(StorageError::Invalid(crate::tr!("unknown storage root")));
        }
        let template = self.template(id).await?;
        validate_template(&template)?;
        Ok(template)
    }
}
/// Builds the game list of one root. Install manifests are read only while
/// the root is reachable, and parsed again only when the file changed.
fn summarize_games(
    root: &Path,
    status: RootStatus,
    entries: Vec<(GameEntry, Option<String>)>,
    manifests: &std::sync::Mutex<ManifestCache>,
) -> (Vec<StorageGame>, u64) {
    let reachable = matches!(status, RootStatus::Online | RootStatus::ReadOnly);
    let mut games = vec![];
    let mut sizes = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (entry, title) in entries {
        let Some(InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
            continue;
        };
        let game_dir = root.join(rel_dir);
        let manifest_path = game_dir.join(INSTALL_MANIFEST_PATH);
        let manifest = if reachable {
            seen.insert(manifest_path.clone());
            manifest_facts(&game_dir, &manifest_path, manifests)
        } else {
            // An offline root is never touched; a size seen earlier still counts.
            crate::lock(manifests)
                .get(&manifest_path)
                .and_then(|facts| facts.parsed)
        };
        let size = entry.size.or(manifest.map(|m| m.size)).unwrap_or(0);
        sizes.entry(rel_dir.clone()).or_insert(size);
        let blocked = if !matches!(status, RootStatus::Online) {
            Some(crate::tr!("Storage location is unavailable."))
        } else {
            match manifest {
                None => Some(crate::tr!(
                    "The managed install manifest is missing or invalid."
                )),
                Some(m) if m.native => Some(crate::tr!(
                    "This installation is managed by its native package manager."
                )),
                Some(_) => None,
            }
        };
        games.push(StorageGame {
            entry_id: entry.id,
            game_id: entry.game_id,
            title: title.unwrap_or_else(|| crate::trp!("untitled game", "Game")),
            relative_dir: rel_dir.clone(),
            size_bytes: size,
            available: reachable && game_dir.is_dir(),
            move_blocked: blocked,
        });
    }
    if reachable {
        // Drop facts of games that left this root.
        crate::lock(manifests).retain(|path, _| !path.starts_with(root) || seen.contains(path));
    }
    games.sort_by_key(|game| std::cmp::Reverse(game.size_bytes));
    (games, sizes.values().copied().sum())
}
fn manifest_facts(
    game_dir: &Path,
    path: &Path,
    manifests: &std::sync::Mutex<ManifestCache>,
) -> Option<ParsedManifest> {
    let stamp = std::fs::symlink_metadata(game_dir.join(".caskade"))
        .ok()
        .filter(|meta| meta.is_dir())
        .and_then(|_| std::fs::symlink_metadata(path).ok())
        .filter(|meta| meta.is_file())
        .and_then(|meta| {
            Some(FileStamp {
                modified: meta.modified().ok()?,
                len: meta.len(),
                #[cfg(unix)]
                inode: std::os::unix::fs::MetadataExt::ino(&meta),
                #[cfg(not(unix))]
                inode: 0,
            })
        });
    let Some(stamp) = stamp else {
        crate::lock(manifests).remove(path);
        // Missing, or no usable timestamp: read it uncached.
        return read_manifest(game_dir);
    };
    if let Some(facts) = crate::lock(manifests).get(path)
        && facts.stamp == stamp
    {
        return facts.parsed;
    }
    let parsed = read_manifest(game_dir);
    crate::lock(manifests).insert(path.to_owned(), ManifestFacts { stamp, parsed });
    parsed
}
fn read_manifest(game_dir: &Path) -> Option<ParsedManifest> {
    let manifest = InstallManifest::read(game_dir).ok()?;
    let record = manifest.record();
    Some(ParsedManifest {
        size: record.files.iter().map(|f| f.size).sum(),
        native: record.relocation.verdict == RelocationVerdict::Pinned
            || record.plan.candidate.strategy == caskade_installer::Strategy::Delegate,
    })
}
fn validate_label(label: Option<&str>) -> Result<()> {
    if label.is_some_and(|s| s.len() > 128 || s.chars().any(char::is_control)) {
        return Err(StorageError::Invalid(crate::tr!(
            "location labels must be at most128 printable bytes"
        )));
    }
    Ok(())
}
fn validate_template(template: &str) -> Result<()> {
    let rest = template.replace("{title}", "");
    if template.len() > 128
        || !template.contains("{title}")
        || rest.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '{' | '}'
                )
        })
        || template.starts_with('.')
    {
        return Err(StorageError::Invalid(crate::tr!(
            "use a single folder name containing {title}"
        )));
    }
    Ok(())
}
