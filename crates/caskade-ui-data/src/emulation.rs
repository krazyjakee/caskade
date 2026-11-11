//! Settings → Emulation (#115, #116, #105): the configured emulators and
//! their profiles, platforms and their defaults, detecting installed
//! emulators, ROM scanners and their scans, one-click installs and the
//! emulators other devices use, through the app layer's commands as the
//! webview's `api/emulationTransport.ts` and `api/emulatorSetup.ts` do.
//! Every check is the core's; a refusal comes back as its report.

use caskade_app::commands::{addon_offers, emulation, emulator_setup, rom_scanners};
use caskade_core::db::LibraryStore;
use caskade_core::devices::Devices;
use caskade_core::emulation::{EmulationService, EmulatorSetup};
use caskade_core::error::ErrorReport;
use caskade_core::sync::SyncService;

/// The types the panel shows.
pub use caskade_app::commands::addon_offers::DeviceEmulator;
pub use caskade_core::emulation::{
    ArgsOverride, ArgvPreview, BiosFetch, DetectImport, DetectJob, EmulationCatalog,
    EmulationOverview, Emulator, EmulatorInstall, EmulatorView, Installable, ManagedEmulator,
    MissingPlatform, PlatformSettings, PlatformView, ProfileDraft, ReviewChoice, RomFolderSetup,
    RomScanner, ScanJob, ScanMode, ScanSummary, ScannerView, SetupJob,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What Settings → Emulation reads and changes, over one app's emulation.
#[derive(Clone)]
pub struct Emulation {
    emulation: EmulationService,
    store: LibraryStore,
    /// One-click installs.
    setup: EmulatorSetup,
    sync: Option<SyncService>,
    devices: Option<Devices>,
}

impl Emulation {
    /// Without other devices: none offer their emulators.
    pub fn new(emulation: EmulationService, setup: EmulatorSetup, store: LibraryStore) -> Self {
        Self {
            emulation,
            store,
            setup,
            sync: None,
            devices: None,
        }
    }

    /// Over `store` with Caskade's shipped definitions, as the app builds
    /// it: for tests and fixtures.
    pub fn shipped(
        store: LibraryStore,
        bus: caskade_core::bus::Bus,
        paths: &caskade_core::paths::AppPaths,
    ) -> Self {
        let catalog = caskade_sources::emulation::catalog().unwrap_or_default();
        let emulation =
            EmulationService::new(store.clone(), bus.clone(), catalog).with_paths(paths);
        let setup = EmulatorSetup::new(store.clone(), bus, emulation.clone(), paths);
        Self::new(emulation, setup, store)
    }

    /// The running app's.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self {
            emulation: services.emulation.clone(),
            store: services.store.clone(),
            setup: services.emulator_setup.clone(),
            sync: Some(services.sync.clone()),
            devices: Some(services.devices.clone()),
        }
    }

    /// The emulators and platforms Caskade knows, and the variables
    /// arguments may use.
    pub fn catalog(&self) -> EmulationCatalog {
        emulation::emulation_catalog(&self.emulation)
    }

    pub async fn overview(&self) -> Result<EmulationOverview> {
        emulation::emulation_overview(&self.emulation).await
    }

    pub async fn emulators(&self) -> Result<Vec<EmulatorView>> {
        emulation::emulators_list(&self.emulation).await
    }

    pub async fn platforms(&self) -> Result<Vec<PlatformView>> {
        emulation::emulation_platforms(&self.emulation).await
    }

    /// Adds (an empty id) or changes an emulator, its profiles with it.
    pub async fn save_emulator(&self, emulator: Emulator) -> Result<EmulatorView> {
        emulation::emulator_save(&self.emulation, emulator).await
    }

    pub async fn delete_emulator(&self, id: &str) -> Result<()> {
        emulation::emulator_delete(&self.emulation, id).await
    }

    /// The command line a profile would run, and what it lacks.
    pub fn preview(&self, draft: &ProfileDraft) -> ArgvPreview {
        emulation::profile_preview(&self.emulation, draft)
    }

    /// Arguments as typed, split as the launcher will.
    pub fn split_args(text: &str) -> Result<Vec<String>> {
        emulation::args_split(text)
    }

    /// A platform's default profile and BIOS folder.
    pub async fn set_platform(&self, settings: PlatformSettings) -> Result<PlatformView> {
        emulation::platform_settings_set(&self.emulation, settings).await
    }

    /// Looks for installed emulators, in `folder` or the usual places.
    pub async fn detect(&self, folder: Option<String>) -> Result<DetectJob> {
        emulation::emulation_detect_start(&self.emulation, folder).await
    }

    pub fn detection(&self) -> Option<DetectJob> {
        emulation::emulation_detect_latest(&self.emulation)
    }

    pub fn cancel_detection(&self, id: &str) -> Result<DetectJob> {
        emulation::emulation_detect_cancel(&self.emulation, id)
    }

    /// Adds the found installs `keys` of detection `id` at `revision`.
    pub async fn import_detected(
        &self,
        id: &str,
        revision: u32,
        keys: &[String],
    ) -> Result<DetectImport> {
        emulation::emulation_detect_import(&self.emulation, id, revision, keys).await
    }

    /// Every ROM scanner, with what it found last.
    pub async fn scanners(&self) -> Result<Vec<ScannerView>> {
        rom_scanners::rom_scanners_list(&self.emulation, &self.store).await
    }

    /// Adds (an empty id) or changes a scanner.
    pub async fn save_scanner(&self, scanner: RomScanner) -> Result<RomScanner> {
        rom_scanners::rom_scanner_save(&self.emulation, scanner).await
    }

    pub async fn delete_scanner(&self, id: &str) -> Result<()> {
        rom_scanners::rom_scanner_delete(&self.emulation, id).await
    }

    /// Gives each console folder in a ROM collection a scanner.
    pub async fn setup_rom_folder(&self, folder: &str) -> Result<RomFolderSetup> {
        rom_scanners::rom_folder_setup(&self.emulation, folder).await
    }

    pub async fn start_scan(&self, scanner_id: &str, mode: ScanMode) -> Result<ScanJob> {
        rom_scanners::rom_scan_start(&self.emulation, scanner_id, mode).await
    }

    pub fn scan_jobs(&self) -> Vec<ScanJob> {
        rom_scanners::rom_scan_jobs(&self.emulation)
    }

    pub fn cancel_scan(&self, id: &str) -> Result<ScanJob> {
        rom_scanners::rom_scan_cancel(&self.emulation, id)
    }

    /// Imports a reviewed scan's choices.
    pub async fn submit_review(
        &self,
        id: &str,
        revision: u32,
        choices: Vec<ReviewChoice>,
    ) -> Result<ScanJob> {
        rom_scanners::rom_scan_submit(&self.emulation, id, revision, choices).await
    }

    /// Every scanner in library updates imports now.
    pub async fn scan_all(&self) -> Result<Vec<ScanSummary>> {
        rom_scanners::rom_scan_all(&self.emulation).await
    }

    /// Exclusions every scanner applies.
    pub async fn exclusions(&self) -> Result<Vec<String>> {
        rom_scanners::rom_exclusions(&self.emulation).await
    }

    pub async fn set_exclusions(&self, patterns: Vec<String>) -> Result<Vec<String>> {
        rom_scanners::rom_exclusions_set(&self.emulation, patterns).await
    }

    /// Every emulator Caskade can install here.
    pub async fn installable(&self) -> Result<Vec<Installable>> {
        emulator_setup::emulator_setup_installable(&self.setup).await
    }

    /// Platforms with games that no emulator runs yet.
    pub async fn missing(&self) -> Result<Vec<MissingPlatform>> {
        emulator_setup::emulator_setup_missing(&self.setup).await
    }

    pub async fn install(&self, definition_id: &str) -> Result<SetupJob> {
        emulator_setup::emulator_setup_install(&self.setup, definition_id).await
    }

    /// Installs the platform's recommended emulator.
    pub async fn install_for(&self, platform_id: &str) -> Result<SetupJob> {
        emulator_setup::emulator_setup_install_for(&self.setup, platform_id).await
    }

    pub fn setup_jobs(&self) -> Vec<SetupJob> {
        emulator_setup::emulator_setup_jobs(&self.setup)
    }

    pub fn cancel_install(&self, job_id: &str) -> Result<SetupJob> {
        emulator_setup::emulator_setup_cancel(&self.setup, job_id)
    }

    /// Looks for a newer version of a Caskade-installed emulator now.
    pub async fn check_update(&self, emulator_id: &str) -> Result<ManagedEmulator> {
        emulator_setup::emulator_setup_check(&self.setup, emulator_id).await
    }

    /// Stops offering the newest version found.
    pub async fn skip_update(&self, emulator_id: &str) -> Result<ManagedEmulator> {
        emulator_setup::emulator_setup_skip(&self.setup, emulator_id).await
    }

    pub async fn update(&self, emulator_id: &str) -> Result<SetupJob> {
        emulator_setup::emulator_setup_update(&self.setup, emulator_id).await
    }

    /// Fetches a platform's missing BIOS files from RomM.
    pub async fn fetch_bios(&self, platform_id: &str) -> Result<BiosFetch> {
        emulator_setup::emulator_setup_fetch_bios(&self.setup, platform_id).await
    }

    /// Emulators the user's other devices use and this one lacks.
    pub async fn from_devices(&self) -> Result<Vec<DeviceEmulator>> {
        addon_offers::emulators_from_devices(
            self.sync.as_ref(),
            self.devices.as_ref(),
            Some(&self.emulation),
            Some(&self.setup),
        )
        .await
    }

    /// Keeps this device's emulator for the platform instead.
    pub async fn keep_mine(&self, platform_id: &str) -> Result<()> {
        addon_offers::emulator_offer_keep(self.sync.as_ref(), platform_id).await
    }
}

impl PartialEq for Emulation {
    /// One app's: a panel's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
