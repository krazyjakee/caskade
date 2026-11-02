//! One-click emulator installs (EMULATOR-SETUP-PLAN §3): download an
//! emulator's release, unpack it into `<data>/emulators/<definition>`,
//! write its first-run config, and configure it with its profiles and the
//! platform defaults that are missing.

mod bios;
mod first_run;
mod jobs;
#[cfg(all(test, feature = "net"))]
mod tests;
mod updates;

pub use crate::managed::{Hosts, Resolved, resolve, version_label};
pub use bios::{BiosFetch, BiosFuture, BiosProvider};
pub use updates::available as update_available;
#[cfg(test)]
use {crate::updates::GitHub, url::Url};

use super::{EmulationService, EmulatorDefinition, InstallSource};
use crate::{
    AppPaths, Result, bus::Bus, db::LibraryStore, launch::LaunchService, settings::SettingsService,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

/// An emulator Caskade installed, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEmulator {
    pub emulator_id: String,
    pub definition_id: String,
    /// `github`, `dolphin`, `url` or `flatpak`.
    pub source: String,
    /// Where it was unpacked; `None` for Flatpaks.
    pub dir: Option<String>,
    /// The version installed; `None` when the source doesn't tell.
    pub installed: Option<String>,
    /// The file it was installed from.
    pub file: Option<String>,
    pub latest: Option<String>,
    pub latest_url: Option<String>,
    /// 0 before the first check.
    pub checked_at_ms: i64,
    pub skipped: Option<String>,
    /// Why the last check failed: as stored ([`crate::i18n::Text::store`])
    /// until [`ManagedEmulator::shown`] translates it.
    pub error: Option<String>,
}

impl ManagedEmulator {
    /// As people see it: the last check's failure in the language in use.
    pub fn shown(mut self) -> Self {
        self.error = self
            .error
            .map(|error| crate::i18n::Text::load(&error).render());
        self
    }
}

/// An emulator Caskade can install on this OS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct Installable {
    pub definition_id: String,
    pub name: String,
    pub website: Option<String>,
    /// Platform ids its profiles run.
    pub platforms: Vec<String>,
    /// `github`, `dolphin`, `url` or `flatpak`.
    pub source: String,
    /// Caskade finds its updates (Flatpak updates its own).
    pub updates: bool,
    /// Its releases, for people.
    pub project_url: Option<String>,
    pub managed: Option<ManagedEmulator>,
    /// The version installed and the newest found, for people.
    pub installed_label: Option<String>,
    pub latest_label: Option<String>,
    /// A newer version than the installed one was found and not skipped.
    pub update_available: bool,
}

/// A platform the user has games for that no emulator runs yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct MissingPlatform {
    pub platform_id: String,
    pub name: String,
    pub short: String,
    pub games: u32,
    /// What one click installs; `None` when nothing can be installed for
    /// it on this OS.
    pub recommended: Option<Recommendation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub definition_id: String,
    pub name: String,
    /// The definition profile that runs the platform.
    pub profile: String,
}

/// Where an install or update is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Queued,
    Downloading,
    Unpacking,
    Configuring,
    Done,
    Failed,
    Cancelled,
}

impl SetupStatus {
    pub fn finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

/// One emulator install or update (§3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupJob {
    pub id: String,
    /// Bumped on every change.
    pub revision: u32,
    pub definition_id: String,
    pub name: String,
    /// The platform it was installed for, if any.
    pub platform_id: Option<String>,
    pub status: SetupStatus,
    pub version: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// The emulator, once configured.
    pub emulator_id: Option<String>,
    /// Platforms that got it as their default.
    pub defaults_set: Vec<String>,
    /// BIOS files fetched for them (from RomM).
    pub fetched_bios: Vec<String>,
    /// Things the user should know: a BIOS to add, a step that failed.
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    emulation: EmulationService,
    /// `<data>/emulators`.
    root: PathBuf,
    /// `<cache>/emulators`: downloads.
    cache: PathBuf,
    hosts: Hosts,
    /// The `flatpak` program.
    flatpak: PathBuf,
    jobs: Mutex<BTreeMap<String, jobs::JobState>>,
    /// `core.updates`; `None` checks every 6 hours.
    settings: Mutex<Option<SettingsService>>,
    /// Running games, so an emulator in use isn't replaced.
    launch: Mutex<Option<LaunchService>>,
    /// Where missing BIOS files come from (RomM).
    bios: Mutex<Option<Arc<dyn BiosProvider>>>,
    /// Emulators whose last update check failed, and when to try again.
    retry_at: Mutex<HashMap<String, Instant>>,
}

/// Installs, configures and updates emulators; cheap to clone.
#[derive(Clone)]
pub struct EmulatorSetup {
    inner: Arc<Inner>,
}

