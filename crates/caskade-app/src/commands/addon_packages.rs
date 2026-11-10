//! Installing, updating and removing catalogue add-ons (#182) through the
//! plugin host and the theme service, not a second package manager.
//!
//! An install downloads the release the catalogue offers, checks it
//! against the catalogue's size and SHA-256, unpacks it into staging,
//! checks it is that add-on at that version and that it is valid, then
//! swaps it into place with a journal (`caskade_addons::package`).
//! A plugin that won't start, or a theme with problems, rolls back to the
//! previous version. A first plugin install is reviewed (its permissions)
//! before it is enabled; an update keeps the user's grant, so a plugin that
//! asks for more is left waiting for consent in Settings → Plugins, never
//! granted silently.
use crate::error::{CommandError, report};
use caskade_addons::{
    AddonKind, AppMode, CatalogueService, ItemState, evaluate_entry,
    package::{
        self, AddonFailure, AddonRecord, AddonRecords, CancellationToken, PackageError,
        RejectedRelease, UpdatePreference,
    },
};
use caskade_core::{
    db::LibraryStore,
    settings::{AppearanceSettings, SettingsService},
    themes::{ThemeService, ThemeSource},
};
use caskade_plugin_host::{PluginHost, PluginPreview, PluginStatus};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{Event, Events};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};
/// Where add-ons live: plugins and themes in their folders, and the
/// journal, downloads and staging in `<data>/addons`.
pub struct AddonFolders {
    pub plugins: PathBuf,
    pub themes: PathBuf,
    pub work: PathBuf,
}

/// Installs under way, to cancel.
#[derive(Default)]
pub struct AddonJobs(Mutex<HashMap<String, CancellationToken>>);

/// How far an install is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AddonPhase {
    Downloading,
    Checking,
    Installing,
    Starting,
}

/// An install's progress, as it goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonProgress {
    pub id: String,
    pub phase: AddonPhase,
    pub received: u64,
    pub total: u64,
}

/// What became of an install or update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AddonOutcome {
    Installed {
        id: String,
        version: String,
        /// An update asks for more than the user granted: it waits, off,
        /// for their consent in Settings → Plugins.
        needs_consent: bool,
        /// Its pages are already loaded; they change after a restart.
        restart_required: bool,
    },
    /// A first install of a plugin: review what it asks for, then install
    /// again with `accept`.
    NeedsReview {
        id: String,
        version: String,
        preview: Box<PluginPreview>,
    },
    Failed {
        id: String,
        kind: AddonFailure,
        message: String,
        /// The previous version is back in place.
        rolled_back: bool,
    },
}

fn failed(id: &str, error: PackageError, rolled_back: bool) -> AddonOutcome {
    AddonOutcome::Failed {
        id: id.to_owned(),
        kind: error.kind,
        message: error.message,
        rolled_back,
    }
}

fn storage(error: impl std::fmt::Display) -> PackageError {
    PackageError::new(AddonFailure::Storage, error.to_string())
}

fn incompatible(message: String) -> PackageError {
    PackageError::new(AddonFailure::Incompatible, message)
}

struct Prepared {
    entry: caskade_addons::CatalogueEntry,
    release: caskade_addons::CatalogueRelease,
    installed: Option<caskade_addons::InstalledAddon>,
}

/// Stops an install of `id` while it downloads; whether one was running.
pub fn addons_install_cancel(jobs: &AddonJobs, id: String) -> bool {
    jobs.0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&id)
        .inspect(|token| token.cancel())
        .is_some()
}

/// When the theme just removed was the one chosen, the default takes over.
async fn fall_back_from(settings: &SettingsService, removed: &str) {
    let Ok(snapshot) = settings.get("core.appearance").await else {
        return;
    };
    if snapshot.value.get("theme").and_then(|t| t.as_str()) != Some(removed) {
        return;
    }
    let mut value = snapshot.value.clone();
    value["theme"] = AppearanceSettings::default().theme.into();
    if let Err(error) = settings.set("core.appearance", value, None).await {
        tracing::warn!(%error, "the default theme wasn't chosen after removing the active one");
    }
}

/// What Caskade remembers about catalogue add-ons, and the update choice.
pub async fn addons_records(store: &LibraryStore) -> Result<AddonRecords, CommandError> {
    AddonRecords::load(store).await.map_err(report)
}

/// Skips `version` of `id`'s updates (`null`: offer every update again).
pub async fn addons_skip_update(
    store: &LibraryStore,
    id: String,
    version: Option<String>,
) -> Result<AddonRecords, CommandError> {
    let mut records = AddonRecords::load(store).await.map_err(report)?;
    let record: &mut AddonRecord = records.addons.entry(id).or_default();
    record.skipped = version;
    records.save(store).await.map_err(report)?;
    Ok(records)
}

