use super::{Classification, Metadata, classify_inventory};
use crate::{ClassifierConfig, Error, HostEnv, LaunchTarget};
use caskade_installer::{
    discover::{DiscoveryConfig, DiscoveryHints, discover_with_config},
    host::HostCaps,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Inspect a bounded tree without following symlinks or writing icons. The
/// caller supplies the host capabilities used by installer launch discovery;
/// fixture callers inject a profile, while CLI may detect at its boundary.
pub fn classify_path(
    env: &HostEnv,
    host: &HostCaps,
    path: &Path,
    metadata: &Metadata,
    config: &ClassifierConfig,
) -> crate::Result<Classification> {
    super::rules::validate(config)?;
    let path = env.resolve_path(path)?;
    let entry = fs::symlink_metadata(&path)?;
    if entry.file_type().is_symlink() {
        return Err(Error::Usage("classifier root may not be a symlink".into()));
    }
    let (root, requested_exe) = if entry.is_file() {
        (
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
            Some(path.clone()),
        )
    } else if entry.is_dir() {
        (path.clone(), None)
    } else {
        return Err(Error::Usage(
            "classifier needs a directory or regular file".into(),
        ));
    };
    let root = dunce::canonicalize(&root)?;
    // Canonicalize through HostEnv too, so a fixture cannot escape its tree.
    let root = env.resolve_path(&root)?;
    let directory = cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
    let mut files = Vec::new();
    let mut visited: u64 = 0;
    let mut stack = vec![(PathBuf::new(), 0usize)];
    let mut warnings = Vec::new();
    while let Some((dir, depth)) = stack.pop() {
        let remaining = config
            .max_entries
            .saturating_sub(visited)
            .min(usize::MAX as u64) as usize;
        let entries = if dir.as_os_str().is_empty() {
            directory.entries()?
        } else {
            directory.read_dir(&dir)?
        };
        let mut entries = entries
            .take(remaining.saturating_add(1))
            .collect::<Result<Vec<_>, _>>()?;
        if entries.len() > remaining {
            return Err(Error::Usage("classifier tree exceeds entry budget".into()));
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if visited >= config.max_entries {
                return Err(Error::Usage("classifier tree exceeds entry budget".into()));
            }
            visited += 1;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() || (!file_type.is_file() && !file_type.is_dir()) {
                continue;
            }
            let relative = dir.join(entry.file_name());
            let Some(text) = relative.to_str() else {
                warnings.push("skipped non-Unicode fingerprint path".into());
                continue;
            };
            files.push(text.replace('\\', "/"));
            if file_type.is_dir() {
                if depth < config.max_depth as usize {
                    stack.push((relative, depth + 1));
                } else {
                    warnings.push("classifier depth budget reached".into());
                }
            }
        }
    }
    let mut metadata = metadata.clone();
    if metadata.install_dir.is_none() {
        metadata.install_dir = Some(root.clone());
    }
    if metadata.executable.is_none() {
        metadata.executable = requested_exe;
    }
    if metadata.title.is_none() {
        metadata.title = root.file_name().and_then(|s| s.to_str()).map(str::to_owned);
    }
    let mut classification = classify_inventory(&metadata, &files, config)?;
    classification.warnings.extend(warnings);
    match discover_with_config(
        &root,
        host,
        &DiscoveryHints {
            installer_title: metadata.title,
            ..Default::default()
        },
        DiscoveryConfig {
            max_entries: config.max_entries,
            write_icon: false,
            ..Default::default()
        },
    ) {
        Ok(discovery) => {
            if let Some(target) = discovery.launch_targets.into_iter().next() {
                // Installer targets are relative to the scanned root.
                classification.launch = Some(LaunchTarget::Exec {
                    program: root.join(target.path),
                    args: target.args,
                    cwd: Some(root.join(target.working_dir)),
                    env: target.env,
                });
            }
            classification.warnings.extend(
                discovery
                    .warnings
                    .into_iter()
                    .map(|warning| warning.message),
            );
            if discovery.question.is_some() {
                classification.warnings.push(
                    "multiple plausible launch targets; highest ranked target returned".into(),
                );
            }
        }
        Err(error) => classification
            .warnings
            .push(format!("launch discovery: {error}")),
    }
    Ok(classification)
}
