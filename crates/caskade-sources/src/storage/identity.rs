use super::{Result, StorageError};
use caskade_installer::{
    exec::{CancellationToken, with_root_exclusive},
    manifest::{InstallManifest, ManifestPath, RootManifest},
    pipeline::INSTALL_ID,
    reconcile::{ReconcileStatus, RootReport},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub(super) async fn prepare(
    mut report: RootReport,
    cancel: CancellationToken,
) -> Result<RootReport> {
    let selected = report.root.clone();
    // Reconciliation already uses a root lease. Mint legacy IDs afterwards in
    // a second lease, never by nesting a lease around scan_root.
    report = with_root_exclusive(
        &selected,
        cancel,
        move |root, token| -> Result<RootReport> {
            if RootManifest::read(&root)?.root_id() != report.root_id {
                return Err(StorageError::Invalid(
                    "root identity changed during scan".into(),
                ));
            }
            for game in &mut report.games {
                check(&token)?;
                if !importable(game.status, game.question.as_ref()) {
                    continue;
                }
                let path = game_path(&root, &game.relative_dir)?;
                let manifest = InstallManifest::read(&path)?;
                let mut record = manifest.into_record();
                if !record.ids.contains_key(INSTALL_ID) {
                    record
                        .ids
                        .insert(INSTALL_ID.into(), caskade_core::model::new_id());
                    let manifest = InstallManifest::new(record)?;
                    manifest.write(&path)?;
                    game.manifest = Some(manifest);
                } else {
                    game.manifest = Some(InstallManifest::new(record)?);
                }
            }
            // A cloned manifest must not silently move a live entry/alias to the
            // second directory. Reconciler detects normal copies via old anchors;
            // this additionally catches duplicate IDs with edited/absent anchors.
            let mut copies: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
            for game in &report.games {
                if let Some(id) = game
                    .manifest
                    .as_ref()
                    .and_then(|m| m.record().ids.get(INSTALL_ID))
                {
                    copies
                        .entry(id.clone())
                        .or_default()
                        .push(root.join(&game.relative_dir));
                }
            }
            for game in &mut report.games {
                if let Some(paths) = game
                    .manifest
                    .as_ref()
                    .and_then(|m| m.record().ids.get(INSTALL_ID))
                    .and_then(|id| copies.get(id))
                    .filter(|paths| paths.len() > 1)
                {
                    let reason = "Several manifests carry the same installation identity; \
                                  choose the authoritative copy.";
                    game.status = ReconcileStatus::NeedsUser;
                    game.question = Some(caskade_installer::Question::ChooseInstallDirectory {
                        paths: paths.clone(),
                        reason: reason.into(),
                    });
                }
            }
            Ok(report)
        },
    )
    .await?;
    Ok(report)
}

pub(super) fn game_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let path = ManifestPath::new(
        relative
            .to_str()
            .ok_or_else(|| StorageError::Invalid("non-Unicode game directory".into()))?,
    )?;
    if path.as_path().starts_with(".caskade") {
        return Err(StorageError::Invalid("game is inside root metadata".into()));
    }
    let mut game = root.to_path_buf();
    for component in path.as_path().components() {
        game.push(component);
        let kind = fs::symlink_metadata(&game)?.file_type();
        if !kind.is_dir() || kind.is_symlink() {
            return Err(StorageError::Invalid(
                "game must contain real directories only".into(),
            ));
        }
    }
    let game = fs::canonicalize(game)?;
    if game == root || !game.starts_with(root) {
        return Err(StorageError::Invalid("game escaped its root".into()));
    }
    Ok(game)
}
/// Restore unresolved legacy/fragile installs for repair, while a choice
/// between physical copies cannot replace an existing entry or its alias.
pub(super) fn importable(
    status: ReconcileStatus,
    question: Option<&caskade_installer::Question>,
) -> bool {
    match status {
        ReconcileStatus::Unchanged | ReconcileStatus::Reconciled | ReconcileStatus::Pinned => true,
        ReconcileStatus::NeedsUser => {
            !matches!(question, Some(caskade_installer::Question::ChooseInstallDirectory { paths, .. }) if paths.len()>1)
        }
        ReconcileStatus::Failed => false,
    }
}
pub(super) fn check(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(StorageError::Cancelled)
    } else {
        Ok(())
    }
}
