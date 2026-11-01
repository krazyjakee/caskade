use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Stable identifier within one probe result, also used by extraction steps.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(transparent)]
pub struct ArtifactId(pub String);

/// Detected from bytes or a directory's structure, never by running the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Unknown,
    Directory,
    Zip,
    SevenZip,
    Tar,
    Gzip,
    Xz,
    Zstd,
    Bzip2,
    Rar,
    Cab,
    Wim,
    Pe,
    Elf,
    MachO,
    Script,
    Inno,
    Nsis,
    Msi,
    Burn,
    InstallShield,
    Wise,
    SetupFactory,
    AppImage,
    Makeself,
    MojoSetup,
    Deb,
    Rpm,
    Flatpak,
    AppBundle,
    Dmg,
    Pkg,
    Iso,
    Cue,
    Bin,
    Chd,
    Rom,
}

/// The payload's platform, independent of the machine doing the analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PayloadPlatform {
    Windows,
    Linux,
    Macos,
    ConsoleRom { platform: Option<String> },
    Unknown,
}

/// Role inferred from metadata, container entries and file names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Installer,
    Application,
    Uninstaller,
    Redistributable,
    Launcher,
    Data,
    Document,
    Unknown,
}

/// A statically identified file, directory or entry inside a container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ArtifactNode {
    pub id: ArtifactId,
    /// Root nodes use an input-relative name; children use container-relative names.
    pub path: PathBuf,
    pub format: Format,
    pub platform: PayloadPlatform,
    pub role: Role,
    /// Unknown for some container entries until their headers have been read.
    pub size: Option<u64>,
    /// Namespaced format metadata, e.g. `inno.app_name` or `pe.overlay_offset`.
    pub facts: BTreeMap<String, serde_json::Value>,
    /// Empty while a container has not been listed, or when it has no children.
    pub children: Vec<ArtifactNode>,
    /// Distinguishes an unlisted container from one whose listing is empty.
    pub children_complete: bool,
}