/// Whether Settings checks for add-on updates when it opens.
pub async fn addons_set_update_preference(
    store: &LibraryStore,
    preference: UpdatePreference,
) -> Result<AddonRecords, CommandError> {
    let mut records = AddonRecords::load(store).await.map_err(report)?;
    records.updates = preference;
    records.save(store).await.map_err(report)?;
    Ok(records)
}

/// The services an install or removal uses, as the shell holds them.
pub struct AddonServices<'a> {
    /// Where install progress goes.
    pub events: Events,
    pub catalogue: Option<&'a CatalogueService>,
    pub plugins: Option<&'a PluginHost>,
    pub themes: Option<&'a ThemeService>,
    pub bus: Option<&'a caskade_core::bus::Bus>,
    pub settings: Option<&'a SettingsService>,
}

fn progress(app: &AddonServices<'_>, id: &str, phase: AddonPhase, received: u64, total: u64) {
    app.events.send(Event::AddonProgress(AddonProgress {
        id: id.to_owned(),
        phase,
        received,
        total,
    }));
}

async fn prepare(
    app: &AddonServices<'_>,
    id: &str,
    mode: AppMode,
) -> std::result::Result<Prepared, PackageError> {
    let service = app
        .catalogue
        .ok_or_else(|| incompatible(caskade_core::tr!("the add-on catalogue is unavailable")))?;
    let catalogue = service.catalogue();
    let entry = catalogue.entry(id).cloned().ok_or_else(|| {
        incompatible(caskade_core::tr!(
            "This add-on isn't in the catalogue any more."
        ))
    })?;
    let installed = super::addons::installed(app.plugins, app.themes).await;
    let compatibility = evaluate_entry(&entry, &installed, &super::addons::host_facts(mode));
    let release = match (compatibility.state, compatibility.release) {
        (ItemState::Incompatible, _) | (_, None) => {
            let why = compatibility
                .reasons
                .iter()
                .map(|reason| reason.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(incompatible(why));
        }
        (_, Some(release)) => release,
    };
    Ok(Prepared {
        installed: installed.into_iter().find(|addon| addon.id == entry.id),
        entry,
        release,
    })
}

/// Downloads, checks and unpacks the release into a staging folder; the
/// package's folder and the staging folder to remove afterwards.
async fn stage(
    app: &AddonServices<'_>,
    folders: &AddonFolders,
    prepared: &Prepared,
    cancel: &CancellationToken,
) -> std::result::Result<(PathBuf, PathBuf), PackageError> {
    let (id, total) = (prepared.entry.id.as_str(), prepared.release.artifact.size);
    let archive = package::download(
        &prepared.release,
        &folders.work.join("downloads"),
        cancel,
        |received| progress(app, id, AddonPhase::Downloading, received, total),
    )
    .await?;
    progress(app, id, AddonPhase::Checking, total, total);
    let staging = folders
        .work
        .join("staging")
        .join(caskade_core::model::new_id());
    let root = {
        let (archive, staging) = (archive.clone(), staging.clone());
        tokio::task::spawn_blocking(move || package::extract(&archive, &staging))
            .await
            .map_err(storage)?
    };
    let root = match root {
        Ok(root) => root,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            // A malformed archive won't unpack next time either.
            let _ = std::fs::remove_file(&archive);
            return Err(error);
        }
    };
    if let Err(error) = package::check_identity(&prepared.entry, &prepared.release, &root) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok((root, staging))
}

