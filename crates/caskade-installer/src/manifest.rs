//! Versioned installation records in `<game>/.caskade/install.json` and storage
//! identity in `<root>/.caskade/root.json`.
//!
//! Filesystem fields are portable relative paths: readers accept either slash
//! style, writers emit `/`, and absolute/traversing paths are rejected on every
//! host. Shared plan/launch/runtime types are reused, with the same validation
//! applied to their nested paths. Source paths are logical, input-relative names;
//! [`ManifestSource::from_input`] reduces an absolute picked source to its name.
//! URLs retain their complete provenance. Opaque command arguments, registry
//! values and artifact facts remain data, not filesystem capabilities.
//!
//! Deserialize through serde or [`InstallManifest::from_json`] to migrate older
//! documents. Unknown future versions fail without rewriting their files. The
//! immutable validated wrapper keeps invalid records from being written later.

mod effects;
mod migration;
pub mod native;
mod paths;
mod storage;
mod uninstall;
mod validation;
mod verify;

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

use crate::{HandlerId, Input, InstallPlan, LaunchTarget, RuntimeSpec, Step};

pub use effects::{ExternalEffect, ExternalLocation, ExternalRoots, effects_from_journal};
pub use paths::{ManifestPath, Sha256Digest};
pub use uninstall::{
    AliasRemover, UninstallError, UninstallFailure, UninstallFailureReason, UninstallOperation,
    UninstallReport, UninstallRequest, Uninstaller,
};
pub use verify::{ChangedFile, FileChange, VerifyReport, verify};

pub const INSTALL_SCHEMA_VERSION: u32 = 1;
pub const ROOT_SCHEMA_VERSION: u32 = 1;
pub const INSTALL_MANIFEST_PATH: &str = ".caskade/install.json";
pub const ROOT_MANIFEST_PATH: &str = ".caskade/root.json";

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("invalid manifest: {0}")]
    Invalid(String),
    #[error("unsupported {document} schema version {version}; supported through {supported}")]
    UnsupportedVersion {
        document: &'static str,
        version: u64,
        supported: u32,
    },
    #[error("manifest JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("manifest filesystem operation: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ManifestError>;

/// Provenance. Local names are input-relative, never an absolute source/game
/// location; keeping a checksum identifies a source after its directory moves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSource {
    pub input: Input,
    pub final_url: Option<Url>,
    pub checksum: Option<Sha256Digest>,
}

impl ManifestSource {
    /// Snapshot a selected input for portable storage. Full URLs are retained;
    /// an absolute local selection becomes its last component on either OS.
    pub fn from_input(
        input: &Input,
        final_url: Option<Url>,
        checksum: Option<Sha256Digest>,
    ) -> Result<Self> {
        let mut input = input.clone();
        if let Input::Path(path) = &mut input {
            let name = paths::path_text(path)?;
            let is_absolute =
                name.starts_with(['/', '\\']) || name.as_bytes().get(1) == Some(&b':');
            if is_absolute {
                let name = name
                    .rsplit(['/', '\\'])
                    .next()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        ManifestError::Invalid("source selection has no file name".into())
                    })?;
                *path = name.into();
            }
            paths::normalize_path(path, false)?;
        }
        validation::input(&mut input)?;
        if final_url
            .as_ref()
            .is_some_and(|url| !matches!(url.scheme(), "http" | "https"))
        {
            return Err(ManifestError::Invalid(
                "final URL must use HTTP or HTTPS".into(),
            ));
        }
        Ok(Self {
            input,
            final_url,
            checksum,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlerVersion {
    pub id: HandlerId,
    pub version: String,
}

/// A regular installed file. Directory/symlink topology belongs to the executor
/// journal; verify does not follow a leaf symlink masquerading as this file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledFile {
    pub path: ManifestPath,
    pub size: u64,
    pub sha256: Option<Sha256Digest>,
}

/// Placeholder extended by the reference scanner/move job in later phases.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelocationRecord {
    pub verdict: RelocationVerdict,
    /// Evidence for the verdict from [`crate::scan::scan_references`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<crate::scan::Reference>,
    /// Host spelling of the game's absolute path when last scanned or moved:
    /// the anchor reconcile (#61) rewrites from. Provenance metadata only,
    /// the one exception to game-relative paths; never a writable capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelocationVerdict {
    /// No reference scan has run; do not claim the installation is free to move.
    #[default]
    Unscanned,
    Free,
    Aliased,
    Rewritable,
    Fragile,
    Pinned,
}

