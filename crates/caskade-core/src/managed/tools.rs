//! Helper programs Caskade installs and keeps: the
//! streaming host and client. Installed under `<data>/<definition dir>`,
//! recorded in `managed_tools`, checked for updates on `core.updates`
//! like managed emulators, and never updated or removed by themselves.

use super::{
    Hosts, ToolCatalog, ToolDefinition, flatpak, install_release, remove_release, resolve,
};
use crate::{
    AppPaths, Error, Result,
    bus::{Bus, Event},
    db::LibraryStore,
    emulation::{EmulatorInstall, InstallSource},
    settings::{SettingsService, UpdateSettings},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

/// How often [`ManagedTools::watch`] looks for tools due a check.
const TICK: Duration = Duration::from_secs(15 * 60);
/// How long after a failed check the watch leaves the tool alone.
const RETRY: Duration = Duration::from_secs(30 * 60);

/// A tool Caskade installed, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedTool {
    pub tool_id: String,
    /// `github`, `dolphin`, `url` or `flatpak`.
    pub source: String,
    /// Where it was unpacked; `None` for Flatpaks.
    pub dir: Option<String>,
    /// What runs it.
    pub install: Option<EmulatorInstall>,
    /// The version installed; `None` when the source doesn't tell.
    pub installed: Option<String>,
    /// The file it was installed from.
    pub file: Option<String>,
    pub latest: Option<String>,
    pub latest_url: Option<String>,
    /// 0 before the first check.
    pub checked_at_ms: i64,
    pub skipped: Option<String>,
    /// Why the last check failed, as stored ([`crate::i18n::Text::store`]).
    pub error: Option<String>,
}

impl ManagedTool {
    /// A newer version than the installed one was found and not skipped.
    pub fn update_available(&self) -> bool {
        super::update_available(
            self.installed.as_deref(),
            self.latest.as_deref(),
            self.skipped.as_deref(),
        )
    }
}

/// A tool as people read it: Caskade's words for what it does, never the
/// program's name.
pub fn tool_label(tool_id: &str) -> String {
    match tool_id {
        "sunshine" => crate::tr!("Game streaming from this device"),
        "moonlight" => crate::tr!("Game streaming to this device"),
        other => other.to_owned(),
    }
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    catalog: ToolCatalog,
    host: super::Host,
    /// `<data>`: tools go in their definitions' folders under it.
    data: PathBuf,
    /// `<cache>/tools`: downloads.
    cache: PathBuf,
    hosts: Hosts,
    flatpak: PathBuf,
    settings: Mutex<Option<SettingsService>>,
    /// Tools whose last update check failed, and when to try again.
    retry_at: Mutex<HashMap<String, Instant>>,
    /// Tools being installed, updated or removed.
    busy: Mutex<BTreeSet<String>>,
    /// Devices nearby that may have a download already (#277).
    #[cfg(feature = "net")]
    near: Mutex<Option<crate::install::nearby::PeerDownloads>>,
}

/// Installs, finds, checks, updates and removes helper tools; cheap to
/// clone.
#[derive(Clone)]
pub struct ManagedTools {
    inner: Arc<Inner>,
}

/// Marks a tool busy until dropped.
struct Busy<'a> {
    tools: &'a ManagedTools,
    id: String,
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        crate::lock(&self.tools.inner.busy).remove(&self.id);
    }
}