/// The plugin's half of an install, from a staged folder.
async fn install_plugin(
    app: &AddonServices<'_>,
    folders: &AddonFolders,
    prepared: &Prepared,
    root: &Path,
    accept: bool,
) -> AddonOutcome {
    let id = prepared.entry.id.as_str();
    let version = prepared.release.version.clone();
    let Some(host) = app.plugins else {
        return failed(
            id,
            incompatible(caskade_core::tr!("plugins are unavailable")),
            false,
        );
    };
    let preview = match host.preview_folder(root).await {
        Ok(preview) => preview,
        Err(error) => return failed(id, incompatible(error.to_string()), false),
    };
    if preview.plugin.status == PluginStatus::Invalid || !preview.plugin.errors.is_empty() {
        let why = preview
            .plugin
            .errors
            .iter()
            .map(|error| format!("{}: {}", error.key, error.message))
            .collect::<Vec<_>>()
            .join("; ");
        return failed(id, incompatible(why), false);
    }
    let fresh = prepared.installed.is_none();
    if fresh && !accept {
        return AddonOutcome::NeedsReview {
            id: id.to_owned(),
            version,
            preview: Box::new(preview),
        };
    }
    let was_enabled = host.enabled(id);
    let had_ui = host
        .list()
        .await
        .iter()
        .any(|plugin| plugin.id == id && plugin.has_ui);
    if !fresh {
        let _ = host.deactivate(id).await;
    }
    progress(app, id, AddonPhase::Installing, 0, 0);
    let target = host
        .folder(id)
        .filter(|folder| folder.parent() == Some(folders.plugins.as_path()))
        .unwrap_or_else(|| folders.plugins.join(id));
    let swap = match package::swap_in(root, &target, &folders.work) {
        Ok(swap) => swap,
        Err(error) => return failed(id, error, false),
    };
    let started = async {
        host.discover().await.map_err(|e| e.to_string())?;
        if fresh {
            host.set_enabled(id, true)
                .await
                .map_err(|e| e.to_string())?;
        }
        let summary = host
            .list()
            .await
            .into_iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| caskade_core::tr!("the plugin didn't load"))?;
        if summary.status == PluginStatus::Invalid {
            return Err(caskade_core::tr!("the plugin's manifest is invalid"));
        }
        if host.enabled(id) && summary.has_backend {
            progress(app, id, AddonPhase::Starting, 0, 0);
            host.activate(id).await.map_err(|e| e.to_string())?;
        }
        Ok(summary)
    }
    .await;
    let outcome = match started {
        Ok(summary) => {
            if let Err(error) = swap.commit() {
                tracing::warn!(%error, plugin = id, "the previous version wasn't removed");
            }
            AddonOutcome::Installed {
                id: id.to_owned(),
                version,
                needs_consent: summary.status == PluginStatus::NeedsConsent,
                restart_required: !fresh && was_enabled && had_ui,
            }
        }
        Err(message) => {
            let _ = host.deactivate(id).await;
            let rolled_back = swap.rollback().is_ok();
            let _ = host.discover().await;
            if fresh {
                // Nothing stays of a first install that didn't start.
                let _ = host.uninstall_with(id, true).await;
            }
            failed(
                id,
                PackageError::new(AddonFailure::Activation, message),
                rolled_back,
            )
        }
    };
    // The webview loads and unloads UI halves on this: an update that waits
    // for consent, or one rolled back, mustn't leave the old one running.
    if let Some(bus) = app.bus {
        bus.emit(caskade_core::bus::Event::PluginsChanged {
            plugin: id.to_owned(),
        });
    }
    outcome
}

/// The theme's half of an install, from a staged folder.
fn install_theme(
    app: &AddonServices<'_>,
    folders: &AddonFolders,
    prepared: &Prepared,
    root: &Path,
) -> AddonOutcome {
    let id = prepared.entry.id.as_str();
    let Some(themes) = app.themes else {
        return failed(
            id,
            incompatible(caskade_core::tr!("themes are unavailable")),
            false,
        );
    };
    let problems = ThemeService::check_folder(root);
    if !problems.is_empty() {
        let why = problems
            .iter()
            .map(|problem| format!("{}: {}", problem.file, problem.message))
            .collect::<Vec<_>>()
            .join("; ");
        return failed(id, incompatible(why), false);
    }
    if themes
        .list()
        .iter()
        .any(|theme| theme.id == id && theme.source != ThemeSource::User)
    {
        return failed(
            id,
            PackageError::new(
                AddonFailure::Identity,
                caskade_core::tr!("a built-in or plugin theme already has this id"),
            ),
            false,
        );
    }
    progress(app, id, AddonPhase::Installing, 0, 0);
    let swap = match package::swap_in(root, &folders.themes.join(id), &folders.work) {
        Ok(swap) => swap,
        Err(error) => return failed(id, error, false),
    };
    themes.reload();
    let applies = themes
        .list()
        .into_iter()
        .find(|theme| theme.id == id)
        .is_some_and(|theme| theme.problems.is_empty());
    if applies {
        if let Err(error) = swap.commit() {
            tracing::warn!(%error, theme = id, "the previous version wasn't removed");
        }
        AddonOutcome::Installed {
            id: id.to_owned(),
            version: prepared.release.version.clone(),
            needs_consent: false,
            restart_required: false,
        }
    } else {
        let rolled_back = swap.rollback().is_ok();
        themes.reload();
        failed(
            id,
            PackageError::new(
                AddonFailure::Activation,
                caskade_core::tr!("the theme didn't load"),
            ),
            rolled_back,
        )
    }
}

