//! Portable snapshots of out-of-root file effects. Scope locations are resolved
//! by the caller on this host, never persisted as absolute filesystem paths.
use super::{ManifestError, ManifestPath, Result};
use crate::exec::JournalEntry;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Logical OS location (e.g. `user.desktop`) plus a confined relative path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalLocation {
    pub scope: String,
    pub path: ManifestPath,
}

/// Replay in reverse order on uninstall. A file with no original was created by
/// this install; otherwise restore the previous bytes instead of deleting user
/// data. Directories are removed only when empty, never recursively.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExternalEffect {
    File {
        location: ExternalLocation,
        original: Option<Vec<u8>>,
    },
    Directory {
        location: ExternalLocation,
    },
    /// Typed inverse of an installer-owned registry value or newly created key.
    Registry {
        effect: super::native::RegistryEffect,
    },
}

impl ExternalEffect {
    pub fn location(&self) -> &ExternalLocation {
        match self {
            Self::File { location, .. } | Self::Directory { location } => location,
            Self::Registry { effect } => effect.location(),
        }
    }
}

/// Bind logical scopes to current host directories at the app/runtime boundary.
/// These absolute paths are not serialized into an install manifest.
pub type ExternalRoots = BTreeMap<String, PathBuf>;

/// Convert retained executor filesystem effects into portable manifest records.
/// Choose the most specific supplied root for nested scopes. Compensation
/// commands are intentionally not file effects: handlers must express those as
/// relative `uninstall_steps`, using the existing Step DSL and runtime IDs.
pub fn effects_from_journal(
    journal: &[JournalEntry],
    roots: &ExternalRoots,
) -> Result<Vec<ExternalEffect>> {
    let mut canonical = Vec::new();
    for (id, root) in roots {
        super::validation::identifier(id, "external scope")?;
        canonical.push((id, std::fs::canonicalize(root)?));
    }
    canonical.sort_by_key(|(_, root)| std::cmp::Reverse(root.components().count()));
    let locate = |path: &Path| -> Result<ExternalLocation> {
        if !path.is_absolute() {
            return Err(ManifestError::Invalid(
                "executor external path must be absolute".into(),
            ));
        }
        for (scope, root) in &canonical {
            if let Ok(relative) = path.strip_prefix(root) {
                let path = ManifestPath::new(super::paths::path_text(relative)?)?;
                return Ok(ExternalLocation {
                    scope: (*scope).clone(),
                    path,
                });
            }
        }
        Err(ManifestError::Invalid(format!(
            "no external scope covers recorded path {}",
            path.display()
        )))
    };
    journal
        .iter()
        .filter_map(|entry| match entry {
            JournalEntry::ExternalFile { path, original } => {
                Some(locate(path).map(|location| ExternalEffect::File {
                    location,
                    original: original.clone(),
                }))
            }
            JournalEntry::ExternalDirectory { path } => {
                Some(locate(path).map(|location| ExternalEffect::Directory { location }))
            }
            JournalEntry::Compensation { .. } | JournalEntry::InstallLocation { .. } => None,
        })
        .collect()
}
