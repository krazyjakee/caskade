use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::ArtifactId;

/// The runtime to execute under. Prefix IDs are resolved by the runtime layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuntimeSpec {
    Native,
    ProtonPrefix { id: String },
    WinePrefix { id: String },
}

/// A source is either an input artifact or a path in the staging/game tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SourcePath {
    /// An entry relative to an identified source artifact (`.` means its root).
    Artifact {
        artifact: ArtifactId,
        path: PathBuf,
    },
    Game(PathBuf),
}

/// A program from the input, the game tree or a host tool resolved by runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Program {
    Source(SourcePath),
    Tool(String),
}

/// A registry value to replay into a Proton/Wine prefix, never the host registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RegistryEntry {
    pub hive: RegistryHive,
    pub key: String,
    /// Empty for the key's default value.
    pub name: String,
    pub value: RegistryValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum RegistryHive {
    CurrentUser,
    LocalMachine,
    ClassesRoot,
    Users,
    CurrentConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RegistryValue {
    String(String),
    ExpandString(String),
    MultiString(Vec<String>),
    Dword(u32),
    Qword(u64),
    Binary(Vec<u8>),
}

/// Fixed plan DSL: analysis describes operations; only the executor acts on them.
///
/// All `to`, `expect`, `target_dir`, `path` and `mount_point` paths are relative
/// to staging/the final game directory. Symlink targets are relative to the
/// link's parent and must resolve inside the game tree. The executor validates
/// these invariants through the safety layer before performing any operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Step {
    Extract {
        from: ArtifactId,
        /// Include globs; empty selects every entry.
        select: Vec<String>,
        /// Optional prefix removed before writing selected entries.
        strip: Option<PathBuf>,
        to: PathBuf,
    },
    Copy {
        from: SourcePath,
        to: PathBuf,
    },
    Move {
        from: SourcePath,
        to: PathBuf,
    },
    Run {
        program: Program,
        /// Argument vector; never interpreted by a shell.
        args: Vec<String>,
        env: BTreeMap<String, String>,
        runtime: RuntimeSpec,
        /// Wall-clock limit in seconds; must be non-zero before execution.
        timeout_secs: u64,
        expect: Vec<PathBuf>,
    },
    RunInteractive {
        program: Program,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        runtime: RuntimeSpec,
        target_dir: PathBuf,
    },
    Mount {
        from: ArtifactId,
        mount_point: PathBuf,
    },
    Unmount {
        mount_point: PathBuf,
    },
    SetExecutable {
        path: PathBuf,
    },
    Symlink {
        path: PathBuf,
        target: PathBuf,
    },
    WriteFile {
        path: PathBuf,
        contents: Vec<u8>,
    },
    /// Writes `length` bytes of a file artifact, from `offset`, to `path`:
    /// how a plugin `install-handler` unpacks a container format it knows
    /// without writing files itself (#55).
    CopyRange {
        from: ArtifactId,
        offset: u64,
        length: u64,
        path: PathBuf,
    },
    RegistryImport {
        /// Execution rejects Native; registry replay only targets prefixes.
        runtime: RuntimeSpec,
        entries: Vec<RegistryEntry>,
    },
    Delegate {
        tool: String,
        args: Vec<String>,
        /// Required compensation recorded for uninstall and rollback.
        uninstall: Vec<DelegateCommand>,
        /// Runs first; the step is skipped unless it exits 0.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        only_if: Option<DelegateCheck>,
    },
}

/// A recorded host-tool command that reverses an out-of-root installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct DelegateCommand {
    pub tool: String,
    pub args: Vec<String>,
    /// Runs first; the command is skipped unless it exits 0, so a package
    /// removed by hand needs no removal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_if: Option<DelegateCheck>,
}

/// A host-tool query that exits 0 while a delegate command has something to
/// do, such as the package manager's "is this package installed".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct DelegateCheck {
    pub tool: String,
    pub args: Vec<String>,
}
