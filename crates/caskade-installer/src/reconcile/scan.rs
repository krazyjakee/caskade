use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    exec::{CancellationToken, with_root_exclusive},
    manifest::{INSTALL_MANIFEST_PATH, RootManifest},
};

use super::engine::{check_cancel, warning};
use super::{GameReport, ReconcileError, ReconcileStatus, Reconciler, Result, RootReport};

impl Reconciler {
    /// Installer-side storage scan; application storage integration (#54) calls
    /// this entry point after resolving a selected root. No files are moved.
    /// Collection is bounded and completes before any game is repaired.
    pub async fn scan_root(
        &self,
        root: &Path,
        cancellation: CancellationToken,
    ) -> Result<RootReport> {
        let worker = self.clone();
        with_root_exclusive(root, cancellation, move |root, token| {
            let identity = RootManifest::read(&root)?;
            let mut report = RootReport {
                root: root.clone(),
                root_id: identity.root_id().into(),
                games: vec![],
                warnings: vec![],
            };
            let games = worker.collect_games(&root, &token, &mut report)?;
            for (index, relative) in games.into_iter().enumerate() {
                check_cancel(&token)?;
                match worker.reconcile_game(
                    &root,
                    identity.root_id(),
                    &relative,
                    None,
                    &format!("reconcile-{index}"),
                    &token,
                ) {
                    Ok(game) => report.games.push(game),
                    Err(error) => {
                        check_cancel(&token)?;
                        report.games.push(GameReport {
                            relative_dir: relative,
                            status: ReconcileStatus::Failed,
                            old_dir: None,
                            rewritten_files: 0,
                            rewritten_values: 0,
                            question: None,
                            warnings: vec![],
                            error: Some(error.to_string()),
                            manifest: None,
                        });
                    }
                }
            }
            Ok(report)
        })
        .await
    }

    fn collect_games(
        &self,
        root: &Path,
        token: &CancellationToken,
        report: &mut RootReport,
    ) -> Result<Vec<PathBuf>> {
        let mut pending = vec![(PathBuf::new(), 0usize)];
        let mut games = vec![];
        let mut seen = 0usize;
        while let Some((relative, depth)) = pending.pop() {
            check_cancel(token)?;
            let mut entries = vec![];
            for entry in fs::read_dir(root.join(&relative))? {
                check_cancel(token)?;
                seen += 1;
                if seen > self.limits.max_entries {
                    return Err(ReconcileError::Limit(self.limits.max_entries));
                }
                entries.push(entry?);
            }
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                check_cancel(token)?;
                if entry.file_name() == ".caskade" {
                    continue;
                }
                let kind = entry.file_type()?;
                if kind.is_symlink() || !kind.is_dir() {
                    continue;
                }
                let child = relative.join(entry.file_name());
                let path = root.join(&child);
                match fs::symlink_metadata(path.join(INSTALL_MANIFEST_PATH)) {
                    Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                        games.push(child);
                    }
                    Ok(_) => report.warnings.push(warning(
                        "reconcile_invalid_manifest",
                        &format!("Ignored non-file manifest in {}", child.display()),
                    )),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if path.join(super::WEB_BUNDLE_MANIFEST).is_file() {
                            // A web game's offline copy: its files are
                            // Caskade's, never an install.
                            continue;
                        }
                        if depth + 1 < self.limits.max_depth {
                            pending.push((child, depth + 1));
                        } else {
                            report.warnings.push(warning(
                                "reconcile_depth_limit",
                                &format!("Did not descend into {}", child.display()),
                            ));
                        }
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
        games.sort();
        Ok(games)
    }
}
