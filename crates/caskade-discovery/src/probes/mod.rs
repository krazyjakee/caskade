//! Reusable read-only OS probes for discovery and future emulator detection.
//! All paths and variables come from HostEnv; no command, shell, application,
//! Flatpak portal or Windows Shell object is ever invoked. See README.md for
//! the stable consumer contract and feature availability.
#[cfg(feature = "deep")]
pub mod appimage;
pub mod bundles;
mod emulator;
#[cfg(feature = "linux-sources")]
pub mod flatpak;
pub mod path;
use crate::{HostEnv, LaunchTarget, ScanCtx, SourceError};
pub use emulator::{EmulatorCandidate, EmulatorOrigin, emulator_candidates};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
/// Neutral application metadata. Presence is not a game/emulator classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Application {
    pub title: String,
    pub location: PathBuf,
    pub identifier: Option<String>,
    pub executable: Option<PathBuf>,
    pub launch: LaunchTarget,
    pub icon: Option<PathBuf>,
    pub categories: Vec<String>,
    pub version: Option<String>,
}
fn exec(program: PathBuf, cwd: Option<PathBuf>) -> LaunchTarget {
    LaunchTarget::Exec {
        program,
        args: vec![],
        cwd,
        env: BTreeMap::new(),
    }
}
fn limit(ctx: &ScanCtx) -> usize {
    ctx.config().classifier.max_entries.min(4096) as usize
}
fn children(
    env: &HostEnv,
    ctx: &ScanCtx,
    path: &Path,
    remaining: &mut usize,
) -> Result<Vec<fs::DirEntry>, SourceError> {
    ctx.check()?;
    let path = env
        .resolve_path(path)
        .map_err(|e| SourceError::io(path, e))?;
    let iter = match fs::read_dir(&path) {
        Ok(iter) => iter,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(SourceError::io(&path, e)),
    };
    let mut entries = iter
        .take(remaining.saturating_add(1))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| SourceError::io(&path, e))?;
    if entries.len() > *remaining {
        return Err(SourceError::Other {
            message: "OS probe directory entry budget exceeded".into(),
        });
    }
    *remaining -= entries.len();
    entries.sort_by_key(|e| e.file_name());
    Ok(entries)
}
fn safe_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}