/// Installs or updates `id` to the newest release that fits. A first
/// plugin install comes back for review unless `accept`; a release that
/// failed a check that can't pass by retrying is refused again unless
/// `retry`.
/// Installs or updates `id` to the newest release that fits. A first
/// plugin install comes back for review unless `accept`; a release that
/// failed a check that can't pass by retrying is refused again unless
/// `retry`.
// Tauri states and the call's arguments.
#[allow(clippy::too_many_arguments)]
pub async fn addons_install(
    app: &AddonServices<'_>,
    folders: &AddonFolders,
    jobs: &AddonJobs,
    store: &LibraryStore,
    id: String,
    mode: AppMode,
    accept: bool,
    retry: bool,
) -> Result<AddonOutcome, CommandError> {
    let prepared = match prepare(app, &id, mode).await {
        Ok(prepared) => prepared,
        Err(error) => return Ok(failed(&id, error, false)),
    };
    let mut records = AddonRecords::load(store).await.map_err(report)?;
    let record = records.addons.entry(id.clone()).or_default();
    if !retry
        && let Some(rejected) = &record.rejected
        && rejected.version == prepared.release.version
    {
        return Ok(AddonOutcome::Failed {
            id,
            kind: rejected.kind,
            message: rejected.message.clone(),
            rolled_back: false,
        });
    }
    let cancel = CancellationToken::new();
    {
        let mut running = jobs
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if running.contains_key(&id) {
            return Err(report(caskade_core::Error::Invalid(caskade_core::tr!(
                "{id} is already being installed",
                id = id
            ))));
        }
        running.insert(id.clone(), cancel.clone());
    }
    let outcome = match stage(app, folders, &prepared, &cancel).await {
        Err(error) => failed(&id, error, false),
        Ok((root, staging)) => {
            let outcome = match prepared.entry.kind {
                AddonKind::Plugin => install_plugin(app, folders, &prepared, &root, accept).await,
                AddonKind::Theme => install_theme(app, folders, &prepared, &root),
            };
            let _ = std::fs::remove_dir_all(&staging);
            outcome
        }
    };
    jobs.0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id);
    let record = records.addons.entry(id.clone()).or_default();
    match &outcome {
        AddonOutcome::Installed { version, .. } => {
            record.version = Some(version.clone());
            record.rejected = None;
            record.skipped = None;
        }
        AddonOutcome::Failed { kind, message, .. }
            if !kind.transient() && *kind != AddonFailure::Cancelled =>
        {
            record.rejected = Some(RejectedRelease {
                version: prepared.release.version.clone(),
                kind: *kind,
                message: message.clone(),
            });
        }
        _ => {}
    }
    records.save(store).await.map_err(report)?;
    if let AddonOutcome::Failed { kind, message, .. } = &outcome {
        tracing::warn!(addon = %id, ?kind, %message, "add-on not installed");
    }
    Ok(outcome)
}

/// Removes an installed add-on: a plugin, keeping its data and settings
/// when `keep_data`; a theme Caskade installed (the active one gives way to
/// the default theme). Built-in themes and dev folders stay.
pub async fn addons_uninstall(
    app: &AddonServices<'_>,
    folders: &AddonFolders,
    store: &LibraryStore,
    id: String,
    keep_data: bool,
) -> Result<(), CommandError> {
    let invalid = |message: String| report(caskade_core::Error::Invalid(message));
    if let Some(host) = app.plugins
        && host
            .list()
            .await
            .iter()
            .any(|plugin| plugin.id == id && !plugin.dev)
    {
        host.uninstall_with(&id, keep_data)
            .await
            .map_err(|error| invalid(error.to_string()))?;
    } else if let Some(themes) = app.themes
        && let Some(theme) = themes.list().into_iter().find(|theme| theme.id == id)
    {
        let folder = theme.folder.map(PathBuf::from);
        let ours = theme.source == ThemeSource::User
            && folder
                .as_ref()
                .is_some_and(|folder| folder.parent() == Some(folders.themes.as_path()));
        let Some(folder) = folder.filter(|_| ours) else {
            return Err(invalid(caskade_core::tr!(
                "only themes Caskade installed can be removed"
            )));
        };
        std::fs::remove_dir_all(&folder).map_err(report)?;
        themes.reload();
        if let Some(settings) = app.settings {
            fall_back_from(settings, &id).await;
        }
    } else {
        return Err(invalid(caskade_core::tr!("This add-on isn't installed.")));
    }
    let mut records = AddonRecords::load(store).await.map_err(report)?;
    records.addons.remove(&id);
    records.save(store).await.map_err(report)?;
    Ok(())
}
