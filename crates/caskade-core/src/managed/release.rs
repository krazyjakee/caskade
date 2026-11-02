//! Putting a downloaded release in place, and taking it out again: unpack
//! beside the install, find the program with its detect rules, swap it in
//! with the user's files from the old version carried across, and on
//! removal delete only what releases shipped.

use super::unpack;
use crate::{
    Error, Result,
    emulation::{DetectRules, EmulatorInstall, OperatingSystem},
};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// One release to put in place.
pub(crate) struct Release {
    /// The downloaded file.
    pub file: PathBuf,
    /// An empty folder beside `target` to unpack into.
    pub staging: PathBuf,
    /// Where the old install waits while the new one moves in.
    pub aside: PathBuf,
    pub target: PathBuf,
    pub rules: DetectRules,
    pub os: OperatingSystem,
    /// The version being replaced, naming the backup of user files a newer
    /// release also ships.
    pub old_version: String,
    /// The program, for logs.
    pub name: String,
    /// The error when the release holds nothing the rules find.
    pub missing: String,
    pub cancel: CancellationToken,
}

/// Unpacks the release next to its target, finds the program in it, then
/// puts it in the target's place with the user's files from the old
/// install. A failure leaves the old install as it was.
pub(crate) async fn install_release(release: Release) -> Result<EmulatorInstall> {
    tokio::task::spawn_blocking(move || swap_in(release))
        .await
        .map_err(|error| Error::Invalid(error.to_string()))?
}

fn swap_in(release: Release) -> Result<EmulatorInstall> {
    let Release {
        file,
        staging,
        aside,
        target,
        rules,
        os,
        old_version,
        name,
        missing,
        cancel,
    } = release;
    let _ = std::fs::remove_dir_all(&staging);
    let unpacked = (|| {
        unpack::unpack(&file, &staging)?;
        let install = unpack::locate(&staging, &rules, os).ok_or_else(|| {
            let listing = unpack::files(&staging)
                .unwrap_or_default()
                .iter()
                .take(40)
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            tracing::warn!(program = name, listing, "the program isn't in the release");
            Error::Invalid(missing)
        })?;
        unpack::record_shipped(&staging)?;
        if cancel.is_cancelled() {
            return Err(Error::Invalid(crate::tr!("cancelled")));
        }
        Ok(install)
    })();
    let install = match unpacked {
        Ok(install) => install,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // The old install goes aside; it comes back if the swap fails.
    if target.exists() {
        std::fs::create_dir_all(aside.parent().unwrap_or(&aside))?;
        std::fs::rename(&target, &aside)?;
    }
    if let Err(error) = std::fs::rename(&staging, &target) {
        if aside.exists() {
            let _ = std::fs::rename(&aside, &target);
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error.into());
    }
    if aside.exists() {
        let kept = unpack::carry(&aside, &target, &old_version)?;
        tracing::info!(
            program = name,
            kept,
            "user files carried to the new version"
        );
        std::fs::remove_dir_all(&aside)?;
    }
    Ok(unpack::rebase(install, &staging, &target))
}

/// Deletes what the release in `dir` shipped and the folders that leaves
/// empty; the user's files (settings, state) stay. Returns how many files
/// went.
pub(crate) async fn remove_release(dir: &Path) -> Result<usize> {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || unpack::remove_shipped(&dir))
        .await
        .map_err(|error| Error::Invalid(error.to_string()))?
}