impl ManagedTools {
    pub fn new(store: LibraryStore, bus: Bus, catalog: ToolCatalog, paths: &AppPaths) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                catalog,
                host: super::Host::current(),
                data: paths.data.clone(),
                cache: paths.cache.join("tools"),
                hosts: Hosts::default(),
                flatpak: "flatpak".into(),
                settings: Mutex::default(),
                retry_at: Mutex::default(),
                busy: Mutex::default(),
                #[cfg(feature = "net")]
                near: Mutex::default(),
            }),
        }
    }

    /// Another machine, release hosts and `flatpak` program (tests).
    pub fn with_host(self, host: super::Host, hosts: Hosts, flatpak: impl Into<PathBuf>) -> Self {
        let inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("with_host before the service is shared"));
        Self {
            inner: Arc::new(Inner {
                host,
                hosts,
                flatpak: flatpak.into(),
                ..inner
            }),
        }
    }

    /// Update checks follow `core.updates` (on/off, interval).
    pub fn set_settings(&self, settings: SettingsService) {
        *crate::lock(&self.inner.settings) = Some(settings);
    }

    /// Downloads are asked of devices nearby first, and kept for them
    /// (#277).
    #[cfg(feature = "net")]
    pub fn set_nearby(&self, near: crate::install::nearby::PeerDownloads) {
        *crate::lock(&self.inner.near) = Some(near);
    }

    pub fn catalog(&self) -> &ToolCatalog {
        &self.inner.catalog
    }

    fn definition(&self, id: &str) -> Result<&ToolDefinition> {
        self.inner
            .catalog
            .tool(id)
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown tool {tool}", tool = id)))
    }

    /// Where `id` installs from on this machine.
    pub fn source(&self, id: &str) -> Option<&InstallSource> {
        self.inner.catalog.tool(id)?.source(&self.inner.host)
    }

    /// Where `id` goes when downloaded.
    pub fn dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.inner.data.join(&self.definition(id)?.dir))
    }

    pub async fn managed(&self, id: &str) -> Result<Option<ManagedTool>> {
        self.inner.store.managed_tool(id).await
    }

    /// The managed install of `id` while it is still there.
    pub async fn find(&self, id: &str) -> Result<Option<EmulatorInstall>> {
        let Some(install) = self.managed(id).await?.and_then(|managed| managed.install) else {
            return Ok(None);
        };
        let there = match &install {
            EmulatorInstall::Folder { dir, executable } => dir.join(executable).is_file(),
            EmulatorInstall::AppImage { file } => file.is_file(),
            EmulatorInstall::AppBundle { bundle } => bundle.is_dir(),
            EmulatorInstall::Flatpak { app_id } => {
                flatpak::installed(&self.inner.flatpak, app_id).await
            }
            EmulatorInstall::Path { .. } => false,
        };
        Ok(there.then_some(install))
    }

    fn take(&self, id: &str) -> Result<Busy<'_>> {
        if !crate::lock(&self.inner.busy).insert(id.to_owned()) {
            return Err(Error::Invalid(crate::tr!(
                "{tool} is already being changed",
                tool = tool_label(id)
            )));
        }
        Ok(Busy {
            tools: self,
            id: id.to_owned(),
        })
    }

    /// Installs `id` from its source on this machine, or reinstalls it
    /// over the copy there with the user's files carried across.
    /// `progress` hears the bytes downloaded and the total when known.
    pub async fn install(
        &self,
        id: &str,
        cancel: &CancellationToken,
        progress: impl FnMut(u64, Option<u64>) + Send,
    ) -> Result<ManagedTool> {
        let definition = self.definition(id)?;
        let source = self.source(id).cloned().ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "{tool} can't be installed on this system",
                tool = tool_label(id)
            ))
        })?;
        let _busy = self.take(id)?;
        let existing = self.managed(id).await?;
        let job = crate::model::new_id();
        let result = self
            .put_in_place(
                definition,
                &source,
                existing.as_ref(),
                &job,
                cancel,
                progress,
            )
            .await;
        let _ = tokio::fs::remove_dir_all(self.inner.cache.join(&job)).await;
        let (install, version, file, dir) = result?;
        let mut managed = existing.unwrap_or(ManagedTool {
            tool_id: id.to_owned(),
            source: String::new(),
            dir: None,
            install: None,
            installed: None,
            file: None,
            latest: None,
            latest_url: None,
            checked_at_ms: 0,
            skipped: None,
            error: None,
        });
        managed.source = source.kind().into();
        managed.dir = dir;
        managed.install = Some(install);
        managed.installed.clone_from(&version);
        managed.file = file;
        managed.latest = version;
        managed.latest_url = None;
        managed.checked_at_ms = crate::now_ms();
        managed.error = None;
        self.inner.store.upsert_managed_tool(&managed).await?;
        crate::lock(&self.inner.retry_at).remove(id);
        self.changed();
        tracing::info!(tool = id, version = ?managed.installed, "tool installed");
        Ok(managed)
    }

    async fn put_in_place(
        &self,
        definition: &ToolDefinition,
        source: &InstallSource,
        existing: Option<&ManagedTool>,
        job: &str,
        cancel: &CancellationToken,
        progress: impl FnMut(u64, Option<u64>) + Send,
    ) -> Result<(
        EmulatorInstall,
        Option<String>,
        Option<String>,
        Option<String>,
    )> {
        if let InstallSource::Flatpak { app } = source {
            flatpak::install(&self.inner.flatpak, app, cancel).await?;
            let install = EmulatorInstall::Flatpak {
                app_id: app.clone(),
            };
            return Ok((install, None, None, None));
        }
        let key = format!(
            "tool:{}:{}:{}",
            definition.id,
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let resolved = match resolve(&self.inner.hosts, source).await {
            Ok(resolved) => resolved,
            // Offline: the newest release a device nearby has (#277).
            Err(error) => self.release_nearby(&key).await.ok_or(error)?,
        };
        let download = self.inner.cache.join(job).join(&resolved.file);
        self.fetch(&resolved.url, &download, cancel, progress)
            .await?;
        self.keep_release(&key, &resolved).await;
        let target = self.inner.data.join(&definition.dir);
        let work = self.inner.data.join(".caskade-tools");
        let missing = crate::tr!(
            "the download for {tool} doesn't contain it",
            tool = tool_label(&definition.id)
        );
        let install = install_release(super::Release {
            file: download,
            staging: work.join("staging").join(job),
            aside: work.join("old").join(job),
            target: target.clone(),
            rules: definition
                .detect
                .on(self.inner.host.os)
                .cloned()
                .unwrap_or_default(),
            os: self.inner.host.os,
            old_version: existing
                .and_then(|managed| managed.installed.clone())
                .unwrap_or_else(|| "previous".into()),
            name: definition.name.clone(),
            missing,
            extract_appimage: definition.extract,
            cancel: cancel.clone(),
        })
        .await;
        let _ = tokio::fs::remove_dir(work.join("staging")).await;
        let _ = tokio::fs::remove_dir(work.join("old")).await;
        let _ = tokio::fs::remove_dir(&work).await;
        Ok((
            install?,
            resolved.version,
            Some(resolved.file),
            Some(target.to_string_lossy().into_owned()),
        ))
    }

    /// Installs the newest version over the installed one.
    pub async fn update(
        &self,
        id: &str,
        cancel: &CancellationToken,
        progress: impl FnMut(u64, Option<u64>) + Send,
    ) -> Result<ManagedTool> {
        self.managed_or_err(id).await?;
        self.install(id, cancel, progress).await
    }

    /// Stops offering `version` (the newest found, when `None`); a newer
    /// one is offered again.
    pub async fn skip(&self, id: &str, version: Option<String>) -> Result<ManagedTool> {
        let mut managed = self.managed_or_err(id).await?;
        managed.skipped = version.or_else(|| managed.latest.clone());
        self.inner.store.upsert_managed_tool(&managed).await?;
        self.changed();
        Ok(managed)
    }

    /// Removes the program: a Flatpak through Flatpak, a download's shipped
    /// files (its settings and state in the folder stay). The record goes.
    pub async fn remove(&self, id: &str) -> Result<()> {
        let managed = self.managed_or_err(id).await?;
        let _busy = self.take(id)?;
        match (&managed.install, &managed.dir) {
            (Some(EmulatorInstall::Flatpak { app_id }), _) => {
                flatpak::uninstall(&self.inner.flatpak, app_id).await?;
            }
            (_, Some(dir)) => {
                let dir = PathBuf::from(dir);
                // Only the tool's own folder, whatever the record says.
                if dir != self.dir(id)? {
                    return Err(Error::Invalid(crate::tr!(
                        "{tool} isn't where Caskade put it",
                        tool = tool_label(id)
                    )));
                }
                if dir.exists() {
                    let removed = remove_release(&dir).await?;
                    tracing::info!(tool = id, removed, "tool removed");
                }
            }
            _ => {}
        }
        self.inner.store.delete_managed_tool(id).await?;
        crate::lock(&self.inner.retry_at).remove(id);
        self.changed();
        Ok(())
    }

    /// Whether every file `id`'s release shipped is still in its folder.
    /// A Flatpak, or a tool not installed, counts as whole.
    pub async fn intact(&self, id: &str) -> Result<bool> {
        let Some(managed) = self.managed(id).await? else {
            return Ok(true);
        };
        let Some(dir) = managed.dir.map(PathBuf::from) else {
            return Ok(true);
        };
        let missing = tokio::task::spawn_blocking(move || super::unpack::missing_shipped(&dir))
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?;
        if !missing.is_empty() {
            tracing::warn!(tool = id, missing = missing.len(), "tool files missing");
        }
        Ok(missing.is_empty())
    }

    /// Checks `id`'s source for a newer version now. A failure is kept in
    /// `error`; the version found and the time of the last check that
    /// worked stay.
    pub async fn check(&self, id: &str) -> Result<ManagedTool> {
        Ok(self.check_once(id).await?.0)
    }

    async fn check_once(&self, id: &str) -> Result<(ManagedTool, Option<Error>)> {
        let mut managed = self.managed_or_err(id).await?;
        let Some(source) = self.source(id).filter(|source| source.tracked()).cloned() else {
            return Ok((managed, None));
        };
        let mut failure = None;
        match resolve(&self.inner.hosts, &source).await {
            Ok(resolved) => {
                crate::lock(&self.inner.retry_at).remove(id);
                managed.latest = resolved.version;
                managed.latest_url = Some(resolved.url.to_string());
                managed.error = None;
                managed.checked_at_ms = crate::now_ms();
            }
            Err(error) => {
                crate::lock(&self.inner.retry_at).insert(id.to_owned(), Instant::now() + RETRY);
                managed.error = Some(crate::i18n::Text::of_error(&error).store());
                failure = Some(error);
            }
        }
        self.inner.store.upsert_managed_tool(&managed).await?;
        self.changed();
        Ok((managed, failure))
    }

    fn failed_recently(&self, id: &str) -> bool {
        let now = Instant::now();
        let mut retry_at = crate::lock(&self.inner.retry_at);
        retry_at.retain(|_, at| *at > now);
        retry_at.contains_key(id)
    }

    /// Checks the managed tools whose last check is older than the update
    /// interval, unless update checks are off, except those that failed in
    /// the last half hour. Stops when a source can't be reached. Returns
    /// how many.
    pub async fn check_due(&self) -> Result<usize> {
        let settings = self.update_settings().await;
        if !settings.check {
            return Ok(0);
        }
        let interval = i64::from(settings.interval_hours.max(1)) * 60 * 60 * 1000;
        let now = crate::now_ms();
        let mut checked = 0;
        for managed in self.inner.store.managed_tools().await? {
            if now - managed.checked_at_ms < interval || self.failed_recently(&managed.tool_id) {
                continue;
            }
            let (_, failure) = self.check_once(&managed.tool_id).await?;
            checked += 1;
            if failure.as_ref().is_some_and(crate::error::unreachable) {
                break;
            }
        }
        Ok(checked)
    }

    /// Checks for updates now and then, in the background.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(TICK);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                timer.tick().await;
                match this.check_due().await {
                    Ok(0) => {}
                    Ok(checked) => tracing::info!(checked, "checked tools for updates"),
                    Err(error) => tracing::warn!(%error, "tool update checks failed"),
                }
            }
        })
    }

    async fn managed_or_err(&self, id: &str) -> Result<ManagedTool> {
        self.managed(id).await?.ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "{tool} isn't installed by Caskade",
                tool = tool_label(id)
            ))
        })
    }

    async fn update_settings(&self) -> UpdateSettings {
        let Some(settings) = crate::lock(&self.inner.settings).clone() else {
            return UpdateSettings::default();
        };
        settings.read_or_default("core.updates").await
    }

    fn changed(&self) {
        self.inner.bus.emit(Event::ToolsChanged);
    }
}

