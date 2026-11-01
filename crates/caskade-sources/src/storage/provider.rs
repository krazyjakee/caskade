use super::{
    Result, StorageError, StorageGameReport, StorageGameStatus, StorageRootReport,
    StorageScanReport, StorageSource,
    identity::{check, importable, prepare},
};
use async_trait::async_trait;
use caskade_core::{
    bus::{Event, LibrarySource, SourceCount},
    install::{STORAGE_SOURCE, from_manifest},
    model::{GameEntry, InstallLocation, StorageRoot},
};
use caskade_discovery::{DetectedGame, GameRuntime, InstallState, LaunchTarget, TrackingHint};
use caskade_installer::{
    exec::CancellationToken, manifest::RootManifest, reconcile::ReconcileStatus,
};
use std::{collections::BTreeSet, path::Path};

impl StorageSource {
    /// Register/adopt a physical RootManifest through core StorageService
    /// first. This method reconciles and imports that registered root only.
    pub async fn adopt_root(
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
        .await
        .map_err(|e| StorageError::Invalid(e.to_string()))??;
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
        self.bus
            .emit_and_wait(Event::LibraryScanCompleted {
                sources: vec![SourceCount {
                    source: STORAGE_SOURCE.to_string(),
                    count: report.imported() as u32,
                }],
            })
            .await;
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
            let result = self.adopt_root(&root, cancel.clone()).await;
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
        self.bus
            .emit_and_wait(Event::LibraryScanCompleted {
                sources: vec![SourceCount {
                    source: STORAGE_SOURCE.to_string(),
                    count: report.imported() as u32,
                }],
            })
            .await;
        Ok(report)
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

#[async_trait]
impl LibrarySource for StorageSource {
    fn id(&self) -> &str {
        "caskade-storage"
    }
    async fn scan(&self) -> std::result::Result<Vec<DetectedGame>, String> {
        let report = self
            .refresh(CancellationToken::new())
            .await
            .map_err(|e| e.to_string())?;
        let mut games = Vec::new();
        for root in report.roots {
            for game in root.games {
                if let (Some(game_id), Some(entry_id)) = (game.game_id, game.entry_id) {
                    let entry = self
                        .store
                        .get_entry(&entry_id)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or("imported entry disappeared")?;
                    let game = self
                        .store
                        .game(&game_id)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or("imported game disappeared")?;
                    games.push(detected(&root.root.path, &game.title, entry));
                }
            }
        }
        Ok(games)
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
fn detected(root: &Path, title: &str, entry: GameEntry) -> DetectedGame {
    let base = match &entry.location {
        Some(InstallLocation::Managed { rel_dir, .. }) => root.join(rel_dir),
        _ => root.to_path_buf(),
    };
    let mut game = DetectedGame::new(STORAGE_SOURCE, entry.external_id, title);
    game.ids = entry.ids;
    game.install_dir = Some(base.clone());
    game.state = entry.state;
    game.size = entry.size;
    game.tracking = entry.tracking.or_else(|| {
        Some(TrackingHint {
            install_dir: Some(base.clone()),
            ..Default::default()
        })
    });
    game.runtime = entry.runtime;
    if let GameRuntime::Windows {
        prefix: Some(prefix),
        ..
    } = &mut game.runtime
    {
        prefix.path = base.join(&prefix.path);
    }
    game.launch = entry
        .launch
        .into_iter()
        .filter_map(|spec| match spec {
            caskade_core::model::LaunchSpec::Executable {
                path,
                args,
                cwd,
                env,
            } => Some(LaunchTarget::Exec {
                program: base.join(path),
                args,
                cwd: cwd.map(|p| base.join(p)),
                env,
            }),
            caskade_core::model::LaunchSpec::Uri { uri } => Some(LaunchTarget::Uri { uri }),
            caskade_core::model::LaunchSpec::Flatpak { app_id, args } => {
                Some(LaunchTarget::Flatpak { app_id, args })
            }
            _ => None,
        })
        .collect();
    game
}
