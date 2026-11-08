//! The user's emulation configuration, stored in SQLite
//! and edited in Settings → Emulators.

use super::InstallKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Component, Path, PathBuf};

/// One installed emulator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct Emulator {
    pub id: String,
    pub name: String,
    /// The definition it follows ("Emulator specification"); `None` is
    /// fully custom.
    pub definition: Option<String>,
    pub install: EmulatorInstall,
    pub profiles: Vec<EmulatorProfile>,
}

impl Emulator {
    pub fn profile(&self, id: &str) -> Option<&EmulatorProfile> {
        self.profiles.iter().find(|profile| profile.id() == id)
    }
}

/// Where an emulator is installed; the kind decides how it launches (§5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EmulatorInstall {
    /// A portable or installed folder.
    Folder {
        dir: PathBuf,
        executable: PathBuf,
    },
    /// A binary found on `$PATH`.
    Path {
        binary: String,
    },
    /// Launched with `flatpak run`.
    Flatpak {
        app_id: String,
    },
    AppImage {
        file: PathBuf,
    },
    /// A macOS `.app`, launched through its inner executable.
    AppBundle {
        bundle: PathBuf,
    },
}

impl EmulatorInstall {
    pub fn kind(&self) -> InstallKind {
        match self {
            Self::Folder { .. } => InstallKind::Folder,
            Self::Path { .. } => InstallKind::Path,
            Self::Flatpak { .. } => InstallKind::Flatpak,
            Self::AppImage { .. } => InstallKind::AppImage,
            Self::AppBundle { .. } => InstallKind::AppBundle,
        }
    }
}

/// How to run one kind of ROM with an emulator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EmulatorProfile {
    /// A profile of the emulator's definition, possibly with other arguments.
    BuiltIn {
        id: String,
        definition_profile: String,
        args: ArgsOverride,
        hooks: ProfileHooks,
    },
    /// Everything user-entered.
    Custom {
        id: String,
        name: String,
        /// `None` runs the emulator's own executable.
        executable: Option<PathBuf>,
        args: Vec<String>,
        working_dir: Option<String>,
        platforms: Vec<String>,
        /// Lowercase, without the dot; `<none>` matches files without one.
        extensions: Vec<String>,
        env: Vec<(String, String)>,
        tracking: Tracking,
        hooks: ProfileHooks,
    },
}

impl EmulatorProfile {
    /// The definition's profile `name` with its default arguments, not
    /// saved yet (no id).
    pub fn built_in(name: impl Into<String>) -> Self {
        Self::BuiltIn {
            id: String::new(),
            definition_profile: name.into(),
            args: ArgsOverride::default(),
            hooks: ProfileHooks::default(),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::BuiltIn { id, .. } | Self::Custom { id, .. } => id,
        }
    }
}

/// Arguments of a built-in profile, or a game's extra arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ArgsOverride {
    #[default]
    Default,
    Replace {
        args: Vec<String>,
        /// The definition's arguments when the override was made, so the
        /// editor can flag later definition fixes (§11). Set by the service.
        #[serde(default)]
        replaced: Vec<String>,
    },
    Append {
        args: Vec<String>,
    },
}

impl ArgsOverride {
    /// The argument templates after this override.
    pub fn apply(&self, base: &[String]) -> Vec<String> {
        match self {
            Self::Default => base.to_vec(),
            Self::Replace { args, .. } => args.clone(),
            Self::Append { args } => base.iter().chain(args).cloned().collect(),
        }
    }

    pub fn args(&self) -> &[String] {
        match self {
            Self::Default => &[],
            Self::Replace { args, .. } | Self::Append { args } => args,
        }
    }
}

/// Commands run around a launch (instead of Playnite's PowerShell scripts).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileHooks {
    /// argv run before launch; a non-zero exit cancels the launch.
    pub before: Option<Vec<String>>,
    pub after_exit: Option<Vec<String>>,
}

/// How the game's process is found once the emulator starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Tracking {
    /// The install kind's default (§5.1).
    #[default]
    Default,
    ProcessTree,
    OriginalProcess,
    /// Any process running from this folder (variables allowed).
    Folder {
        path: String,
    },
    ProcessName {
        name: String,
    },
}

/// A profile of a configured emulator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRef {
    pub emulator_id: String,
    pub profile_id: String,
}

/// Per-platform choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformSettings {
    pub platform: String,
    /// The emulator and profile games of this platform follow unless pinned.
    pub default: Option<ProfileRef>,
    /// Where BIOS files are looked for instead of the emulator's folder.
    pub bios_dir: Option<PathBuf>,
}

/// A saved ROM folder scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RomScanner {
    pub id: String,
    pub name: String,
    pub folder: PathBuf,
    /// `None` takes the profile's platform(s).
    pub platform: Option<String>,
    /// Its extensions decide which files match.
    pub profile: ProfileRef,
    /// What imported games launch with.
    pub launch_with: EmulatorTarget,
    pub recursive: bool,
    pub scan_archives: bool,
    pub skip_cloud_placeholders: bool,
    /// Playnite's exclusion syntax (§6.3): `bios/`, `>*.txt`, `?^beta/.*`,
    /// `>?\(Beta\)`.
    pub exclusions: Vec<String>,
    pub in_library_update: bool,
}

/// What an emulated game launches with. The plan calls this `LaunchTarget`;
/// discovery and the installer already export types of that name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EmulatorTarget {
    /// The platform's default emulator and profile.
    #[default]
    PlatformDefault,
    /// Pinned to one profile.
    Profile {
        emulator_id: String,
        profile_id: String,
    },
    /// Choose a profile of this emulator at launch.
    AskProfile { emulator_id: String },
    /// Choose any compatible emulator at launch.
    AskEmulator,
}

/// A ROM file of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameRom {
    pub location: RomLocation,
    /// Shown in the disc picker, e.g. `Disc 2`.
    pub name: Option<String>,
    pub size: Option<u64>,
}

impl GameRom {
    pub fn absolute(path: impl Into<PathBuf>) -> Self {
        Self {
            location: RomLocation::Absolute { path: path.into() },
            name: None,
            size: None,
        }
    }

    /// The ROM's file. The launcher and the readiness check both resolve
    /// ROMs here, so a path that escapes its scanner folder is rejected by
    /// both.
    pub(crate) async fn path(&self, store: &crate::db::LibraryStore) -> crate::Result<PathBuf> {
        match &self.location {
            RomLocation::Absolute { path } => Ok(path.clone()),
            RomLocation::Scanner { scanner_id, path } => {
                let scanner = store.get_scanner(scanner_id).await?.ok_or_else(|| {
                    crate::Error::Invalid(crate::tr!("the game's ROM scanner no longer exists"))
                })?;
                RomLocation::scanner_path(&scanner.folder, path).ok_or_else(|| {
                    crate::Error::Invalid(crate::tr!("ROM path escapes its scanner folder"))
                })
            }
        }
    }
}

/// Where a ROM is. Paths under a scanner are relative to its folder, so
/// moving a collection means editing one scanner (§4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RomLocation {
    Scanner { scanner_id: String, path: PathBuf },
    Absolute { path: PathBuf },
}

impl RomLocation {
    /// A scanner-relative ROM path joined to the scanner's `folder`, or
    /// `None` when it would leave that folder: absolute, rooted, with a
    /// drive prefix, or climbing out with `..`.
    pub fn scanner_path(folder: &Path, path: &Path) -> Option<PathBuf> {
        let escapes = path.is_absolute()
            || path.has_root()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)));
        (!escapes).then(|| folder.join(path))
    }
}
