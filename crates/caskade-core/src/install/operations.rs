//! Operations are selected by stored entry/root IDs. No client-supplied path,
//! execution step, manifest or candidate is used as an executable capability.
use super::*;
use crate::{
    launch::LaunchService,
    model::{GameEntry, InstallLocation},
};
use caskade_installer::{Question, pipeline::RunControl};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{future::Future, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobAnswer {
    Select { index: u32 },
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    pub checked_files: u64,
    pub missing: Vec<String>,
    pub changed: Vec<String>,
    pub valid: bool,
}

impl InstallService {
    /// Revision and bounds are checked while holding the job mutex. Answers
    /// only select from the server-owned question; they never carry paths.
    pub async fn answer(&self, id: &str, revision: u32, answer: JobAnswer) -> Result<JobSnapshot> {
        if matches!(answer, JobAnswer::Cancel) {
            let valid = lock(&self.inner.jobs).get(id).is_some_and(|job| {
                job.snapshot.revision == revision && job.snapshot.status == JobStatus::NeedsUser
            });
            if !valid {
                return Err(Error::Invalid(crate::tr!(
                    "question changed; refresh the job before answering"
                )));
            }
            return self.cancel(id);
        }
        let JobAnswer::Select { index } = answer else {
            unreachable!()
        };
        // Where installed ROMs go is decided here, not by the pipeline (#120).
        let destination = lock(&self.inner.jobs).get(id).is_some_and(|job| {
            job.snapshot.revision == revision
                && job.snapshot.status == JobStatus::NeedsUser
                && matches!(
                    job.snapshot.question,
                    Some(Question::ChooseDestination { .. })
                )
        });
        if destination {
            return self.place_roms(id, index).await;
        }
        let control = {
            let jobs = lock(&self.inner.jobs);
            let job = jobs
                .get(id)
                .ok_or_else(|| Error::Invalid(crate::tr!("unknown job")))?;
            if job.snapshot.revision != revision || job.snapshot.status != JobStatus::NeedsUser {
                return Err(Error::Invalid(crate::tr!(
                    "question changed; refresh the job before answering"
                )));
            }
            let question = job
                .snapshot
                .question
                .clone()
                .ok_or_else(|| Error::Invalid(crate::tr!("job has no question")))?;
            let count = match &question {
                Question::ChooseAsset { assets } => assets.len(),
                Question::ChooseInstaller { artifacts } => artifacts.len(),
                Question::ChooseLaunchTarget { targets } => targets.len(),
                Question::LowConfidence { candidates }
                | Question::ConfirmExecution { candidates } => candidates.len(),
                Question::ChooseDestination { options, .. } => options.len(),
                Question::ChooseInstallDirectory { .. } => {
                    return Err(Error::Invalid(crate::tr!(
                        "observed external directories require explicit adoption or reinstall; they are never moved automatically"
                    )));
                }
                Question::ReinstallAfterMove { .. } => {
                    return Err(Error::Invalid(crate::tr!(
                        "use the saved source in a new install request after reviewing the moved game"
                    )));
                }
            };
            if index as usize >= count {
                return Err(Error::Invalid(crate::tr!("choice is out of range")));
            }
            let mut control = job.control.clone();
            control.answers.push((question, index));
            control
        };
        self.restart(id, Some(revision), control).await
    }
    /// Runs a stopped install or update again from the start: with the
    /// alternative at `candidate_index`, or without one as it ran before,
    /// with the same choices.
    pub async fn retry(&self, id: &str, candidate_index: Option<u32>) -> Result<JobSnapshot> {
        self.user_acted(id);
        let control = {
            let jobs = lock(&self.inner.jobs);
            let job = jobs
                .get(id)
                .ok_or_else(|| Error::Invalid(crate::tr!("unknown job")))?;
            if !matches!(
                job.snapshot.status,
                JobStatus::Failed | JobStatus::Cancelled | JobStatus::NeedsUser
            ) {
                return Err(Error::Invalid(crate::tr!(
                    "only a stopped job can be retried"
                )));
            }
            let mut control = job.control.clone();
            if let Some(candidate_index) = candidate_index {
                let plans = job.control.plans().ok_or_else(|| {
                    Error::Invalid(crate::tr!("job has no retained alternatives"))
                })?;
                let candidate = plans
                    .candidates
                    .get(candidate_index as usize)
                    .ok_or_else(|| Error::Invalid(crate::tr!("alternative is out of range")))?
                    .clone();
                control.candidate = Some(candidate);
            } else if job.snapshot.status == JobStatus::NeedsUser {
                return Err(Error::Invalid(crate::tr!(
                    "the job is waiting for a choice; answer it instead"
                )));
            }
            control
        };
        self.restart(id, None, control).await
    }
    pub(super) async fn restart(
        &self,
        id: &str,
        revision: Option<u32>,
        control: RunControl,
    ) -> Result<JobSnapshot> {
        let (snapshot, cancel) = {
            let mut jobs = lock(&self.inner.jobs);
            let job = jobs
                .get_mut(id)
                .ok_or_else(|| Error::Invalid(crate::tr!("unknown job")))?;
            if job.active
                || !matches!(
                    job.snapshot.kind,
                    JobKind::Install | JobKind::Update | JobKind::PeerCopy
                )
                || revision.is_some_and(|r| r != job.snapshot.revision)
            {
                return Err(Error::Invalid(crate::tr!(
                    "job is still stopping or changed; refresh before retrying"
                )));
            }
            job.control = control;
            job.cancel = CancellationToken::new();
            job.active = true;
            job.snapshot.status = JobStatus::Queued;
            job.snapshot.question = None;
            job.snapshot.error = None;
            job.snapshot.failure = None;
            job.snapshot.progress = None;
            job.snapshot.revision += 1;
            job.snapshot.updated_at_ms = now_ms();
            (job.snapshot.clone(), job.cancel.clone())
        };
        let root = match self.root(snapshot.root_id.as_deref()).await {
            Ok(root) => root,
            Err(error) => {
                self.end_operation(id, Err(Error::Invalid(error.to_string())), false);
                return Err(error);
            }
        };
        self.publish(&snapshot);
        if snapshot.kind == JobKind::Update {
            self.spawn_update(id.to_owned(), snapshot.input.clone(), root, cancel);
        } else if snapshot.kind == JobKind::PeerCopy {
            // Again from the device: what arrived before stays.
            self.spawn_peer_copy(id.to_owned(), root, cancel);
        } else {
            self.spawn_install(id.to_owned(), snapshot.input.clone(), root, cancel);
        }
        Ok(snapshot)
    }
    pub(super) fn spawn_install(
        &self,
        id: String,
        input: String,
        root: StorageRoot,
        cancel: CancellationToken,
    ) {
        let service = self.clone();
        tokio::spawn(async move {
            let _activity = caskade_jobs::global().activity(crate::tr!("Installing game"));
            let result = service.run(&id, input, root, cancel.clone()).await;
            service.end_operation(&id, result, cancel.is_cancelled());
        });
    }
    pub(super) fn end_operation(&self, id: &str, result: Result<()>, cancelled: bool) {
        self.sync_plans(id);
        if let Some(job) = lock(&self.inner.jobs).get_mut(id) {
            job.active = false;
        }
        let (unclassified, retry) = match &result {
            Err(error) if !cancelled => {
                let unclassified = self.unclassified(id, error);
                let failure = self
                    .job(id)
                    .and_then(|job| job.failure)
                    .or(unclassified.clone());
                (unclassified, self.next_retry(id, failure.as_ref()))
            }
            _ => (None, None),
        };
        let ended = self.update(id, |job| {
            if !job.status.is_finished() {
                if cancelled {
                    job.status = JobStatus::Cancelled;
                    job.question = None;
                    job.failure = None;
                } else if let Err(error) = result {
                    job.status = JobStatus::Failed;
                    job.error = Some(caskade_installer::failure::redact(&error.to_string()));
                    if job.failure.is_none() {
                        job.failure = unclassified;
                    }
                    if let (Some(failure), Some(delay)) = (&mut job.failure, retry) {
                        let delay = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
                        failure.retry_at_ms = Some(now_ms().saturating_add(delay));
                    }
                }
            }
        });
        if let Some(ended) = ended {
            self.after_run(&ended, retry);
        }
    }
    pub(super) fn queue_operation<F, Fut>(
        &self,
        kind: JobKind,
        title: String,
        input: String,
        root_id: Option<String>,
        phase: JobPhase,
        operation: F,
    ) -> JobSnapshot
    where
        F: FnOnce(Self, String, CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let snapshot = JobSnapshot {
            phase: Some(phase),
            ..JobSnapshot::queued(kind, title, input, root_id)
        };
        let id = snapshot.id.clone();
        let cancel = self.add_job(snapshot.clone(), None, DiscoveryHints::default());
        let service = self.clone();
        tokio::spawn(async move {
            let label = match kind {
                JobKind::Install => crate::tr!("Installing game"),
                JobKind::Uninstall => crate::tr!("Uninstalling game"),
                JobKind::Verify => crate::tr!("Verifying game"),
                JobKind::Fetch => crate::tr!("Downloading game"),
                JobKind::Move => crate::tr!("Moving game"),
                JobKind::Update => crate::tr!("Updating game"),
                JobKind::PeerCopy => crate::tr!("Copying game"),
            };
            let _activity = caskade_jobs::global().activity(label);
            let result = operation(service.clone(), id.clone(), cancel.clone()).await;
            service.end_operation(&id, result, cancel.is_cancelled());
        });
        snapshot
    }
    pub async fn uninstall(&self, entry_id: String, launch: LaunchService) -> Result<JobSnapshot> {
        self.not_sending(&entry_id)?;
        let (entry, root, game) = self.managed_entry(&entry_id).await?;
        let lease = launch
            .try_game_lease(&entry.game_id)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if self
            .inner
            .store
            .in_flight_sessions()
            .await?
            .iter()
            .any(|s| s.game_id == entry.game_id)
        {
            return Err(Error::Invalid(crate::tr!("game has an open play session")));
        }
        Ok(self.queue_operation(
            JobKind::Uninstall,
            entry.external_id.clone(),
            entry_id,
            Some(root.id.clone()),
            JobPhase::Install,
            move |service, id, cancel| async move {
                let _lease = lease;
                service
                    .uninstall_files(&id, &cancel, &entry, &root, game)
                    .await?;
                service.inner.bus.emit(Event::LibraryGameUpdated {
                    game_id: entry.game_id.clone(),
                });
                service.update(&id, |job| {
                    job.status = JobStatus::Completed;
                    job.game_id = Some(entry.game_id);
                });
                Ok(())
            },
        ))
    }
    /// Deletes a managed entry's files and then the entry, inside job `id`,
    /// once its root is free.
    pub(super) async fn uninstall_files(
        &self,
        id: &str,
        cancel: &CancellationToken,
        entry: &GameEntry,
        root: &StorageRoot,
        game: PathBuf,
    ) -> Result<()> {
        let root_lock = self.root_lock(&root.id);
        let _root = root_lock.lock().await;
        if cancel.is_cancelled() {
            return Err(Error::Invalid(crate::tr!("cancelled")));
        }
        self.update(id, |job| job.status = JobStatus::Installing);
        let mut ctx = self.context(id, cancel.clone());
        ctx.config = self.preferences().await?.config();
        ctx.host = lock(&self.inner.host).clone();
        let manifest = InstallManifest::read(&game).map_err(|e| Error::Invalid(e.to_string()))?;
        if manifest.record().ids.get(pipeline::INSTALL_ID) != Some(&entry.external_id) {
            return Err(Error::Invalid(crate::tr!(
                "installation identity changed before uninstall"
            )));
        }
        pipeline::uninstall(game, vec![], &ctx)
            .await
            .map_err(|e| Error::Invalid(e.to_string()))?;
        self.inner.store.delete_entry(&entry.id).await?;
        Ok(())
    }
    pub async fn verify(&self, entry_id: String) -> Result<JobSnapshot> {
        let (entry, root, game) = self.managed_entry(&entry_id).await?;
        Ok(self.queue_operation(
            JobKind::Verify,
            entry.external_id,
            entry_id,
            Some(root.id),
            JobPhase::Install,
            move |service, id, cancel| async move {
                let report = caskade_installer::exec::with_root_exclusive(
                    &root.path,
                    cancel,
                    move |_, token| -> pipeline::PipelineResult<Verification> {
                        if token.is_cancelled() {
                            return Err(caskade_installer::exec::ExecError::Cancelled.into());
                        }
                        let manifest = InstallManifest::read(&game)
                            .map_err(|e| Error::Invalid(e.to_string()))?;
                        let report = manifest
                            .verify(&game)
                            .map_err(|e| Error::Invalid(e.to_string()))?;
                        Ok(Verification {
                            valid: report.is_ok(),
                            checked_files: report.checked_files,
                            missing: report
                                .missing
                                .iter()
                                .map(|p| p.as_path().to_string_lossy().into_owned())
                                .collect(),
                            changed: report
                                .changed
                                .iter()
                                .map(|p| p.path.as_path().to_string_lossy().into_owned())
                                .collect(),
                        })
                    },
                )
                .await
                .map_err(|e| Error::Invalid(e.to_string()))?;
                service.update(&id, |job| {
                    job.status = JobStatus::Completed;
                    job.verification = Some(report);
                });
                Ok(())
            },
        ))
    }
    pub async fn fetch(&self, input: String, root_id: Option<String>) -> Result<JobSnapshot> {
        if pipeline::input::remote(&input).is_none() {
            return Err(Error::Invalid(crate::tr!(
                "fetch requires an HTTP or HTTPS URL"
            )));
        }
        let root = self.root(root_id.as_deref()).await?;
        Ok(self.queue_operation(
            JobKind::Fetch,
            title_of(&input),
            input.clone(),
            Some(root.id.clone()),
            JobPhase::Download,
            move |service, id, cancel| async move {
                let preferences = service.preferences().await?;
                service
                    .inner
                    .downloads
                    .set_limit(preferences.concurrent_downloads as usize);
                let _permit = service.inner.downloads.acquire(&cancel).await?;
                service.update(&id, |job| job.status = JobStatus::Downloading);
                let mut ctx = service.context(&id, cancel);
                ctx.config = preferences.config();
                ctx.host = lock(&service.inner.host).clone();
                let host =
                    pipeline::actual_host(&ctx).map_err(|e| Error::Invalid(e.to_string()))?;
                let prepared = pipeline::input::prepare(input, &root.path, &id, &host, &ctx)
                    .await
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                service.update(&id, |job| {
                    job.status = JobStatus::Completed;
                    job.target_dir = Some(prepared.path);
                });
                Ok(())
            },
        ))
    }
    pub async fn move_game(
        &self,
        entry_id: String,
        target_root_id: String,
        storage: crate::storage::StorageService,
    ) -> Result<JobSnapshot> {
        self.not_sending(&entry_id)?;
        let (entry, _, from) = self.managed_entry(&entry_id).await?;
        self.root(Some(&target_root_id)).await?;
        Ok(self.queue_operation(
            JobKind::Move,
            entry.external_id,
            entry_id.clone(),
            Some(target_root_id.clone()),
            JobPhase::Move,
            move |service, id, cancel| async move {
                service.update(&id, |job| job.status = JobStatus::Installing);
                let ctx = service.context(&id, cancel.clone());
                let moved = storage
                    .move_entry(&entry_id, &target_root_id, &id, ctx.sink, cancel)
                    .await
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                let warning = super::hooks::notify::<super::hooks::InstallMoved>(
                    &service.inner.bus,
                    super::hooks::MovedGame {
                        job_id: id.clone(),
                        game_id: moved.game_id.clone(),
                        from,
                        to: moved.directory.clone(),
                    },
                )
                .await;
                service.update(&id, |job| {
                    job.warnings.extend(warning);
                    job.status = JobStatus::Completed;
                    job.target_dir = Some(moved.directory);
                    job.game_id = Some(moved.game_id);
                });
                Ok(())
            },
        ))
    }
    pub(super) fn context(&self, id: &str, cancel: CancellationToken) -> Context {
        let (control, hints) = lock(&self.inner.jobs)
            .get(id)
            .map(|job| (job.control.clone(), job.hints.clone()))
            .unwrap_or_default();
        let mut ctx = Context::new(Arc::new(Progress::new(self.clone(), id.into())), cancel);
        ctx.aliases = self.inner.options.aliases.clone();
        ctx.control = control;
        ctx.hints = hints;
        ctx.review = Some(Arc::new(super::hooks::BusReview {
            bus: self.inner.bus.clone(),
            job_id: id.to_owned(),
        }));
        #[cfg(feature = "net")]
        {
            ctx.resolvers = self
                .inner
                .bus
                .url_resolvers()
                .into_iter()
                .map(|resolver| {
                    Arc::new(super::hooks::BusResolver(resolver))
                        as Arc<dyn caskade_installer::fetch::UrlResolver>
                })
                .collect();
        }
        if let Some(tool) = lock(&self.inner.tool).clone() {
            ctx.tool = Some(tool);
        }
        ctx
    }
    /// Production adapters inject the bundled SHA-pinned utility. It is never
    /// accepted from an IPC request or resolved from downloaded input metadata.
    pub fn with_sidecar(self, path: PathBuf) -> Result<Self> {
        let tool = caskade_installer::formats::archive::sevenzip::SevenZip::pinned(path)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        *lock(&self.inner.tool) = Some(tool);
        Ok(self)
    }
    /// Copies the pipeline's plan set into the job. Publishes only when it
    /// changed: progress calls this on every report it publishes.
    pub(super) fn sync_plans(&self, id: &str) {
        let plans = {
            let jobs = lock(&self.inner.jobs);
            let Some(job) = jobs.get(id) else { return };
            let Some(plans) = job.control.plans() else {
                return;
            };
            if job.snapshot.candidates == plans.candidates
                && job.snapshot.selected_candidate_index == plans.selected
            {
                return;
            }
            plans
        };
        self.update(id, |job| {
            job.candidates = plans.candidates;
            job.selected_candidate_index = plans.selected;
        });
    }
    pub(super) async fn managed_entry(
        &self,
        id: &str,
    ) -> Result<(GameEntry, StorageRoot, PathBuf)> {
        let entry = self
            .inner
            .store
            .get_entry(id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown entry")))?;
        let Some(InstallLocation::Managed { root_id, rel_dir }) = &entry.location else {
            return Err(Error::Invalid(crate::tr!(
                "only managed installations support this operation"
            )));
        };
        if entry.source_id != STORAGE_SOURCE {
            return Err(Error::Invalid(crate::tr!(
                "entry is not a Caskade installation"
            )));
        }
        let root = self.root(Some(root_id)).await?;
        let identity = caskade_installer::manifest::RootManifest::read(&root.path)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if identity.root_id() != root.id {
            return Err(Error::Invalid(crate::tr!("storage root identity changed")));
        }
        let safe = caskade_installer::safety::sanitize_path(&rel_dir.to_string_lossy())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let canonical_root = std::fs::canonicalize(&root.path)?;
        let game = canonical_root.join(safe);
        if std::fs::symlink_metadata(&game)?.is_symlink()
            || !std::fs::canonicalize(&game)?.starts_with(&canonical_root)
        {
            return Err(Error::Invalid(crate::tr!(
                "managed directory escapes its root"
            )));
        }
        Ok((entry, root, game))
    }
}

#[cfg(test)]
#[path = "operation_tests.rs"]
mod tests;
