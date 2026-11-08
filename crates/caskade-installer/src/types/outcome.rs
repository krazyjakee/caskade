use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

use super::{ArtifactId, Candidate, Format, Input, PayloadPlatform, RuntimeSpec, Step};

/// Nonfatal, machine-readable diagnostic with human-readable context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Warning {
    pub code: String,
    pub message: String,
    /// When present, relative to the game directory.
    pub path: Option<PathBuf>,
}

/// A discovered executable and the data needed to launch it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct LaunchTarget {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub env: BTreeMap<String, String>,
    pub runtime: RuntimeSpec,
    /// Discovery rank; larger scores rank first.
    pub score: f32,
    pub reason: String,
}

/// A redistributable or runtime component discovered in the installed tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Dependency {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub install_steps: Vec<Step>,
    pub uninstall_steps: Vec<Step>,
    /// Equivalent protonfixes/winetricks verbs when using a prefix.
    pub runtime_verbs: Vec<String>,
}

/// Successful managed installation. Paths other than `dir` are game-relative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Installed {
    pub root_id: String,
    pub dir: PathBuf,
    pub title: String,
    pub ids: BTreeMap<String, String>,
    pub launch_targets: Vec<LaunchTarget>,
    pub runtime: RuntimeSpec,
    pub dependencies: Vec<Dependency>,
    pub warnings: Vec<Warning>,
    /// Normally `.caskade/install.json`; does not depend on the manifest schema.
    pub manifest: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum HandOff {
    /// File names relative to the resolved input set, passed to emulation import.
    Rom {
        platform: String,
        files: Vec<PathBuf>,
    },
    Delegated {
        launch_uri: String,
        uninstall_steps: Vec<Step>,
    },
    /// A deb or rpm the system package manager installed. `program` is the
    /// absolute command its desktop entry or binaries name, when it has one.
    Package {
        package: String,
        /// The desktop entry's name, else the package's.
        title: String,
        program: Option<PathBuf>,
        args: Vec<String>,
        uninstall_steps: Vec<Step>,
    },
}

/// A resolver's proposed download asset, suitable for a choice dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct DownloadAsset {
    pub name: String,
    pub url: Url,
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

/// An explicit ambiguity; the default pipeline asks only when it cannot decide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Question {
    /// Observed install folders, never automatic permission to move or delete.
    ChooseInstallDirectory {
        paths: Vec<PathBuf>,
        reason: String,
    },
    /// A move left references Caskade cannot rewrite safely (Fragile, #61):
    /// offer reinstalling from the kept source; never done automatically.
    ReinstallAfterMove {
        title: String,
        source: Input,
        reason: String,
    },
    ChooseLaunchTarget {
        targets: Vec<LaunchTarget>,
    },
    ChooseInstaller {
        artifacts: Vec<ArtifactId>,
    },
    ChooseAsset {
        assets: Vec<DownloadAsset>,
    },
    LowConfidence {
        candidates: Vec<Candidate>,
    },
    /// Asked under the Ask policy before installer code runs: when the best
    /// plan runs it, or when a strategy that doesn't failed and the next one
    /// would (the candidates left). The Allow policy never asks.
    ConfirmExecution {
        candidates: Vec<Candidate>,
    },
    /// Where handed-off files should go, e.g. into a ROM scanner's folder.
    /// The first option is the suggested one.
    ChooseDestination {
        reason: String,
        options: Vec<Destination>,
    },
}

/// One answer to [`Question::ChooseDestination`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Destination {
    pub label: String,
    /// `None` keeps the files where they are.
    pub folder: Option<PathBuf>,
    /// Copy instead of moving.
    pub copy: bool,
}

/// Operating systems supported by the installer. Host capability detection is
/// supplied separately; using this enum does not inspect the current machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum OperatingSystem {
    Windows,
    Linux,
    Macos,
}

/// Machine-readable explanation for an unsupported input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Reason {
    UnknownFormat,
    UnsupportedFormat {
        format: Format,
    },
    PlatformMismatch {
        payload: PayloadPlatform,
        host: OperatingSystem,
    },
    MissingRuntime {
        runtime: RuntimeSpec,
    },
    ExecutionDisabled,
    InvalidInput {
        message: String,
    },
}

/// Every analysis/execution result has an explicit, serializable outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum InstallOutcome {
    Installed(Installed),
    HandOff(HandOff),
    NeedsUser(Question),
    Unsupported(Reason),
}
