//! Redistributables: VC++, DirectX, .NET, PhysX,
//! OpenAL, XNA. Detection reads file names in the installed tree (also under
//! `__redist/` and `_CommonRedist/`), installer `[Run]` targets and PE
//! ProductNames, matched against `data/redists.toml`. On Windows each missing
//! one becomes a silent, prompt-free run of the shipped installer; inside a
//! Proton/Wine prefix it is recorded with its winetricks verbs (applied in I4).
use crate::{Dependency, OperatingSystem, Program, RuntimeSpec, SourcePath, Step, host::HostCaps};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedistDefinition {
    pub id: String,
    pub name: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub product_names: Vec<String>,
    pub silent: Vec<String>,
    pub installed: InstalledCheck,
    pub winetricks: Vec<String>,
}

/// Evidence that a runtime is already present on a Windows host.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum InstalledCheck {
    Registry {
        hive: String,
        key: String,
        value: String,
    },
    /// Path relative to the Windows directory.
    File { file: String },
}

#[derive(Debug, Clone)]
pub struct RedistTable {
    pub redists: Vec<RedistDefinition>,
}

impl Default for RedistTable {
    fn default() -> Self {
        #[derive(Deserialize)]
        struct Table {
            redist: Vec<RedistDefinition>,
        }
        let table: Table = toml::from_str(include_str!("../data/redists.toml"))
            .expect("embedded redistributable table is valid");
        Self {
            redists: table.redist,
        }
    }
}

/// Folders conventionally holding redistributables, matched case-insensitively.
const REDIST_DIRS: &[&str] = &["__redist", "_commonredist", "redist", "redistributables"];

/// Executable or MSI inside a redistributable folder, or matching a table
/// file name anywhere.
fn candidate_file(path: &Path) -> bool {
    let in_redist_dir = path.components().any(|component| {
        REDIST_DIRS.contains(
            &component
                .as_os_str()
                .to_string_lossy()
                .to_lowercase()
                .as_str(),
        )
    });
    let installer = path.extension().is_some_and(|ext| {
        let ext = ext.to_string_lossy().to_lowercase();
        ext == "exe" || ext == "msi"
    });
    installer && in_redist_dir
}

/// `*` and `?` glob over ASCII-lowercased names.
fn glob(pattern: &str, name: &str) -> bool {
    fn matches(pattern: &[u8], name: &[u8]) -> bool {
        match (pattern.first(), name.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                matches(&pattern[1..], name) || (!name.is_empty() && matches(pattern, &name[1..]))
            }
            (Some(b'?'), Some(_)) => matches(&pattern[1..], &name[1..]),
            (Some(a), Some(b)) if a == b => matches(&pattern[1..], &name[1..]),
            _ => false,
        }
    }
    matches(pattern.as_bytes(), name.to_ascii_lowercase().as_bytes())
}

/// A redistributable found in the game, before host-specific decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub definition: RedistDefinition,
    /// Game-relative installer path.
    pub path: PathBuf,
    pub reason: &'static str,
}

impl RedistTable {
    fn by_name(&self, path: &Path) -> Option<&RedistDefinition> {
        let name = path.file_name()?.to_string_lossy();
        self.redists
            .iter()
            .find(|definition| definition.files.iter().any(|pattern| glob(pattern, &name)))
    }

    fn by_product(&self, product: &str) -> Option<&RedistDefinition> {
        let product = product.to_lowercase();
        self.redists.iter().find(|definition| {
            definition
                .product_names
                .iter()
                .any(|prefix| product.starts_with(&prefix.to_lowercase()))
        })
    }

    /// Match game-relative paths (installed tree or installer file list) and
    /// installer `[Run]` targets. `product_name` reads a PE/MSI ProductName
    /// for unrecognised executables in redistributable folders.
    pub fn detect<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a Path>,
        run_targets: impl IntoIterator<Item = &'a Path>,
        product_name: &dyn Fn(&Path) -> Option<String>,
    ) -> Vec<Found> {
        let mut found: BTreeMap<String, Found> = BTreeMap::new();
        let mut add = |definition: &RedistDefinition, path: &Path, reason| {
            found.entry(definition.id.clone()).or_insert_with(|| Found {
                definition: definition.clone(),
                path: path.to_path_buf(),
                reason,
            });
        };
        for path in run_targets {
            if let Some(definition) = self.by_name(path) {
                add(definition, path, "installer [Run] entry");
            }
        }
        for path in paths {
            if let Some(definition) = self.by_name(path) {
                add(definition, path, "file name");
            } else if candidate_file(path)
                && let Some(definition) = product_name(path)
                    .as_deref()
                    .and_then(|product| self.by_product(product))
            {
                add(definition, path, "product name");
            }
        }
        found.into_values().collect()
    }
}

/// Host decision for each redistributable. On Windows: already installed per
/// `installed`, else a silent run of the shipped installer (never elevated by
/// Caskade; Windows owns any UAC prompt). In a prefix: winetricks verbs only.
pub fn dependencies(
    found: &[Found],
    host: &HostCaps,
    runtime: &RuntimeSpec,
    installed: &dyn Fn(&InstalledCheck) -> bool,
) -> Vec<Dependency> {
    found
        .iter()
        .map(|item| {
            let definition = &item.definition;
            let native_windows =
                host.os == OperatingSystem::Windows && *runtime == RuntimeSpec::Native;
            let already = native_windows && installed(&definition.installed);
            let install_steps = if native_windows && !already {
                vec![Step::Run {
                    program: Program::Source(SourcePath::Game(item.path.clone())),
                    args: definition.silent.clone(),
                    env: BTreeMap::new(),
                    runtime: RuntimeSpec::Native,
                    timeout_secs: 1800,
                    expect: vec![],
                }]
            } else {
                vec![]
            };
            Dependency {
                id: definition.id.clone(),
                name: definition.name.clone(),
                installed: already,
                install_steps,
                uninstall_steps: vec![],
                runtime_verbs: definition.winetricks.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_loads_and_globs_match_case_insensitively() {
        let table = RedistTable::default();
        assert!(table.redists.len() >= 8);
        assert!(glob(
            "vcredist_2019*_x64.exe",
            "VCRedist_2019_14.29_x64.EXE"
        ));
        assert!(!glob("vcredist_x64.exe", "vcredist_x86.exe"));
        assert!(glob("dxsetup.exe", "DXSETUP.exe"));
    }
}
