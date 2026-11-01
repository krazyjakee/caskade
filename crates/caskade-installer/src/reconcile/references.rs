use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    manifest::ManifestPath,
    scan::{Location, Reference, Via},
};

use super::{ReconcileError, Reconciler, Result};

impl Reconciler {
    pub(super) fn rescan(
        &self,
        game: &Path,
        old: &str,
        manifest: &crate::manifest::InstallManifest,
        repair: &crate::relocate::Relocator,
    ) -> Result<(
        Vec<Reference>,
        crate::manifest::RelocationVerdict,
        Vec<String>,
    )> {
        use crate::{
            Strategy,
            manifest::RelocationVerdict,
            scan::{RegistryValue, ScanInput},
        };

        let record = manifest.record();
        let allowed = self.writable_references(game, &record.relocation.references)?;
        let external_files: Vec<_> = allowed
            .iter()
            .filter_map(|reference| match &reference.location {
                Location::ExternalFile { path, .. } => Some(path.into()),
                _ => None,
            })
            .collect();
        let mut registry = vec![];
        for reference in &record.relocation.references {
            if let Location::Registry {
                hive, key, name, ..
            } = &reference.location
                && let Some(value) = repair.registry.read(hive, key, name)?
            {
                registry.push(RegistryValue {
                    hive: value.hive,
                    key: value.key,
                    name: value.name,
                    value_type: value.value_type,
                    data: value.data,
                });
            }
        }
        let alias = match (&self.aliases, &record.alias) {
            (Some(store), Some(id)) => Some(
                store
                    .path(id)
                    .map_err(std::io::Error::other)?
                    .to_string_lossy()
                    .into_owned(),
            ),
            _ => None,
        };
        let scan = crate::scan::scan_references(&ScanInput {
            game_dir: game,
            real_path: old,
            alias_path: alias.as_deref(),
            real_variants: &[],
            registry: &registry,
            external_files: &external_files,
            delegated: false,
            limits: Default::default(),
        })?;
        let mut references = record.relocation.references.clone();
        references.extend(scan.references);
        references.sort();
        references.dedup();
        // Retain evidence and an uninstall-safe verdict even after a rewrite
        // removes the strings that originally made the installation fragile.
        let found = crate::scan::verdict(false, &references);
        let verdict = if record.relocation.verdict == RelocationVerdict::Fragile
            || found == RelocationVerdict::Fragile
            || !scan.skipped.is_empty()
            || record.relocation.verdict == RelocationVerdict::Unscanned
                && matches!(
                    record.plan.candidate.strategy,
                    Strategy::RunSilent | Strategy::RunInteractive
                ) {
            RelocationVerdict::Fragile
        } else if record.relocation.verdict == RelocationVerdict::Rewritable
            || found == RelocationVerdict::Rewritable
        {
            RelocationVerdict::Rewritable
        } else if record.alias.is_some() || found == RelocationVerdict::Aliased {
            RelocationVerdict::Aliased
        } else {
            RelocationVerdict::Free
        };
        self.writable_references(game, &references)?;
        Ok((references, verdict, scan.skipped))
    }

    pub(super) fn writable_references(
        &self,
        game: &Path,
        references: &[Reference],
    ) -> Result<Vec<Reference>> {
        let mut writable = vec![];
        for reference in references {
            if reference.via != Via::Real || !reference.location.rewritable() {
                continue;
            }
            match &reference.location {
                Location::GameFile { path, .. } => {
                    let relative = ManifestPath::new(path)?;
                    if relative
                        .as_path()
                        .components()
                        .next()
                        .is_some_and(|component| metadata_component(component.as_os_str()))
                    {
                        return Err(ReconcileError::Invalid(
                            "a game reference cannot rewrite Caskade metadata".into(),
                        ));
                    }
                    if !confined_file(game, relative.as_path())? {
                        continue;
                    }
                }
                Location::ExternalFile { path, .. } => {
                    let path = Path::new(path);
                    if path
                        .components()
                        .any(|component| metadata_component(component.as_os_str()))
                    {
                        return Err(ReconcileError::Invalid(
                            "an external reference cannot rewrite Caskade metadata".into(),
                        ));
                    }
                    if !path.is_absolute() {
                        return Err(ReconcileError::Invalid(
                            "external reference must be absolute".into(),
                        ));
                    }
                    let mut allowed = false;
                    for root in &self.external_roots {
                        let root = fs::canonicalize(root)?;
                        if let Some(relative) = external_relative(path, &root)
                            && !relative.as_os_str().is_empty()
                            && confined_file(&root, &relative)?
                        {
                            allowed = true;
                            break;
                        }
                    }
                    if !allowed {
                        return Err(ReconcileError::Invalid(format!(
                            "external reference is outside configured roots: {}",
                            path.display()
                        )));
                    }
                }
                // The injected RegistryStore controls the host/prefix scope.
                Location::Registry { .. } => {}
            }
            writable.push(reference.clone());
        }
        Ok(writable)
    }
}

/// Reject every symlink component. In particular a leaf symlink must never turn
/// a relative manifest reference into an ambient external write.
pub(super) fn confined_file(root: &Path, relative: &Path) -> Result<bool> {
    let relative = ManifestPath::new(
        relative
            .to_str()
            .ok_or_else(|| ReconcileError::Invalid("rewrite path is not UTF-8".into()))?,
    )?;
    let mut path = root.to_path_buf();
    for component in relative.as_path().components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ReconcileError::Invalid(format!(
                    "symlink in rewrite path: {}",
                    path.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        }
    }
    if !path.is_file() || !fs::canonicalize(&path)?.starts_with(root) {
        return Err(ReconcileError::Invalid(
            "rewrite path escapes its root".into(),
        ));
    }
    Ok(true)
}

fn metadata_component(component: &std::ffi::OsStr) -> bool {
    if cfg!(windows) {
        component
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(".caskade"))
    } else {
        component == ".caskade"
    }
}

// canonicalize() uses the extended Windows prefix; watcher references may use
// the ordinary spelling. Compare their components without weakening the later
// component-by-component symlink checks against the canonical root.
#[cfg(windows)]
fn external_relative(path: &Path, root: &Path) -> Option<PathBuf> {
    fn ordinary(path: &Path) -> Option<PathBuf> {
        let text = path.to_str()?;
        Some(if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{unc}"))
        } else {
            PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text))
        })
    }
    let path = ordinary(path)?;
    let root = ordinary(root)?;
    let mut components = path.components();
    for expected in root.components() {
        if !components
            .next()?
            .as_os_str()
            .to_str()?
            .eq_ignore_ascii_case(expected.as_os_str().to_str()?)
        {
            return None;
        }
    }
    Some(components.collect())
}
#[cfg(not(windows))]
fn external_relative(path: &Path, root: &Path) -> Option<PathBuf> {
    path.strip_prefix(root).ok().map(Path::to_path_buf)
}