impl ManagedTools {
    #[cfg(feature = "net")]
    async fn release_nearby(&self, key: &str) -> Option<super::source::Resolved> {
        let near = crate::lock(&self.inner.near).clone()?;
        let (url, file, version) = near.release(key).await?;
        Some(super::source::Resolved { version, url, file })
    }

    #[cfg(not(feature = "net"))]
    async fn release_nearby(&self, _key: &str) -> Option<super::source::Resolved> {
        None
    }

    #[cfg(feature = "net")]
    async fn keep_release(&self, key: &str, resolved: &super::source::Resolved) {
        let near = crate::lock(&self.inner.near).clone();
        if let Some(near) = near {
            near.keep_release(key, &resolved.url, resolved.version.clone())
                .await;
        }
    }

    #[cfg(not(feature = "net"))]
    async fn keep_release(&self, _key: &str, _resolved: &super::source::Resolved) {}

    #[cfg(feature = "net")]
    async fn fetch(
        &self,
        url: &url::Url,
        to: &std::path::Path,
        cancel: &CancellationToken,
        progress: impl FnMut(u64, Option<u64>) + Send,
    ) -> Result<()> {
        let near = crate::lock(&self.inner.near).clone();
        super::download::fetch(url, to, cancel, near.as_ref(), progress).await
    }

    #[cfg(not(feature = "net"))]
    async fn fetch(
        &self,
        _url: &url::Url,
        _to: &std::path::Path,
        _cancel: &CancellationToken,
        _progress: impl FnMut(u64, Option<u64>) + Send,
    ) -> Result<()> {
        Err(Error::Io(std::io::Error::other(
            "built without network support",
        )))
    }
}
