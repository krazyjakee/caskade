//! Emulation: platform and emulator definitions as data,
//! the variables profiles use, argv resolution, and the user's emulator
//! configuration.
//!
//! Definitions are TOML text handed to [`Catalog::load`]; the shipped files
//! live in `caskade-sources`. Resolution is pure: [`resolve`] turns profile
//! templates into argv tokens for an install kind on an OS, and the launch
//! pipeline adds the install's program (§5.1).

mod bios;
mod catalog;
mod config;
mod definition;
mod detect;
mod handoff;
mod job_table;
pub mod scan;
mod scanners;
mod service;
pub mod setup;
#[cfg(test)]
mod tests;
mod validate;
mod vars;

pub use bios::{BiosHashes, BiosState, bios_dirs};
pub use caskade_installer::OperatingSystem;
pub use catalog::{Catalog, DefinitionError, DefinitionErrors, Resolved, known, resolve};
pub use config::{
    ArgsOverride, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget, GameRom,
    PlatformSettings, ProfileHooks, ProfileRef, RomLocation, RomScanner, Tracking,
};
pub use definition::{
    Bios, DefinitionProfile, DefinitionSaves, Detect, DetectRules, Display, EmulatorDefinition,
    Install, InstallKind, InstallSource, OSES, OsVars, Platform, PlatformIds, Region, RequiresFrom,
    SetupAction, SetupStep, Vars, os_key,
};
pub use detect::{DetectedInstall, DetectedProfile, detect, folders as detect_folders};
pub use handoff::{platform_for, rom_entry};
pub use scanners::{
    ROMS_SOURCE, ReviewChoice, ReviewGame, ReviewRom, RomFolderSetup, RomOverride, ScanJob,
    ScanMode, ScanShared, ScanStatus, ScanSummary, ScannerCount, ScannerService, ScannerView,
};
pub(crate) use service::flatpak_installations;
pub use service::{
    AddableProfile, ArgsDrift, ArgvPreview, Availability, BiosEntry, Command, CompatibleProfile,
    DefinitionInfo, DefinitionProfileInfo, DetectImport, DetectJob, DetectStatus, DiscView,
    EmulationCatalog, EmulationOverview, EmulationService, EmulatorStatus, EmulatorView,
    GameEmulation, PlatformInfo, PlatformView, ProfileDraft, ProfileView, Readiness, VariableInfo,
    command, emulator_dir, host_os, installed,
};
pub use setup::{
    BiosFetch, BiosFuture, BiosProvider, EmulatorSetup, Installable, ManagedEmulator,
    MissingPlatform, Recommendation, SetupJob, SetupStatus,
};
pub use vars::{BUILTINS, OsFolders, VarContext, builtin, names};

use crate::{Error, Result};

impl Resolved {
    /// A built-in profile is available when every required file exists.
    pub fn available(&self) -> bool {
        self.requires.iter().all(|path| path.exists())
    }
}

/// Checks user-entered templates when they are saved: custom profiles have
/// only the built-in variables (§3.3: unknown variables fail at save time,
/// never become empty strings at launch).
pub fn check_templates<'a>(
    vars: &Vars,
    kind: InstallKind,
    templates: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    for template in templates {
        for name in names(template).map_err(Error::Invalid)? {
            if !OSES.iter().any(|os| known(vars, name, *os, kind)) {
                return Err(Error::Invalid(crate::tr!(
                    "unknown variable `{variable}`",
                    variable = format!("{{{name}}}")
                )));
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod readiness_tests;
