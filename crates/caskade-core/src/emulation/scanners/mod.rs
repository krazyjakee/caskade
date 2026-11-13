//! ROM scanners: saved folders whose ROMs
//! become library games. Automatic scans (a library update, "Scan all",
//! the installer's hand-off) import straight away; a manual scan stops at a
//! review list, and the choices made there are stored as exclusions and
//! overrides so later automatic scans agree with them.
//!
//! Imported entries come from the `roms` source, are keyed on their scanner
//! and their ROM paths stay relative to the scanner folder, so moving a
//! collection means editing one folder.

mod import;
mod jobs;
mod overrides;

use super::{
    EmulationService, GameRom, RomLocation, RomScanner,
    scan::{self, CancellationToken, ScanError, ScanProgress},
};
use crate::{
    Error, Result,
    bus::{Bus, Event},
    db::LibraryStore,
    lock,
    model::{GameEntry, InstallState, LaunchSpec, SourceId, new_id},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

/// The library source of every scanned ROM.
pub const ROMS_SOURCE: SourceId = SourceId::new("roms");

/// A review choice for one ROM, by its path relative to the scanner folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct RomOverride {
    pub title: Option<String>,
    pub platform: Option<String>,
    pub regions: Option<Vec<String>>,
    /// ROMs with the same group are one game, ordered by `order`. `None`
    /// keeps the automatic grouping.
    pub group: Option<String>,
    pub order: Option<u32>,
}

impl RomOverride {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A scanner as the settings list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScannerView {
    pub scanner: RomScanner,
    /// Games the last scan found.
    pub games: u32,
    pub last_scan_ms: Option<i64>,
    /// The platforms it imports to: its own, or its profile's.
    pub platforms: Vec<String>,
    pub extensions: Vec<String>,
    /// Why it can't scan, e.g. its profile is gone.
    pub problem: Option<String>,
    /// The scanner's Flatpak emulator can't see the folder by itself (§11).
    pub flatpak_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    /// Stop at a review list (§6.1 manual).
    Review,
    /// Import straight away (§6.1 automatic).
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Running,
    /// Waiting for `scan_submit`.
    Review,
    Done,
    Failed,
    Cancelled,
}

/// One ROM of a game in the review list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRom {
    pub path: String,
    /// `Disc 2` and the like.
    pub name: Option<String>,
    pub size: Option<u64>,
}

/// A game as a scan found it, with the stored choices applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewGame {
    pub title: String,
    pub platform: String,
    pub regions: Vec<String>,
    pub roms: Vec<ReviewRom>,
    /// Every file it claims, playlists' tracks included.
    pub files: Vec<String>,
    /// The library game it already is, if any.
    pub game_id: Option<String>,
}

/// What the user decided for one game of the review list. Merging puts the
/// ROMs of several games in one choice; splitting spreads one game's ROMs
/// over several.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewChoice {
    /// ROM paths, in disc order.
    pub roms: Vec<String>,
    /// Unticked games are excluded from this and later scans.
    pub include: bool,
    pub title: String,
    pub platform: String,
    pub regions: Vec<String>,
}

/// What one scan changed in the library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub scanner_id: String,
    pub added: u32,
    pub updated: u32,
    /// Entries whose ROMs are gone; they are marked not installed.
    pub missing: u32,
    /// Every game the scan found, new or not.
    pub games: Vec<String>,
    pub warnings: Vec<String>,
    /// Set when the scanner couldn't run; the other fields are then empty.
    pub error: Option<String>,
}

/// Counts per scanner for `emulation.scan.completed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScannerCount {
    pub scanner_id: String,
    pub games: u32,
    pub added: u32,
    pub updated: u32,
    pub missing: u32,
}

impl From<&ScanSummary> for ScannerCount {
    fn from(summary: &ScanSummary) -> Self {
        Self {
            scanner_id: summary.scanner_id.clone(),
            games: summary.games.len() as u32,
            added: summary.added,
            updated: summary.updated,
            missing: summary.missing,
        }
    }
}

/// A running or finished scan; every change is `emulation.scan.changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanJob {
    pub id: String,
    /// Bumped on every change; a submit names the revision it reviewed.
    pub revision: u32,
    pub scanner_id: String,
    pub mode: ScanMode,
    pub status: ScanStatus,
    pub progress: Option<ScanProgress>,
    pub review: Vec<ReviewGame>,
    pub summary: Option<ScanSummary>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

