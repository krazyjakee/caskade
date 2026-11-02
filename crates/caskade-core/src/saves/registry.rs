use super::{LocationKind, LocationSpec, RegistrySnapshot, SaveLocation};
use crate::{Error, Result};
use caskade_discovery::registry::{Registry, RegistryHive, RegistryKey, RegistryView};
use globset::GlobBuilder;
use std::collections::{BTreeSet, VecDeque};

pub(super) fn parse_key(path: &str) -> Option<(RegistryHive, String)> {
    let path = path.replace('/', "\\");
    let (hive, path) = path.split_once('\\')?;
    if path.is_empty() {
        return None;
    }
    let hive = match hive.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => RegistryHive::CurrentUser,
        "HKLM" | "HKEY_LOCAL_MACHINE" => RegistryHive::LocalMachine,
        _ => return None,
    };
    Some((hive, path.into()))
}

pub(super) fn resolve(
    registry: &dyn Registry,
    pattern: &str,
    tags: Vec<String>,
) -> Result<Vec<SaveLocation>> {
    let Some((hive, path)) = parse_key(pattern) else {
        return Ok(Vec::new());
    };
    let hive_name = if hive == RegistryHive::CurrentUser {
        "HKEY_CURRENT_USER"
    } else {
        "HKEY_LOCAL_MACHINE"
    };
    let mut locations = Vec::new();
    for view in [RegistryView::Registry64, RegistryView::Registry32] {
        let mut paths = vec![String::new()];
        for component in path.split('\\').filter(|part| !part.is_empty()) {
            let matcher = GlobBuilder::new(component)
                .backslash_escape(false)
                .case_insensitive(true)
                .build()
                .map_err(|error| Error::Invalid(error.to_string()))?
                .compile_matcher();
            let mut next = BTreeSet::new();
            for parent in &paths {
                let key = RegistryKey::new(hive, view, parent);
                for child in registry.subkeys(&key)? {
                    if matcher.is_match(&child) {
                        next.insert(if parent.is_empty() {
                            child
                        } else {
                            format!("{parent}\\{child}")
                        });
                    }
                }
            }
            paths = next.into_iter().collect();
        }
        for path in paths {
            let mut snapshots = Vec::new();
            let mut pending = VecDeque::from([path.clone()]);
            let mut seen = BTreeSet::new();
            while let Some(path) = pending.pop_front() {
                if !seen.insert(path.to_lowercase()) {
                    continue;
                }
                if seen.len() > 4096 {
                    return Err(Error::Invalid(crate::tr!(
                        "The save scan exceeded its registry key limit."
                    )));
                }
                let key = RegistryKey::new(hive, view, &path);
                if !registry.key_exists(&key)? {
                    continue;
                }
                snapshots.push(RegistrySnapshot {
                    key: format!("{hive_name}\\{path}"),
                    view,
                    values: registry.values(&key)?,
                });
                for child in registry.subkeys(&key)? {
                    pending.push_back(format!("{path}\\{child}"));
                }
            }
            if !snapshots.is_empty() {
                locations.push(SaveLocation {
                    spec: LocationSpec {
                        kind: LocationKind::Registry,
                        path: format!("{hive_name}\\{path}"),
                    },
                    tags: tags.clone(),
                    files: Vec::new(),
                    registry: snapshots,
                });
            }
        }
    }
    Ok(locations)
}