/// Mutable construction data. Pass this through [`InstallManifest::new`] after
/// changes to validate every typed path and metadata invariant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    pub title: String,
    #[serde(default)]
    pub ids: BTreeMap<String, String>,
    pub source: ManifestSource,
    pub plan: InstallPlan,
    pub handlers: Vec<HandlerVersion>,
    pub files: Vec<InstalledFile>,
    pub launch_targets: Vec<LaunchTarget>,
    pub runtime: RuntimeSpec,
    pub uninstall_steps: Vec<Step>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_effects: Vec<ExternalEffect>,
    pub relocation: RelocationRecord,
    /// Stable alias ID (#31) the game is reachable through; the alias location
    /// is resolved per host and never stored as an absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

/// Validated schema-v1 manifest. Serialization and deserialization both use a
/// flat document with `schema_version`; construction data is accessed via record.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallManifest {
    record: InstallRecord,
}

impl InstallManifest {
    /// The executed plan's input is replaced with the portable source snapshot;
    /// the live execution request can retain its original absolute source path.
    pub fn new(mut record: InstallRecord) -> Result<Self> {
        validation::record(&mut record)?;
        Ok(Self { record })
    }
    pub const fn schema_version(&self) -> u32 {
        INSTALL_SCHEMA_VERSION
    }
    pub fn record(&self) -> &InstallRecord {
        &self.record
    }
    pub fn into_record(self) -> InstallRecord {
        self.record
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        Self::from_value(serde_json::from_slice(bytes)?)
    }
    fn from_value(value: serde_json::Value) -> Result<Self> {
        let value = migration::install(value)?;
        Self::new(serde_json::from_value(value)?)
    }
    pub fn read(game_dir: &Path) -> Result<Self> {
        Self::from_json(&storage::read(game_dir, "install.json")?)
    }
    /// Replace the file atomically inside .caskade; validation happens before I/O.
    pub fn write(&self, game_dir: &Path) -> Result<()> {
        storage::write(game_dir, "install.json", &serde_json::to_vec_pretty(self)?)
    }
    pub fn verify(&self, game_dir: &Path) -> Result<VerifyReport> {
        verify(game_dir, self)
    }
}

impl Serialize for InstallManifest {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut value = serde_json::to_value(&self.record).map_err(serde::ser::Error::custom)?;
        value
            .as_object_mut()
            .expect("record serializes as object")
            .insert("schema_version".into(), INSTALL_SCHEMA_VERSION.into());
        value.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for InstallManifest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Self::from_value(serde_json::Value::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Portable root identity. Resolve the actual mount point outside this record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootManifest {
    root_id: String,
}

impl RootManifest {
    pub fn new(root_id: impl Into<String>) -> Result<Self> {
        let root_id = root_id.into();
        validation::identifier(&root_id, "root id")?;
        Ok(Self { root_id })
    }
    pub const fn schema_version(&self) -> u32 {
        ROOT_SCHEMA_VERSION
    }
    pub fn root_id(&self) -> &str {
        &self.root_id
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        Self::from_value(serde_json::from_slice(bytes)?)
    }
    fn from_value(value: serde_json::Value) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            root_id: String,
        }
        let record: Record = serde_json::from_value(migration::root(value)?)?;
        Self::new(record.root_id)
    }
    pub fn read(root_dir: &Path) -> Result<Self> {
        Self::from_json(&storage::read(root_dir, "root.json")?)
    }
    pub fn write(&self, root_dir: &Path) -> Result<()> {
        storage::write(root_dir, "root.json", &serde_json::to_vec_pretty(self)?)
    }
}
impl Serialize for RootManifest {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serde_json::json!({ "schema_version": ROOT_SCHEMA_VERSION, "root_id": self.root_id })
            .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for RootManifest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Self::from_value(serde_json::Value::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}
