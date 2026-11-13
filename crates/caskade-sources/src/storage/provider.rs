use super::{
    Result, StorageError, StorageGameReport, StorageGameStatus, StorageRootReport,
    StorageScanReport, StorageSource,
    identity::{check, importable, prepare},
};
use caskade_core::{
    bus::{Event, SourceCount},
    install::{STORAGE_SOURCE, from_manifest},
    model::{GameEntry, InstallLocation, StorageRoot},
};
use caskade_discovery::InstallState;
use caskade_installer::{
    exec::CancellationToken, manifest::RootManifest, reconcile::ReconcileStatus,
};
use std::collections::BTreeSet;

impl StorageSource {
    /// Register/adopt a physical RootManifest through core StorageService
    /// first. This method reconciles and imports that registered root only.
    pub async fn adopt_root(
        &self,
        root: &StorageRoot,
        cancel: CancellationToken,
    ) -> Result<StorageRootReport> {
        let report = self.adopt(root, cancel).await?;
        self.scan_completed(report.imported()).await;
        Ok(report)
    }

    /// [`Self::adopt_root`] without the completion event, so a refresh of
    /// several roots emits one.
    async fn adopt(
        &self,
        root: &StorageRoot,
        cancel: CancellationToken,
    ) -> Result<StorageRootReport> {
        let _guard = self.imports.lock().await;
        check(&cancel)?;
        let registered = self
            .store
            .get_root(&root.id)
            .await?
            .ok_or_else(|| StorageError::Invalid("root is not registered".into()))?;
        if registered.path != root.path {
            return Err(StorageError::Invalid("root registry path changed".into()));
        }
        let selected = root.path.clone();
        let expected = root.id.clone();
        caskade_jobs::blocking("Checking storage", move || -> Result<()> {
            if RootManifest::read(&selected)?.root_id() != expected {
                return Err(StorageError::Invalid(
                    "physical root identity differs from registry".into(),
                ));
            }
            Ok(())
        })
        .await??;
        let hint_error = if let Some(remounts) = &self.remounts {
            remounts.remember(root).await.err()
        } else {
            None
        };
        let scan = self.reconcile.scan_root(&root.path, cancel.clone()).await?;
        let scan = prepare(scan, cancel.clone()).await?;
        let mut report = StorageRootReport {
            root: root.clone(),
            games: vec![],
            warnings: scan.warnings,
            error: None,
        };
        if let Some(error) = hint_error {
            report.warnings.push(caskade_installer::Warning {
                code: "storage_remount_hints".into(),
                message: error.to_string(),
                path: None,
            });
        }
        let observed: BTreeSet<_> = scan.games.iter().map(|g| g.relative_dir.clone()).collect();
        for scanned in scan.games {
            check(&cancel)?;
            let mut game = StorageGameReport {
                relative_dir: scanned.relative_dir.clone(),
                status: status(scanned.status),
                game_id: None,
                entry_id: None,
                question: scanned.question,
                warnings: scanned.warnings,
                error: scanned.error,
            };
            if importable(scanned.status, game.question.as_ref())
                && let Some(manifest) = scanned.manifest
            {
                match from_manifest(&root.id, &scanned.relative_dir, &manifest) {
                    Ok((library_game, mut entry)) => {
                        if scanned.status == ReconcileStatus::NeedsUser {
                            let reason = "The managed installation needs reconciliation or \
                                          reinstall; review its storage scan question.";
                            entry.state = InstallState::Broken {
                                reason: reason.into(),
                            };
                        }
                        match self.existing_copy(&entry, root).await? {
                            Some(paths) => {
                                let reason = "This installation identity is already present at \
                                              another existing location.";
                                game.status = StorageGameStatus::NeedsUser;
                                game.question =
                                    Some(caskade_installer::Question::ChooseInstallDirectory {
                                        paths,
                                        reason: reason.into(),
                                    });
                            }
                            None => {
                                match self.store.import_group(&library_game, &[entry], None).await {
                                    Ok(saved) => {
                                        game.game_id = Some(saved.game_id.clone());
                                        game.entry_id = saved.entry_ids.into_iter().next();
                                        self.bus
                                            .emit_and_wait(if saved.created {
                                                Event::LibraryGameAdded {
                                                    game_id: saved.game_id,
                                                }
                                            } else {
                                                Event::LibraryGameUpdated {
                                                    game_id: saved.game_id,
                                                }
                                            })
                                            .await;
                                    }
                                    Err(error) => {
                                        game.status = StorageGameStatus::Failed;
                                        game.error = Some(error.to_string());
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        game.status = StorageGameStatus::Failed;
                        game.error = Some(error.to_string());
                    }
                }
            }
            report.games.push(game);
        }
        // A depth-limit/invalid-manifest warning means absence is uncertain.
        // Preserve known entries in incomplete scans and failed game folders.
        if report.warnings.is_empty() {
            for mut old in self.store.entries_for_root(&root.id).await? {
                check(&cancel)?;
                if old.source_id != STORAGE_SOURCE {
                    continue;
                }
                let Some(InstallLocation::Managed { rel_dir, .. }) = &old.location else {
                    continue;
                };
                if !observed.contains(rel_dir) && !matches!(old.state, InstallState::Partial) {
                    old.state = InstallState::Partial;
                    self.store.upsert_entry(&old).await?;
                    self.bus
                        .emit_and_wait(Event::LibraryGameUpdated {
                            game_id: old.game_id,
                        })
                        .await;
                }
            }
        }
        Ok(report)
    }

    /// Scan roots from the registry. Offline/malformed roots become report
    /// errors; another valid root can still restore its games.
    pub async fn refresh(&self, cancel: CancellationToken) -> Result<StorageScanReport> {
        let _activity = caskade_jobs::global().activity("Checking storage");
        let mut report = StorageScanReport::default();
        for mut root in self.store.roots().await? {
            check(&cancel)?;
            if let Some(remounts) = &self.remounts {
                match remounts.resolve(&root).await {
                    Ok(resolved) => root = resolved,
                    Err(error) => {
                        report.roots.push(StorageRootReport {
                            root,
                            games: vec![],
                            warnings: vec![],
                            error: Some(error.to_string()),
                        });
                        continue;
                    }
                }
            }
            let result = self.adopt(&root, cancel.clone()).await;
            check(&cancel)?;
            match result {
                Ok(scanned) => report.roots.push(scanned),
                Err(error) => report.roots.push(StorageRootReport {
                    root,
                    games: vec![],
                    warnings: vec![],
                    error: Some(error.to_string()),
                }),
            }
        }
        self.scan_completed(report.imported()).await;
        Ok(report)
    }

    async fn scan_completed(&self, imported: usize) {
        self.bus
            .emit_and_wait(Event::LibraryScanCompleted {
                sources: vec![SourceCount {
                    source: STORAGE_SOURCE.to_string(),
                    count: imported as u32,
                }],
            })
            .await;
    }

    async fn existing_copy(
        &self,
        incoming: &GameEntry,
        root: &StorageRoot,
    ) -> Result<Option<Vec<std::path::PathBuf>>> {
        let Some(old) = self
            .store
            .entry_by_source(STORAGE_SOURCE.as_str(), &incoming.external_id)
            .await?
        else {
            return Ok(None);
        };
        let (
            Some(InstallLocation::Managed { root_id, rel_dir }),
            Some(InstallLocation::Managed {
                rel_dir: new_dir, ..
            }),
        ) = (&old.location, &incoming.location)
        else {
            return Ok(None);
        };
        let Some(previous_root) = self.store.get_root(root_id).await? else {
            return Ok(None);
        };
        let previous = previous_root.path.join(rel_dir);
        let current = root.path.join(new_dir);
        if previous != current && previous.try_exists()? {
            return Ok(Some(vec![previous, current]));
        }
        Ok(None)
    }
}

fn status(status: ReconcileStatus) -> StorageGameStatus {
    match status {
        ReconcileStatus::Unchanged => StorageGameStatus::Unchanged,
        ReconcileStatus::Reconciled => StorageGameStatus::Reconciled,
        ReconcileStatus::NeedsUser => StorageGameStatus::NeedsUser,
        ReconcileStatus::Pinned => StorageGameStatus::Pinned,
        ReconcileStatus::Failed => StorageGameStatus::Failed,
    }
}