impl super::job_table::TableJob for ScanJob {
    fn id(&self) -> &str {
        &self.id
    }
    fn revision(&mut self) -> &mut u32 {
        &mut self.revision
    }
    fn finished(&self) -> bool {
        !matches!(self.status, ScanStatus::Running | ScanStatus::Review)
    }
}

/// Scan jobs and per-scanner locks, shared by every `ScannerService` of one
/// emulation service.
#[derive(Default)]
pub struct ScanShared {
    jobs: super::job_table::JobTable<ScanJob, CancellationToken>,
    busy: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

/// The settings key of the global exclusions: a JSON list of patterns.
const GLOBAL_EXCLUSIONS: &str = "emulation.rom_exclusions";
const MAX_GLOBAL_EXCLUSIONS: usize = 1000;

/// ROM scanners over the emulation service's store, bus and catalog; cheap
/// to clone. Get one from [`EmulationService::scanners`].
#[derive(Clone)]
pub struct ScannerService {
    store: LibraryStore,
    bus: Bus,
    emulation: EmulationService,
    shared: Arc<ScanShared>,
}

/// What a scanner's profile resolves to.
struct Resolved {
    platforms: Vec<super::Platform>,
    extensions: Vec<String>,
}

fn scan_error(error: ScanError) -> Error {
    match error {
        ScanError::Io(error) => Error::Io(error),
        other => Error::Invalid(other.to_string()),
    }
}

impl ScannerService {
    pub fn new(
        store: LibraryStore,
        bus: Bus,
        emulation: EmulationService,
        shared: Arc<ScanShared>,
    ) -> Self {
        Self {
            store,
            bus,
            emulation,
            shared,
        }
    }

    /// Exclusions every scanner applies, in
    /// Playnite's syntax.
    pub async fn global_exclusions(&self) -> Result<Vec<String>> {
        match self.store.get_setting(GLOBAL_EXCLUSIONS).await? {
            Some(value) => serde_json::from_value(value).map_err(|error| {
                Error::Invalid(crate::tr!("global ROM exclusions: {error}", error = error))
            }),
            None => Ok(Vec::new()),
        }
    }

    /// Replaces the global exclusions; the next scan uses them. Blank and
    /// repeated patterns are dropped, and every pattern must compile.
    pub async fn set_global_exclusions(&self, patterns: Vec<String>) -> Result<Vec<String>> {
        let mut kept: Vec<String> = Vec::new();
        for pattern in patterns {
            let pattern = pattern.trim();
            if !pattern.is_empty() && !kept.iter().any(|known| known == pattern) {
                kept.push(pattern.to_owned());
            }
        }
        if kept.len() > MAX_GLOBAL_EXCLUSIONS {
            return Err(Error::Invalid(crate::tr!(
                "at most {max} global exclusions",
                max = MAX_GLOBAL_EXCLUSIONS
            )));
        }
        scan::Exclusions::compile(&kept).map_err(scan_error)?;
        self.store
            .set_setting(GLOBAL_EXCLUSIONS, &serde_json::json!(kept))
            .await?;
        self.changed();
        Ok(kept)
    }

    fn changed(&self) {
        self.bus.emit(Event::EmulationChanged {
            scope: "scanners".into(),
        });
    }

    /// Every scanner with its last scan and what it imports. `sees` tells
    /// whether a Flatpak app can already read a folder.
    pub async fn list(
        &self,
        sees: &(dyn Fn(&str, &Path) -> bool + Sync),
    ) -> Result<Vec<ScannerView>> {
        let states = self.store.scan_states().await?;
        let emulators = self.emulation.emulators().await?;
        let mut views = Vec::new();
        for scanner in self.store.scanners().await? {
            let state = states.get(&scanner.id);
            let (platforms, extensions, problem) = match self.resolve_in(&emulators, &scanner) {
                Ok(resolved) => (
                    resolved.platforms.into_iter().map(|p| p.id).collect(),
                    resolved.extensions,
                    None,
                ),
                Err(error) => (Vec::new(), Vec::new(), Some(error.to_string())),
            };
            let flatpak_hint = emulators
                .iter()
                .map(|view| &view.emulator)
                .find(|emulator| emulator.id == scanner.profile.emulator_id)
                .and_then(|emulator| match &emulator.install {
                    super::EmulatorInstall::Flatpak { app_id }
                        if !sees(app_id, &scanner.folder) =>
                    {
                        Some(crate::tr!(
                            "{emulator} runs as a Flatpak and can't see this folder by itself. Caskade shares it read-only when it launches a game; the emulator's own file browser won't show it.",
                            emulator = emulator.name
                        ))
                    }
                    _ => None,
                });
            views.push(ScannerView {
                games: state.map_or(0, |state| state.games),
                last_scan_ms: state.map(|state| state.last_scan_ms),
                platforms,
                extensions,
                problem,
                flatpak_hint,
                scanner,
            });
        }
        Ok(views)
    }

