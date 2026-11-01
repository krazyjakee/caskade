//! Library folders and `appmanifest_*.acf` records.

use super::{
    super::{is_file, join, list},
    vdf,
};
use crate::{
    env::HostEnv,
    parsers::binary_vdf::Value,
    types::{InstallState, ScanCtx, SourceError},
};
use std::path::{Path, PathBuf};

/// Every library a root lists, the root's own first. Old (`"1" "D:\\Lib"`)
/// and current (`"1" { "path" … }`) layouts are both read.
pub(super) fn libraries(env: &HostEnv, root: &Path, ctx: &ScanCtx) -> Vec<PathBuf> {
    let mut libraries = vec![root.to_path_buf()];
    let file = ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"]
        .into_iter()
        .map(|relative| join(root, relative))
        .find(|path| is_file(env, path));
    let Some(file) = file else {
        return libraries;
    };
    match vdf::load(env, &file, &vdf::small()) {
        Ok((value, warnings)) => {
            warnings.into_iter().for_each(|warning| ctx.warn(warning));
            let folders = vdf::entries(&value).first().map(|(_, body)| body);
            for (key, folder) in folders.map(vdf::entries).unwrap_or_default() {
                if key.parse::<u32>().is_err() {
                    continue;
                }
                let path = match folder {
                    Value::String(path) => Some(path.as_str()),
                    folder => vdf::text(folder, "path"),
                };
                match path.filter(|path| !path.trim().is_empty()) {
                    Some(path) => match super::super::resolve(env, path) {
                        Some(path) => libraries.push(path),
                        None => ctx.warn(format!("library path {path:?} is not readable")),
                    },
                    None => ctx.warn(format!("library {key} has no path")),
                }
            }
        }
        Err(error) => ctx.warn(error.to_string()),
    }
    libraries
}

/// One parsed `appmanifest_<id>.acf`.
#[derive(Debug, Clone)]
pub(super) struct Manifest {
    pub file: PathBuf,
    pub app_id: u32,
    pub name: Option<String>,
    pub install_dir: Option<String>,
    pub flags: u32,
    pub size: Option<u64>,
    pub last_played: Option<u64>,
}

/// The manifests of one library, by app id.
pub(super) fn manifests(
    env: &HostEnv,
    library: &Path,
    ctx: &ScanCtx,
) -> Result<Vec<Manifest>, SourceError> {
    let steamapps = join(library, "steamapps");
    let Ok(names) = list(env, &steamapps) else {
        ctx.warn(format!("library {} is not readable", library.display()));
        return Ok(Vec::new());
    };
    let mut ids: Vec<u32> = names
        .iter()
        .filter_map(|name| {
            name.strip_prefix("appmanifest_")?
                .strip_suffix(".acf")?
                .parse()
                .ok()
        })
        .collect();
    ids.sort_unstable();
    let mut manifests = Vec::new();
    for app_id in ids {
        ctx.check()?;
        let file = join(&steamapps, &format!("appmanifest_{app_id}.acf"));
        match vdf::load(env, &file, &vdf::small()) {
            Ok((value, warnings)) => {
                warnings.into_iter().for_each(|warning| ctx.warn(warning));
                let Some(state) = vdf::child(&value, "AppState") else {
                    ctx.warn(format!("{}: no AppState", file.display()));
                    continue;
                };
                if vdf::number::<u32>(state, "appid").is_some_and(|id| id != app_id) {
                    ctx.warn(format!(
                        "{}: appid does not match the file name",
                        file.display()
                    ));
                    continue;
                }
                let text = |name| {
                    vdf::text(state, name)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned)
                };
                manifests.push(Manifest {
                    app_id,
                    name: text("name"),
                    install_dir: text("installdir"),
                    flags: vdf::number(state, "StateFlags").unwrap_or(0),
                    size: vdf::number(state, "SizeOnDisk"),
                    last_played: vdf::number(state, "LastPlayed").filter(|time| *time > 0),
                    file,
                });
            }
            Err(error) => ctx.warn(error.to_string()),
        }
    }
    Ok(manifests)
}

const UNINSTALLED: u32 = 1;
const UPDATE_REQUIRED: u32 = 2;
const FULLY_INSTALLED: u32 = 4;
const FILES_MISSING: u32 = 32;
const FILES_CORRUPT: u32 = 128;
const UPDATE_RUNNING: u32 = 256;
const UPDATE_PAUSED: u32 = 512;
const UPDATE_STARTED: u32 = 1024;
const UNINSTALLING: u32 = 2048;
const PREALLOCATING: u32 = 1 << 19;
const DOWNLOADING: u32 = 1 << 20;
const STAGING: u32 = 1 << 21;
const COMMITTING: u32 = 1 << 22;

/// `StateFlags` as an install state; `None` for apps that are not (or no
/// longer) installed.
pub(super) fn state(flags: u32) -> Option<InstallState> {
    let updating = UPDATE_REQUIRED
        | UPDATE_RUNNING
        | UPDATE_PAUSED
        | UPDATE_STARTED
        | PREALLOCATING
        | DOWNLOADING
        | STAGING
        | COMMITTING;
    if flags == 0
        || flags & UNINSTALLING != 0
        || (flags & UNINSTALLED != 0 && flags & FULLY_INSTALLED == 0 && flags & updating == 0)
    {
        return None;
    }
    Some(if flags & (FILES_MISSING | FILES_CORRUPT) != 0 {
        InstallState::Broken {
            reason: if flags & FILES_MISSING != 0 {
                "files missing"
            } else {
                "files corrupt"
            }
            .into(),
        }
    } else if flags & FULLY_INSTALLED == 0 {
        InstallState::Partial
    } else if flags & updating != 0 {
        InstallState::Updating
    } else {
        InstallState::Installed
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_flags() {
        let broken = |reason: &str| {
            Some(InstallState::Broken {
                reason: reason.into(),
            })
        };
        assert_eq!(state(FULLY_INSTALLED), Some(InstallState::Installed));
        assert_eq!(
            state(FULLY_INSTALLED | UPDATE_REQUIRED),
            Some(InstallState::Updating)
        );
        assert_eq!(
            state(FULLY_INSTALLED | UPDATE_RUNNING | DOWNLOADING),
            Some(InstallState::Updating)
        );
        // First install, still downloading.
        assert_eq!(
            state(UPDATE_REQUIRED | UPDATE_STARTED),
            Some(InstallState::Partial)
        );
        assert_eq!(
            state(UNINSTALLED | UPDATE_REQUIRED),
            Some(InstallState::Partial)
        );
        assert_eq!(
            state(FULLY_INSTALLED | FILES_MISSING),
            broken("files missing")
        );
        assert_eq!(
            state(FULLY_INSTALLED | FILES_CORRUPT),
            broken("files corrupt")
        );
        assert_eq!(state(0), None);
        assert_eq!(state(UNINSTALLED), None);
        assert_eq!(state(FULLY_INSTALLED | UNINSTALLING), None);
    }
}
