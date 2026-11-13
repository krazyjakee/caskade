//! Reference scanner and relocation verdict (layer 3).
//! Finds where an install path was written (game files, registry values the
//! caller read, recorded out-of-root files) and rates how movable the game is.
//! Nothing is executed or rewritten here.
mod variants;

pub use crate::manifest::RelocationVerdict;
pub use variants::{Encoding, Needle, find, needles};

use cap_std::{ambient_authority, fs::Dir};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// Larger files are skipped and reported, never partially trusted.
    pub max_file_bytes: u64,
    pub max_files: usize,
}
impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 64 * 1024 * 1024,
            max_files: 100_000,
        }
    }
}

/// A registry value read by the caller (`winreg` on Windows, a prefix `.reg`
/// reader (I4) or an installer's recorded `[Registry]` data elsewhere), or
/// one a move writes. `hive` is `HKCU`/`HKLM`; `data` is raw bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryValue {
    pub hive: String,
    pub key: String,
    pub name: String,
    /// `REG_SZ`, `REG_EXPAND_SZ`, `REG_MULTI_SZ`, `REG_BINARY`, ...
    pub value_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanInput<'a> {
    /// Real game directory to read.
    pub game_dir: &'a Path,
    /// The path installers saw as the real location, as text (e.g.
    /// `C:\Games\Foo`; on Unix usually the game directory itself).
    pub real_path: &'a str,
    pub alias_path: Option<&'a str>,
    /// Caller-known spellings of the real path, e.g. its 8.3 short name.
    pub real_variants: &'a [String],
    pub registry: &'a [RegistryValue],
    /// Absolute paths of recorded out-of-root files (executor journal).
    pub external_files: &'a [PathBuf],
    /// Installed outside the root by a `Delegate` step.
    pub delegated: bool,
    pub limits: ScanLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    Real,
    Alias,
}

/// Where a reference was found, and whether it can be rewritten safely.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Location {
    /// Game-relative file. `rewritable` for text configs and shortcuts.
    GameFile { path: String, rewritable: bool },
    /// Recorded out-of-root file (absolute on this host).
    ExternalFile { path: String, rewritable: bool },
    Registry {
        hive: String,
        key: String,
        name: String,
        value_type: String,
        rewritable: bool,
    },
}
impl Location {
    pub fn rewritable(&self) -> bool {
        match self {
            Self::GameFile { rewritable, .. }
            | Self::ExternalFile { rewritable, .. }
            | Self::Registry { rewritable, .. } => *rewritable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Reference {
    pub location: Location,
    pub via: Via,
    pub encoding: Encoding,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanReport {
    pub verdict: RelocationVerdict,
    pub references: Vec<Reference>,
    /// Game-relative files over the size limit, never scanned.
    pub skipped: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("game tree exceeds {0} files")]
    TooManyFiles(usize),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Text formats a move job can rewrite transactionally (§7.1).
const TEXT_EXTENSIONS: &[&str] = &[
    "ini",
    "cfg",
    "conf",
    "config",
    "json",
    "xml",
    "txt",
    "toml",
    "yaml",
    "yml",
    "properties",
    "desktop",
    "reg",
    "vdf",
    "acf",
    "lua",
    "bat",
    "cmd",
    "sh",
    "log",
    "info",
    "manifest",
    "plist",
];

fn rewritable_file(path: &Path, bytes: &[u8]) -> bool {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("lnk") => true,
        Some(ext) if TEXT_EXTENSIONS.contains(&ext) => true,
        // Unknown extension: text only if UTF-8 without NULs.
        _ => !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok(),
    }
}

/// Scan for real-path and alias references and compute the verdict.
pub fn scan_references(input: &ScanInput<'_>) -> Result<ScanReport, ScanError> {
    let real = needles(input.real_path, input.real_variants);
    let alias = input
        .alias_path
        .map(|alias| needles(alias, &[]))
        .unwrap_or_default();
    let mut report = ScanReport::default();
    let mut record = |location: &dyn Fn() -> Location, bytes: &[u8]| {
        for (via, set) in [(Via::Real, &real), (Via::Alias, &alias)] {
            if let Some(needle) = set.iter().find(|needle| find(bytes, needle).is_some()) {
                report.references.push(Reference {
                    location: location(),
                    via,
                    encoding: needle.encoding,
                });
            }
        }
    };

    let root = Dir::open_ambient_dir(input.game_dir, ambient_authority())?;
    let mut pending = vec![PathBuf::new()];
    let mut files = 0usize;
    let mut skipped = vec![];
    while let Some(relative) = pending.pop() {
        let directory = if relative.as_os_str().is_empty() {
            root.try_clone()?
        } else {
            root.open_dir(&relative)?
        };
        for entry in directory.entries()? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            // Caskade's own metadata is not the game's.
            if path.starts_with(".caskade") {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(path);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            files += 1;
            if files > input.limits.max_files {
                return Err(ScanError::TooManyFiles(input.limits.max_files));
            }
            let text = path.to_string_lossy().replace('\\', "/");
            if entry.metadata()?.len() > input.limits.max_file_bytes {
                skipped.push(text);
                continue;
            }
            let mut bytes = vec![];
            entry.open()?.read_to_end(&mut bytes)?;
            let rewritable = rewritable_file(&path, &bytes);
            record(
                &|| Location::GameFile {
                    path: text.clone(),
                    rewritable,
                },
                &bytes,
            );
        }
    }
    for value in input.registry {
        let rewritable = matches!(
            value.value_type.as_str(),
            "REG_SZ" | "REG_EXPAND_SZ" | "REG_MULTI_SZ"
        );
        record(
            &|| Location::Registry {
                hive: value.hive.clone(),
                key: value.key.clone(),
                name: value.name.clone(),
                value_type: value.value_type.clone(),
                rewritable,
            },
            &value.data,
        );
    }
    for path in input.external_files {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > input.limits.max_file_bytes {
            continue;
        }
        let bytes = std::fs::read(path)?;
        let rewritable = rewritable_file(path, &bytes);
        record(
            &|| Location::ExternalFile {
                path: path.to_string_lossy().into_owned(),
                rewritable,
            },
            &bytes,
        );
    }
    report.references.sort();
    report.skipped = skipped;
    report.verdict = verdict(input.delegated, &report.references);
    Ok(report)
}

/// §7.1: Pinned for delegated installs; with real-path references,
/// Rewritable when every one is in a rewritable place, otherwise Fragile;
/// without them, Aliased when the alias is referenced, otherwise Free.
pub fn verdict(delegated: bool, references: &[Reference]) -> RelocationVerdict {
    if delegated {
        return RelocationVerdict::Pinned;
    }
    let mut real = references.iter().filter(|r| r.via == Via::Real).peekable();
    if real.peek().is_some() {
        if real.all(|reference| reference.location.rewritable()) {
            RelocationVerdict::Rewritable
        } else {
            RelocationVerdict::Fragile
        }
    } else if references.iter().any(|r| r.via == Via::Alias) {
        RelocationVerdict::Aliased
    } else {
        RelocationVerdict::Free
    }
}
