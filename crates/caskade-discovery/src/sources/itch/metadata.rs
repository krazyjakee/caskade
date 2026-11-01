use super::super::{is_file, read, relative};
use crate::{HostEnv, OperatingSystem, SourceError};
use serde::Deserialize;
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const MAX_RECEIPT: u64 = 2 << 20;
pub(super) fn receipt(env: &HostEnv, root: &Path) -> Result<Option<Value>, SourceError> {
    let path = root.join(".itch/receipt.json.gz");
    if !is_file(env, &path) {
        return Ok(None);
    }
    let bytes = read(env, &path, 1 << 20).map_err(|e| SourceError::io(&path, e))?;
    let mut decoded = vec![];
    flate2::read::GzDecoder::new(bytes.as_slice())
        .take(MAX_RECEIPT + 1)
        .read_to_end(&mut decoded)
        .map_err(|e| SourceError::parse(&path, e))?;
    if decoded.len() as u64 > MAX_RECEIPT {
        return Err(SourceError::parse(&path, "decoded receipt byte limit"));
    }
    serde_json::from_slice(&decoded)
        .map(Some)
        .map_err(|e| SourceError::parse(&path, e))
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Manifest {
    actions: Vec<Action>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Action {
    path: String,
    args: Vec<String>,
    platform: Option<String>,
}
pub(super) struct Target {
    pub program: PathBuf,
    pub args: Vec<String>,
}
pub(super) fn targets(
    env: &HostEnv,
    root: &Path,
    windows: bool,
) -> Result<Vec<Target>, SourceError> {
    let path = root.join(".itch.toml");
    if !is_file(env, &path) {
        return Ok(vec![]);
    }
    let bytes = read(env, &path, 1 << 20).map_err(|e| SourceError::io(&path, e))?;
    let manifest: Manifest =
        toml::from_str(std::str::from_utf8(&bytes).map_err(|e| SourceError::parse(&path, e))?)
            .map_err(|e| SourceError::parse(&path, e))?;
    if manifest.actions.len() > 256 {
        return Err(SourceError::parse(&path, "manifest action limit"));
    }
    let platform = if windows {
        "windows"
    } else {
        match env.os() {
            OperatingSystem::Linux => "linux",
            OperatingSystem::Windows => "windows",
            OperatingSystem::Macos => "osx",
        }
    };
    let mut targets = vec![];
    for action in manifest.actions {
        if action
            .platform
            .as_deref()
            .is_some_and(|p| p != "unknown" && p != platform)
        {
            continue;
        }
        if action.args.len() > 256
            || action
                .args
                .iter()
                .any(|a| a.len() > 16_384 || a.contains('\0'))
        {
            return Err(SourceError::parse(&path, "manifest argv limit"));
        }
        if let Some(program) = relative(env, root, &action.path) {
            targets.push(Target {
                program,
                args: action.args,
            });
        }
    }
    Ok(targets)
}
