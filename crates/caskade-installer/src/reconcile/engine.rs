use std::{fs, path::Path};

use crate::{
    Question, Warning,
    exec::{CancellationToken, with_root_exclusive},
    manifest::{InstallManifest, ManifestPath, RelocationVerdict, RootManifest},
    relocate::{RelocateError, RepairRequest},
};

use super::{GameReport, ReconcileError, ReconcileRequest, ReconcileStatus, Reconciler, Result};

impl Reconciler {
    pub async fn reconcile(
        &self,
        request: ReconcileRequest,
        cancellation: CancellationToken,
    ) -> Result<GameReport> {
        let worker = self.clone();
        let root = request.root.clone();
        with_root_exclusive(&root, cancellation, move |root, token| {
            let identity = RootManifest::read(&root)?;
            worker.reconcile_game(
                &root,
                identity.root_id(),
                &request.relative_dir,
                request.old_dir.as_deref(),
                &request.job_id,
                &token,
            )
        })
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reconcile_game(
        &self,
        root: &Path,
        root_id: &str,
        relative_dir: &Path,
        old_override: Option<&str>,
        job_id: &str,
        token: &CancellationToken,
    ) -> Result<GameReport> {
        check_cancel(token)?;
        let relative = ManifestPath::new(
            relative_dir
                .to_str()
                .ok_or_else(|| ReconcileError::Invalid("game path is not UTF-8".into()))?,
        )?;
        if relative.as_path().starts_with(".caskade") {
            return Err(ReconcileError::Invalid(
                "game is inside root metadata".into(),
            ));
        }
        let mut game = root.to_path_buf();
        for component in relative.as_path().components() {
            game.push(component);
            let kind = fs::symlink_metadata(&game)?.file_type();
            if !kind.is_dir() || kind.is_symlink() {
                return Err(ReconcileError::Invalid(
                    "game path must contain only real directories".into(),
                ));
            }
        }
        // dunce, like the root lease: std's verbatim \\?\ spelling on Windows
        // never starts with the root's ordinary one.
        let game = dunce::canonicalize(game)?;
        if !game.starts_with(root) || game == root {
            return Err(ReconcileError::Invalid("game escapes selected root".into()));
        }
        let manifest = InstallManifest::read(&game)?;
        let repair = match &self.factory {
            Some(factory) => factory.for_game(&game, &manifest)?,
            None => self.repair.clone(),
        };
        if repair.running.running(&game) {
            return Err(RelocateError::Running.into());
        }
        let record = manifest.record();
        let verdict = record.relocation.verdict;
        let mut report = GameReport {
            relative_dir: relative.as_path().into(),
            status: ReconcileStatus::Unchanged,
            old_dir: None,
            rewritten_files: 0,
            rewritten_values: 0,
            question: None,
            warnings: vec![],
            error: None,
            manifest: Some(manifest.clone()),
        };
        if verdict == RelocationVerdict::Pinned
            || record.plan.candidate.strategy == crate::Strategy::Delegate
        {
            report.status = ReconcileStatus::Pinned;
            report.warnings.push(warning(
                "reconcile_pinned",
                "Delegated installations cannot be repaired by moving their manifest folder.",
            ));
            return Ok(report);
        }

        let alias_target = match (&self.aliases, &record.alias) {
            (Some(store), Some(id)) => store.target(id).map_err(std::io::Error::other)?,
            _ => None,
        };
        let old = old_override
            .map(str::to_owned)
            .or_else(|| record.relocation.last_path.clone())
            .or_else(|| {
                alias_target
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
            });
        let old = match old {
            Some(old) => old,
            None if verdict == RelocationVerdict::Free => game.to_string_lossy().into_owned(),
            None if verdict == RelocationVerdict::Fragile => {
                let reason = "The moved Fragile installation has no previous-path anchor. Select its kept source or URL to reinstall; no paths were inferred or changed.";
                report.status = ReconcileStatus::NeedsUser;
                report.warnings.push(warning("reconcile_fragile", reason));
                report.question = Some(Question::ReinstallAfterMove {
                    title: record.title.clone(),
                    source: record.source.input.clone(),
                    reason: reason.into(),
                });
                return Ok(report);
            }
            None => {
                report.status = ReconcileStatus::NeedsUser;
                report.question = Some(Question::ChooseInstallDirectory {
                    paths: vec![game],
                    reason: "The legacy manifest has no previous real-path anchor. Supply its old location explicitly; it cannot be inferred safely.".into(),
                });
                return Ok(report);
            }
        };
        validate_anchor(&old)?;
        report.old_dir = Some(old.clone());
        let current = game.to_string_lossy();
        let path_changed = !same_spelling(&old, &current);
        let metadata_changed = record.plan.root_id != root_id
            || record.plan.relative_dir != relative.as_path()
            || record.relocation.last_path.is_none();
        let alias_changed = record.alias.is_some()
            && self.aliases.is_some()
            && alias_target
                .as_ref()
                .is_none_or(|path| !same_spelling(&path.to_string_lossy(), &current));
        if !path_changed && !metadata_changed && !alias_changed {
            return Ok(report);
        }

        // A copied game is not proof that the original moved. Do not steal its
        // alias or rewrite shared external state while both locations exist.
        if path_changed && Path::new(&old).is_absolute() {
            match fs::symlink_metadata(&old) {
                Ok(_) if dunce::canonicalize(&old).ok().as_deref() != Some(game.as_path()) => {
                    report.status = ReconcileStatus::NeedsUser;
                    report.question = Some(Question::ChooseInstallDirectory {
                        paths: vec![old.into(), game],
                        reason: "Both the previous and observed locations exist. Choose the authoritative installation before retargeting its alias or references.".into(),
                    });
                    return Ok(report);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        if let Some(target) = &alias_target
            && !same_spelling(&target.to_string_lossy(), &old)
            && !same_spelling(&target.to_string_lossy(), &current)
            && target.try_exists()?
        {
            report.status = ReconcileStatus::NeedsUser;
            report.question = Some(Question::ChooseInstallDirectory {
                paths: vec![target.clone(), game],
                reason: "The alias already names another existing installation.".into(),
            });
            return Ok(report);
        }
        let alias = match (&record.alias, &self.aliases) {
            (Some(id), Some(store)) => Some((store.clone(), id.clone())),
            (Some(_), None) => return Err(ReconcileError::MissingAliases(game)),
            (None, _) => None,
        };
        let (references, effective_verdict, skipped) =
            self.rescan(&game, &old, &manifest, &repair)?;
        if !skipped.is_empty() {
            report.warnings.push(warning(
                "reconcile_scan_incomplete",
                &format!(
                    "{} files exceeded the reference scan limit; relocation remains Fragile.",
                    skipped.len()
                ),
            ));
        }
        check_cancel(token)?;
        let repaired = repair.repair(
            &RepairRequest {
                job_id: job_id.into(),
                old_dir: old,
                game_dir: game.clone(),
                references,
                spellings: vec![],
                alias,
                root_id: Some(root_id.into()),
                relative_dir: Some(relative.as_path().into()),
                verdict: Some(effective_verdict),
            },
            token,
        )?;
        report.status = ReconcileStatus::Reconciled;
        report.rewritten_files = repaired.rewritten_files;
        report.rewritten_values = repaired.rewritten_values;
        report.manifest = Some(InstallManifest::read(&game)?);
        if effective_verdict == RelocationVerdict::Fragile {
            let reason = "Some recorded references cannot be rewritten safely. Known text/registry references were repaired; manifest-based uninstall remains required.";
            report.warnings.push(warning("reconcile_fragile", reason));
            report.question = Some(Question::ReinstallAfterMove {
                title: record.title.clone(),
                source: record.source.input.clone(),
                reason: reason.into(),
            });
            report.status = ReconcileStatus::NeedsUser;
        }
        Ok(report)
    }
}

pub(super) fn check_cancel(token: &CancellationToken) -> Result<()> {
    if token.is_cancelled() {
        Err(ReconcileError::Cancelled)
    } else {
        Ok(())
    }
}

pub(super) fn warning(code: &str, message: &str) -> Warning {
    Warning {
        code: code.into(),
        message: message.into(),
        path: None,
    }
}

fn validate_anchor(path: &str) -> Result<()> {
    let windows_absolute = path.as_bytes().get(1) == Some(&b':')
        && path
            .as_bytes()
            .get(2)
            .is_some_and(|c| matches!(c, b'/' | b'\\'));
    if path.is_empty()
        || path.len() > 32_768
        || path.chars().any(char::is_control)
        || !(Path::new(path).is_absolute() || windows_absolute || path.starts_with(r"\\"))
    {
        return Err(ReconcileError::Invalid(
            "invalid previous-path metadata anchor".into(),
        ));
    }
    Ok(())
}

fn same_spelling(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        let normalize = |path: &str| {
            path.strip_prefix(r"\\?\")
                .unwrap_or(path)
                .replace('/', "\\")
                .trim_end_matches('\\')
                .to_ascii_lowercase()
        };
        normalize(a) == normalize(b)
    } else {
        Path::new(a) == Path::new(b)
    }
}