    /// Creates (empty id) or updates a scanner.
    pub async fn save(&self, mut scanner: RomScanner) -> Result<RomScanner> {
        if scanner.id.is_empty() {
            scanner.id = new_id();
        }
        scanner.name = scanner.name.trim().to_owned();
        scanner
            .exclusions
            .retain(|pattern| !pattern.trim().is_empty());
        scan::Exclusions::compile(&scanner.exclusions).map_err(scan_error)?;
        self.resolve(&scanner).await?;
        let saved = self.store.upsert_scanner(&scanner).await?;
        self.changed();
        Ok(saved)
    }

    /// Deletes a scanner. Its games stay in the library, not installed,
    /// because their ROMs can no longer be found.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let busy = self.busy(id);
        let _guard = busy.lock().await;
        for mut entry in self.store.entries_for_scanner(id).await? {
            if entry.state != InstallState::Partial {
                entry.state = InstallState::Partial;
                let game_id = entry.game_id.clone();
                self.store.upsert_entry(&entry).await?;
                self.bus.emit(Event::LibraryGameUpdated { game_id });
            }
        }
        let deleted = self.store.delete_scanner(id).await?;
        if deleted {
            self.changed();
        }
        Ok(deleted)
    }

    /// Scanners that import games of a platform: their own platform, or
    /// their profile's. Library-update scanners first.
    pub async fn for_platform(&self, platform: &str) -> Result<Vec<RomScanner>> {
        let emulators = self.emulation.emulators().await?;
        let mut found = Vec::new();
        for scanner in self.store.scanners().await? {
            let matches = match &scanner.platform {
                Some(own) => own == platform,
                None => self
                    .resolve_in(&emulators, &scanner)
                    .is_ok_and(|resolved| resolved.platforms.iter().any(|p| p.id == platform)),
            };
            if matches {
                found.push(scanner);
            }
        }
        found.sort_by_key(|scanner| !scanner.in_library_update);
        Ok(found)
    }

    async fn resolve(&self, scanner: &RomScanner) -> Result<Resolved> {
        self.resolve_in(&self.emulation.emulators().await?, scanner)
    }

    fn resolve_in(&self, views: &[super::EmulatorView], scanner: &RomScanner) -> Result<Resolved> {
        let profile = views
            .iter()
            .find(|view| view.emulator.id == scanner.profile.emulator_id)
            .and_then(|view| {
                view.profiles
                    .iter()
                    .find(|profile| profile.id == scanner.profile.profile_id)
            })
            .ok_or_else(|| Error::Invalid(crate::tr!("the scanner's profile does not exist")))?;
        let ids: Vec<&String> = match &scanner.platform {
            Some(platform) if profile.platforms.contains(platform) => vec![platform],
            Some(_) => {
                return Err(Error::Invalid(crate::tr!(
                    "The profile {profile} doesn't run this scanner's platform.",
                    profile = profile.name
                )));
            }
            None => profile.platforms.iter().collect(),
        };
        let catalog = self.emulation.catalog();
        let platforms = ids
            .into_iter()
            .filter_map(|id| catalog.platform(id).cloned())
            .collect::<Vec<_>>();
        if platforms.is_empty() {
            return Err(Error::Invalid(crate::tr!(
                "the scanner has no known platform"
            )));
        }
        Ok(Resolved {
            platforms,
            extensions: profile.extensions.clone(),
        })
    }

    fn busy(&self, scanner_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.shared.busy)
            .entry(scanner_id.to_owned())
            .or_default()
            .clone()
    }
}

/// A scanner ROM's path relative to its folder, `/`-separated.
fn rom_path(rom: &GameRom) -> Option<String> {
    match &rom.location {
        RomLocation::Scanner { path, .. } => Some(path.to_string_lossy().replace('\\', "/")),
        RomLocation::Absolute { .. } => None,
    }
}

fn entry_paths(entry: &GameEntry) -> Vec<String> {
    match &entry.launch {
        Some(LaunchSpec::Emulator { roms, .. }) => roms.iter().filter_map(rom_path).collect(),
        _ => Vec::new(),
    }
}

mod rom_folders;
pub use rom_folders::RomFolderSetup;

#[cfg(test)]
mod tests;
