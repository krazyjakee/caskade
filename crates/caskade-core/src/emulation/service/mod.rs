//! The emulation service (#116): emulators,
//! profiles and platform settings with what the editor shows next to them:
//! install presence, profile availability, the resolved command line for a
//! sample ROM, and Replace overrides whose definition args changed since.

mod detection;
mod emulators;
mod games;
mod install;
mod platforms;

pub use detection::{DetectImport, DetectJob, DetectStatus};
pub use emulators::{
    AddableProfile, ArgsDrift, ArgvPreview, Availability, BiosEntry, EmulatorStatus, EmulatorView,
    ProfileDraft, ProfileView,
};
pub use games::{DiscView, GameEmulation, Readiness};
pub(crate) use install::flatpak_installations;
pub use install::{Command, command, emulator_dir, host_os, installed};
pub use platforms::{CompatibleProfile, EmulationOverview, PlatformView};

use emulators::bios_entry;

use super::{BUILTINS, Catalog, OsFolders};
use crate::{
    AppPaths, Error, Result,
    bus::{Bus, Event},
    db::LibraryStore,
};
use caskade_installer::OperatingSystem;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub id: String,
    pub name: String,
    pub short: String,
    pub extensions: Vec<String>,
    pub bios: Vec<BiosEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionProfileInfo {
    pub name: String,
    pub platforms: Vec<String>,
    pub extensions: Vec<String>,
    pub args: Vec<String>,
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionInfo {
    pub id: String,
    pub name: String,
    pub website: Option<String>,
    pub profiles: Vec<DefinitionProfileInfo>,
    /// The definition's own variables, for the variables reference.
    pub vars: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct VariableInfo {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
}

/// Read-only data for the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmulationCatalog {
    pub platforms: Vec<PlatformInfo>,
    pub definitions: Vec<DefinitionInfo>,
    pub variables: Vec<VariableInfo>,
}

/// What a built-in variable holds, for the variable list.
fn description(name: &str) -> String {
    match name {
        "RomPath" => crate::tr!("Full path of the ROM being launched"),
        "RomName" => crate::tr!("ROM file name with extension"),
        "RomNameNoExt" => crate::tr!("ROM file name without extension"),
        "RomDir" => crate::tr!("Folder of the ROM"),
        "EmulatorDir" => crate::tr!("Emulator install folder (a Flatpak's data dir)"),
        "Name" => crate::tr!("Game title"),
        "Platform" => crate::tr!("Platform id"),
        "GameId" => crate::tr!("Caskade game id"),
        "CaskadeDir" => crate::tr!("Caskade's data folder"),
        "StorageRoot" => crate::tr!("The default storage location"),
        "UserHome" => crate::tr!("Your home folder"),
        "XdgConfig" => crate::tr!("~/.config on Linux; empty elsewhere"),
        "AppData" => crate::tr!("%AppData% on Windows; empty elsewhere"),
        "ProgramFiles" => crate::tr!("Program Files on Windows; empty elsewhere"),
        "AppSupport" => {
            crate::tr!("~/Library/Application Support on macOS; empty elsewhere")
        }
        _ => String::new(),
    }
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    catalog: Arc<Catalog>,
    os: OperatingSystem,
    folders: OsFolders,
    caskade_dir: Option<PathBuf>,
    /// Where `Path` installs are looked up; `None` reads `$PATH`.
    search_path: Option<Vec<PathBuf>>,
    /// The machine detection looks at.
    env: caskade_discovery::HostEnv,
    jobs: crate::emulation::job_table::JobTable<DetectJob, caskade_discovery::Cancel>,
    hashes: super::BiosHashes,
    scans: Arc<super::ScanShared>,
}

/// Emulators, profiles and platform settings; cheap to clone.
#[derive(Clone)]
pub struct EmulationService {
    inner: Arc<Inner>,
}

impl EmulationService {
    pub fn new(store: LibraryStore, bus: Bus, catalog: Catalog) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                catalog: Arc::new(catalog),
                os: host_os(),
                folders: OsFolders::host(),
                caskade_dir: None,
                search_path: None,
                env: caskade_discovery::HostEnv::live(),
                jobs: Default::default(),
                hashes: super::BiosHashes::default(),
                scans: Arc::default(),
            }),
        }
    }

    fn configure(mut self, change: impl FnOnce(&mut Inner)) -> Self {
        change(Arc::get_mut(&mut self.inner).expect("configure the service before cloning it"));
        self
    }

    /// `{CaskadeDir}` is the app's data folder.
    pub fn with_paths(self, paths: &AppPaths) -> Self {
        let data = paths.data.clone();
        self.configure(|inner| inner.caskade_dir = Some(data))
    }

    /// Resolves for another OS and folders (tests, fixtures).
    pub fn with_host(self, os: OperatingSystem, folders: OsFolders) -> Self {
        self.configure(|inner| {
            inner.os = os;
            inner.folders = folders;
        })
    }

    /// The OS resolution targets (the host, unless `with_host` changed it).
    pub fn os(&self) -> OperatingSystem {
        self.inner.os
    }

    /// The OS folders the variables see.
    pub fn folders(&self) -> &OsFolders {
        &self.inner.folders
    }

    /// `{CaskadeDir}`.
    pub fn caskade_dir(&self) -> Option<&Path> {
        self.inner.caskade_dir.as_deref()
    }

    /// Looks `Path` installs up in these folders instead of `$PATH` (tests).
    pub fn with_search_path(self, dirs: Vec<PathBuf>) -> Self {
        self.configure(|inner| inner.search_path = Some(dirs))
    }

    /// Where `Path` installs are looked up; `None` reads `$PATH`.
    pub fn search_path(&self) -> Option<&[PathBuf]> {
        self.inner.search_path.as_deref()
    }

    pub fn catalog(&self) -> Arc<Catalog> {
        self.inner.catalog.clone()
    }

    /// ROM scanners (#119) over this service's store, bus and catalog.
    pub fn scanners(&self) -> super::ScannerService {
        super::ScannerService::new(
            self.inner.store.clone(),
            self.inner.bus.clone(),
            self.clone(),
            self.inner.scans.clone(),
        )
    }

    fn changed(&self, scope: &str) {
        self.inner.bus.emit(Event::EmulationChanged {
            scope: scope.to_owned(),
        });
    }

    pub fn catalog_view(&self) -> EmulationCatalog {
        let catalog = self.catalog();
        let os = self.inner.os;
        EmulationCatalog {
            platforms: catalog.platforms().map(platform_info).collect(),
            definitions: catalog
                .emulators()
                .map(|definition| DefinitionInfo {
                    id: definition.id.clone(),
                    name: definition.name.clone(),
                    website: definition.website.clone(),
                    profiles: definition
                        .profiles
                        .iter()
                        .map(|profile| DefinitionProfileInfo {
                            name: profile.name.clone(),
                            platforms: profile.platforms.clone(),
                            extensions: profile.extensions.clone(),
                            args: profile.args.clone(),
                            requires: profile.requires.clone(),
                        })
                        .collect(),
                    vars: {
                        let mut names: Vec<String> = super::InstallKind::on(os)
                            .iter()
                            .flat_map(|kind| definition.vars.names(os, *kind))
                            .map(str::to_owned)
                            .collect();
                        names.sort();
                        names.dedup();
                        names
                    },
                })
                .collect(),
            variables: BUILTINS
                .iter()
                .map(|name| VariableInfo {
                    name: (*name).to_owned(),
                    aliases: super::vars::aliases_of(name).map(str::to_owned).collect(),
                    description: description(name),
                })
                .collect(),
        }
    }

    /// Splits a command line typed as one string with shell-word rules.
    pub fn split_args(text: &str) -> Result<Vec<String>> {
        shlex::split(text)
            .ok_or_else(|| Error::Invalid(crate::tr!("unbalanced quotes or a trailing backslash")))
    }
}

fn platform_info(platform: &super::Platform) -> PlatformInfo {
    PlatformInfo {
        id: platform.id.clone(),
        name: platform.name.clone(),
        short: platform.short.clone(),
        extensions: platform.extensions.clone(),
        bios: platform.bios.iter().map(bios_entry).collect(),
    }
}
