//! A job's run: preparing its input (downloaded under the download limit),
//! installing it under its root's lock, and storing what it produced.

use super::*;

/// A run's input, downloaded or read, and what installing it needs.
pub(super) struct Ready {
    pub(super) prepared: pipeline::input::Prepared,
    pub(super) host: caskade_installer::host::HostCaps,
    pub(super) job_id: String,
    pub(super) preferences: crate::settings::InstallerSettings,
}

impl InstallService {
    pub(super) async fn run(
        &self,
        id: &str,
        input: String,
        root: StorageRoot,
        cancel: CancellationToken,
    ) -> Result<()> {
        let mut ctx = self.context(id, cancel.clone());
        let Some(ready) = self
            .prepare_run(id, &input, &root, &mut ctx, &cancel)
            .await?
        else {
            return Ok(());
        };
        let source = ready.prepared.path.clone();
        let download = ready.prepared.download_url.clone();
        let remote = pipeline::input::downloads(&input);
        let cleanup = source::SourceCleanup::capture(
            self,
            &ready.prepared,
            &input,
            // A copy's received folder is Caskade's own, never the user's to
            // keep or bin (#233).
            if lock(&self.inner.jobs)
                .get(id)
                .is_some_and(|job| job.peer.is_some())
            {
                crate::settings::SourceHandling::Delete
            } else {
                ready.preferences.source_handling
            },
            cancel.clone(),
        )
        .await?;

        let root_lock = self.root_lock(&root.id);
        let _guard = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Invalid(crate::tr!("cancelled"))),
            guard = root_lock.lock() => guard,
        };
        self.update(id, |job| {
            job.status = JobStatus::Installing;
            job.phase = Some(JobPhase::Install);
            // The download's last report isn't the install's.
            job.progress = None;
        });
        let outcome = pipeline::install_prepared(
            ready.prepared,
            root.path.clone(),
            ready.job_id,
            ready.host,
            ready.preferences.keep_downloads,
            false,
            &ctx,
        )
        .await
        .map_err(|error| {
            self.failed(
                id,
                &*error,
                caskade_installer::failure::Probe {
                    stage: self.running_stage(id),
                    source: Some(&source),
                    remote,
                    root: Some(&root.path),
                },
            )
        })?;
        self.sync_plans(id);
        self.finish(
            id,
            &root,
            &input,
            (&source, download.as_ref()),
            outcome,
            (cleanup, cancel),
        )
        .await
    }

    /// Sets `ctx` up for a run into `root`, then downloads `input` (under
    /// the download limit) or reads it. `None` when the job now waits on a
    /// question.
    pub(super) async fn prepare_run(
        &self,
        id: &str,
        input: &str,
        root: &StorageRoot,
        ctx: &mut Context,
        cancel: &CancellationToken,
    ) -> Result<Option<Ready>> {
        let preferences = self.preferences().await?;
        ctx.config = preferences.config();
        ctx.host = lock(&self.inner.host).clone();
        self.inner
            .downloads
            .set_limit(preferences.concurrent_downloads as usize);
        if let Some(storage) = self.storage()
            && ctx.folder_template.is_none()
        {
            ctx.folder_template = Some(
                storage
                    .folder_template(&root.id)
                    .await
                    .map_err(|error| Error::Invalid(error.to_string()))?,
            );
        }
        let failed =
            |error: Box<dyn std::error::Error + Send + Sync>| Error::Invalid(error.to_string());
        let mut host = pipeline::actual_host(ctx).map_err(failed)?;
        if let Some(storage) = self.storage() {
            let caps = storage
                .capabilities(&root.id)
                .await
                .map_err(|e| Error::Invalid(e.to_string()))?;
            host.filesystem = caskade_installer::host::FileSystemCaps {
                writable: caps.writable,
                exec_bits: caps.executable,
                symlinks: caps.symlinks,
                case_sensitive: caps.case_sensitive,
            };
        }
        let job_id = pipeline::input::job_id(input, Some(id.to_owned())).map_err(failed)?;
        let remote = pipeline::input::downloads(input);
        ctx.config.policy.execution = preferences.execution_for(remote);

        let prepared = {
            let _permit = if remote {
                self.update(id, |job| {
                    job.status = JobStatus::Downloading;
                    job.phase = Some(JobPhase::Download);
                });
                Some(self.inner.downloads.acquire(cancel).await?)
            } else {
                None
            };
            // A copy from another device is an installed game's folder:
            // installed as it is, never planned by what its files look like
            // (#76).
            let copied = lock(&self.inner.jobs)
                .get(id)
                .is_some_and(|job| job.peer.is_some());
            let prepared = if copied {
                pipeline::input::prepare_portable(input.into(), ctx).await
            } else {
                pipeline::input::prepare(input.to_owned(), &root.path, &job_id, &host, ctx).await
            };
            match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    if let Some(pipeline::NeedsDecision(question)) =
                        error.downcast_ref::<pipeline::NeedsDecision>()
                    {
                        self.update(id, |job| {
                            job.status = JobStatus::NeedsUser;
                            job.question = Some(question.clone());
                        });
                        return Ok(None);
                    }
                    return Err(self.failed(
                        id,
                        &*error,
                        caskade_installer::failure::Probe {
                            stage: caskade_installer::failure::FailureStage::Inspect,
                            source: (!remote).then(|| std::path::Path::new(input)),
                            remote,
                            root: Some(&root.path),
                        },
                    ));
                }
            }
        };
        if remote {
            self.inner.bus.emit(Event::InstallDownloaded {
                job_id: id.to_owned(),
            });
        }
        let handlers = self.inner.bus.install_handlers();
        if !handlers.is_empty() {
            let input = crate::bus::InstallInput {
                path: prepared.path.clone(),
                artifact: prepared.artifact.clone(),
            };
            ctx.handlers = hooks::propose(handlers, &input).await;
        }
        Ok(Some(Ready {
            prepared,
            host,
            job_id,
            preferences,
        }))
    }

    /// Stores what the install produced and finishes the job.
    async fn finish(
        &self,
        id: &str,
        root: &StorageRoot,
        input: &str,
        (source, download): (&std::path::Path, Option<&url::Url>),
        outcome: InstallOutcome,
        retirement: (Option<source::SourceCleanup>, CancellationToken),
    ) -> Result<()> {
        let store = &self.inner.store;
        match outcome {
            InstallOutcome::Installed(installed) => {
                let (cleanup, cancel) = retirement;
                let manifest = InstallManifest::read(&root.path.join(&installed.dir))
                    .map_err(|error| Error::Invalid(error.to_string()))?;
                let (game, mut entry) = from_manifest(&root.id, &installed.dir, &manifest)?;
                // A copy from another device is a release of the game the
                // user copied, while it's still here (#233), and a port one
                // of the game it was installed for.
                let (copied, attach) = {
                    let jobs = lock(&self.inner.jobs);
                    let job = jobs.get(id);
                    (
                        job.and_then(|job| job.peer.as_ref().map(|peer| peer.game_id.clone())),
                        job.and_then(|job| job.attach.clone()),
                    )
                };
                let target = copied.or_else(|| attach.as_ref().map(|to| to.game_id.clone()));
                let existing = match target {
                    Some(game_id) => store.game(&game_id).await?,
                    None => None,
                };
                let mut attach_warnings = Vec::new();
                let (game, added) = match (existing, attach) {
                    (Some(existing), Some(attach)) => {
                        attach_warnings =
                            attach::apply(&root.path.join(&installed.dir), &attach, &mut entry)
                                .await
                                .into_iter()
                                .map(|message| caskade_installer::Warning {
                                    code: "attach".into(),
                                    message,
                                    path: None,
                                })
                                .collect();
                        (existing, false)
                    }
                    (Some(existing), None) => (existing, false),
                    (None, _) => (store.upsert_game(&game).await?, true),
                };
                entry.game_id = game.id.clone();
                let entry = store.upsert_entry(&entry).await?;
                self.track(&entry.id, &manifest, download).await;
                let cleanup_warning = match cleanup {
                    Some(cleanup) => cleanup.finish(cancel).await,
                    None => None,
                };
                self.inner.bus.emit(if added {
                    Event::LibraryGameAdded {
                        game_id: game.id.clone(),
                    }
                } else {
                    Event::LibraryGameUpdated {
                        game_id: game.id.clone(),
                    }
                });
                let after_warning = hooks::notify::<hooks::InstallAfter>(
                    &self.inner.bus,
                    hooks::InstalledGame {
                        job_id: id.to_owned(),
                        game_id: game.id.clone(),
                        title: game.title.clone(),
                        dir: root.path.join(&installed.dir),
                    },
                )
                .await;
                self.update(id, |job| {
                    job.status = JobStatus::Completed;
                    job.title = game.title.clone();
                    job.warnings = installed
                        .warnings
                        .iter()
                        .cloned()
                        .chain(cleanup_warning)
                        .chain(after_warning)
                        .chain(attach_warnings.iter().cloned())
                        .collect();
                    job.game_id = Some(game.id.clone());
                    job.target_dir = Some(root.path.join(&installed.dir));
                    job.dependencies = installed.dependencies.clone();
                });
            }
            InstallOutcome::HandOff(caskade_installer::HandOff::Rom { platform, files }) => {
                self.finish_roms(id, &platform, &files, source).await?;
            }
            InstallOutcome::HandOff(handoff) => {
                let (game, entry) = from_handoff(&handoff, &title_of(input), source);
                let game = store.upsert_game(&game).await?;
                store.upsert_entry(&entry).await?;
                self.inner.bus.emit(Event::LibraryGameAdded {
                    game_id: game.id.clone(),
                });
                self.update(id, |job| {
                    job.status = JobStatus::Completed;
                    job.title = game.title.clone();
                    job.game_id = Some(game.id.clone());
                });
            }
            InstallOutcome::NeedsUser(question) => {
                self.update(id, |job| {
                    job.status = JobStatus::NeedsUser;
                    job.question = Some(question.clone());
                });
            }
            // Why, in words, with what can be done (#76): never the
            // installer's reason as it is.
            InstallOutcome::Unsupported(reason) => {
                self.record_failure(
                    id,
                    caskade_installer::failure::Failure::unsupported(
                        &reason,
                        Some(source.to_path_buf()),
                    ),
                );
                self.update(id, |job| {
                    job.status = JobStatus::Failed;
                    job.error = job.failure.as_ref().map(|failure| failure.summary.clone());
                });
            }
        }
        Ok(())
    }
}
