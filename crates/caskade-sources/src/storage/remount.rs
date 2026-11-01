//! Bounded remount recovery: a recorded volume-relative folder on current
//! mounts, authenticated by root.json. Never traverses an entire drive.
use super::{Result, StorageError};
use cap_std::fs::{Dir, OpenOptions};
use caskade_core::{
    model::{StorageRoot, new_id},
    storage::StorageService,
};
use caskade_discovery::HostEnv;
use caskade_installer::manifest::{ManifestPath, RootManifest};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const FILE: &str = "storage-volume-hints.json";
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Hints {
    schema_version: u32,
    roots: BTreeMap<String, PathBuf>,
}

pub(super) struct Remounts {
    registry: StorageService,
    env: HostEnv,
    config: PathBuf,
    mutation: tokio::sync::Mutex<()>,
}
impl Remounts {
    pub fn new(registry: StorageService, env: HostEnv, config: PathBuf) -> Self {
        Self {
            registry,
            env,
            config,
            mutation: Default::default(),
        }
    }
    pub async fn remember(&self, root: &StorageRoot) -> Result<()> {
        let _guard = self.mutation.lock().await;
        let (root, env, config) = (root.clone(), self.env.clone(), self.config.clone());
        caskade_jobs::blocking("Remembering storage", move || {
            let mut hints = read(&config)?;
            let volumes = env.volumes()?;
            if let Some(relative) = volumes
                .iter()
                .filter_map(|v| {
                    root.path
                        .strip_prefix(&v.root)
                        .ok()
                        .map(|p| (v.root.components().count(), p.to_path_buf()))
                })
                .max_by_key(|(depth, _)| *depth)
                .map(|(_, p)| p)
            {
                let relative = if relative.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    relative
                };
                validate_relative(&relative)?;
                hints.roots.insert(root.id, relative);
                if hints.roots.len() > 128 {
                    return Err(StorageError::Invalid("too many remount hints".into()));
                }
                write(&config, &hints)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Invalid(e.to_string()))?
    }
    pub async fn resolve(&self, root: &StorageRoot) -> Result<StorageRoot> {
        let (previous, env, config) = (root.clone(), self.env.clone(), self.config.clone());
        let candidate =
            caskade_jobs::blocking("Finding storage", move || -> Result<Option<PathBuf>> {
                if RootManifest::read(&previous.path).is_ok_and(|m| m.root_id() == previous.id) {
                    return Ok(None);
                }
                let hints = read(&config)?;
                let Some(relative) = hints.roots.get(&previous.id) else {
                    return Ok(None);
                };
                validate_relative(relative)?;
                let mut candidates = BTreeSet::new();
                let volumes = env.volumes()?;
                if volumes.len() > 128 {
                    return Err(StorageError::Invalid(
                        "too many mounted volumes for remount recovery".into(),
                    ));
                }
                for volume in volumes {
                    let path = if relative == Path::new(".") {
                        volume.root
                    } else {
                        volume.root.join(relative)
                    };
                    // Fixture confinement and canonical identity are checked before
                    // the root registrar can gain any filesystem authority.
                    let Ok(path) = env.resolve_path(&path) else {
                        continue;
                    };
                    if RootManifest::read(&path).is_ok_and(|m| m.root_id() == previous.id) {
                        candidates.insert(std::fs::canonicalize(path)?);
                    }
                }
                if candidates.len() > 1 {
                    let reason = "this root identity exists on several mounted volumes; \
                                  select the authoritative copy";
                    return Err(StorageError::Invalid(reason.into()));
                }
                Ok(candidates.into_iter().next())
            })
            .await
            .map_err(|e| StorageError::Invalid(e.to_string()))??;
        match candidate {
            None => Ok(root.clone()),
            Some(path) => self
                .registry
                .register_root(path, root.label.clone())
                .await
                .map(|s| s.root)
                .map_err(|e| StorageError::Invalid(e.to_string())),
        }
    }
}
fn validate_relative(path: &Path) -> Result<()> {
    if path == Path::new(".") {
        return Ok(());
    }
    ManifestPath::new(
        path.to_str()
            .ok_or_else(|| StorageError::Invalid("non-Unicode remount hint".into()))?,
    )?;
    Ok(())
}
fn read(config: &Path) -> Result<Hints> {
    let empty = || Hints {
        schema_version: 1,
        ..Default::default()
    };
    let dir = match Dir::open_ambient_dir(config, cap_std::ambient_authority()) {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(empty()),
        Err(e) => return Err(e.into()),
    };
    match dir.symlink_metadata(FILE) {
        Ok(m) if m.is_file() => (),
        Ok(_) => {
            return Err(StorageError::Invalid(
                "remount hints must be a regular file".into(),
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(empty()),
        Err(e) => return Err(e.into()),
    }
    let mut bytes = vec![];
    dir.open(FILE)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(StorageError::Invalid("remount hints exceed 1MiB".into()));
    }
    let hints: Hints = serde_json::from_slice(&bytes).map_err(caskade_core::Error::from)?;
    if hints.schema_version != 1 || hints.roots.len() > 128 {
        return Err(StorageError::Invalid("unsupported remount hints".into()));
    }
    for (id, path) in &hints.roots {
        RootManifest::new(id.clone())?;
        validate_relative(path)?;
    }
    Ok(hints)
}
fn write(config: &Path, hints: &Hints) -> Result<()> {
    std::fs::create_dir_all(config)?;
    let dir = Dir::open_ambient_dir(config, cap_std::ambient_authority())?;
    let temporary = format!(".storage-volume-hints-{}.tmp", new_id());
    let mut file = dir.open_with(&temporary, OpenOptions::new().create_new(true).write(true))?;
    let result = (|| -> Result<()> {
        serde_json::to_writer(&mut file, hints).map_err(caskade_core::Error::from)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        dir.rename(&temporary, &dir, FILE)?;
        #[cfg(unix)]
        dir.open(".")?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = dir.remove_file(temporary);
    }
    result
}