impl EmulatorSetup {
    pub fn new(
        store: LibraryStore,
        bus: Bus,
        emulation: EmulationService,
        paths: &AppPaths,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                emulation,
                root: paths.data.join("emulators"),
                cache: paths.cache.join("emulators"),
                hosts: Hosts::default(),
                flatpak: "flatpak".into(),
                jobs: Mutex::default(),
                settings: Mutex::default(),
                launch: Mutex::default(),
                bios: Mutex::default(),
                retry_at: Mutex::default(),
            }),
        }
    }

    /// Update checks follow `core.updates` (on/off, interval).
    pub fn set_settings(&self, settings: SettingsService) {
        *crate::lock(&self.inner.settings) = Some(settings);
    }

    /// Missing BIOS files are fetched from `provider` after an install
    /// and on request.
    pub fn set_bios(&self, provider: Arc<dyn BiosProvider>) {
        *crate::lock(&self.inner.bios) = Some(provider);
    }

    /// Updates wait while a game runs in the emulator. Settable after
    /// construction because the app builds the launch service later.
    pub fn set_launch(&self, launch: LaunchService) {
        *crate::lock(&self.inner.launch) = Some(launch);
    }

    /// Other release hosts and `flatpak` program (tests).
    pub fn with_hosts(self, hosts: Hosts, flatpak: impl Into<PathBuf>) -> Self {
        let inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("with_hosts before the service is shared"));
        Self {
            inner: Arc::new(Inner {
                hosts,
                flatpak: flatpak.into(),
                ..inner
            }),
        }
    }

    /// Where managed emulators are unpacked.
    pub fn root(&self) -> &std::path::Path {
        &self.inner.root
    }

    fn source<'a>(&self, definition: &'a EmulatorDefinition) -> Option<&'a InstallSource> {
        definition.install.on(self.inner.emulation.os())
    }

    /// The platform's recommendation to install: the first with a download
    /// (one Caskade keeps up to date), else the first from Flathub.
    pub(super) fn pick<'a>(
        &self,
        recommended: Vec<(&'a EmulatorDefinition, &'a super::DefinitionProfile)>,
    ) -> Option<(&'a EmulatorDefinition, &'a super::DefinitionProfile)> {
        let source = |definition: &EmulatorDefinition| self.source(definition).cloned();
        recommended
            .iter()
            .find(|(definition, _)| {
                source(definition).is_some_and(|source| source.kind() != "flatpak")
            })
            .or_else(|| {
                recommended
                    .iter()
                    .find(|(definition, _)| source(definition).is_some())
            })
            .copied()
    }

    /// Every emulator Caskade can install on this OS, by name.
    pub async fn installable(&self) -> Result<Vec<Installable>> {
        let catalog = self.inner.emulation.catalog();
        let managed = self.inner.store.managed_emulators().await?;
        let mut list: Vec<Installable> = catalog
            .emulators()
            .filter_map(|definition| {
                let source = self.source(definition)?;
                let mut platforms: Vec<String> = definition
                    .profiles
                    .iter()
                    .flat_map(|profile| profile.platforms.iter().cloned())
                    .collect();
                platforms.sort();
                platforms.dedup();
                let project_url = match source {
                    InstallSource::Github { repo, .. } => {
                        Some(format!("https://github.com/{repo}/releases"))
                    }
                    InstallSource::Flatpak { app } => {
                        Some(format!("https://flathub.org/apps/{app}"))
                    }
                    InstallSource::Dolphin { .. } | InstallSource::Url { .. } => {
                        definition.website.clone()
                    }
                };
                let managed = managed
                    .iter()
                    .find(|row| row.definition_id == definition.id)
                    .cloned();
                Some(Installable {
                    definition_id: definition.id.clone(),
                    name: definition.name.clone(),
                    website: definition.website.clone(),
                    platforms,
                    source: source.kind().into(),
                    updates: source.tracked(),
                    project_url,
                    installed_label: managed
                        .as_ref()
                        .and_then(|row| row.installed.as_deref())
                        .map(version_label),
                    latest_label: managed
                        .as_ref()
                        .and_then(|row| row.latest.as_deref())
                        .map(version_label),
                    update_available: managed.as_ref().is_some_and(updates::available),
                    managed: managed.map(ManagedEmulator::shown),
                })
            })
            .collect();
        list.sort_by_key(|item| item.name.to_lowercase());
        Ok(list)
    }

    /// Platforms with games and no available profile to run them, with
    /// what to install for each (§7).
    pub async fn missing(&self) -> Result<Vec<MissingPlatform>> {
        let catalog = self.inner.emulation.catalog();
        let platforms = self.inner.emulation.platforms().await?;
        Ok(platforms
            .into_iter()
            .filter(|view| {
                view.rom_count > 0 && !view.compatible.iter().any(|profile| profile.available)
            })
            .map(|view| {
                let recommended = self.pick(catalog.recommended(&view.platform.id)).map(
                    |(definition, profile)| Recommendation {
                        definition_id: definition.id.clone(),
                        // A profile with files of its own is a core to name.
                        name: if profile.requires.is_empty() {
                            definition.name.clone()
                        } else {
                            format!("{} ({})", definition.name, profile.name)
                        },
                        profile: profile.name.clone(),
                    },
                );
                MissingPlatform {
                    platform_id: view.platform.id,
                    name: view.platform.name,
                    short: view.platform.short,
                    games: view.rom_count,
                    recommended,
                }
            })
            .collect())
    }
}
