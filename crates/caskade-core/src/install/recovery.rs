//! Failed installs that can recover (#158): every failed install or update
//! carries a classified [`Failure`] with its stage, file and recovery
//! actions; transient failures retry on their own a bounded number of
//! times with growing delays; everything else waits for the user, who picks
//! a [`Recovery`]. Interrupted installs are rolled back at startup.
//!
//! A retry starts from the beginning: the input is inspected and planned
//! again, a kept download is checked against its saved validators and
//! checksum before it is used, and the executor first rolls back whatever
//! an earlier, interrupted run left (its journal: staged files, a native
//! installer's recorded effects), so an external installer that may have
//! run is reconciled before anything runs again.

use super::*;
use caskade_installer::failure::{self, Failure, FailureKind, FailureStage, Probe, RecoveryAction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

/// What the user chose to do about a failed install or update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Recovery {
    /// Run again as before, keeping a valid download.
    Retry,
    /// Run again after discarding the job's download.
    Redownload,
    /// Install from another file or folder (or URL), planned afresh.
    #[serde(rename_all = "camelCase")]
    LocateSource { input: String },
    /// Install into another storage root (installs only: an update
    /// replaces the game where it is).
    #[serde(rename_all = "camelCase")]
    ChooseDestination { root_id: String },
}

/// Per-job recovery bookkeeping, outside the snapshot.
#[derive(Default)]
pub(super) struct State {
    attempts: Mutex<HashMap<String, Attempts>>,
    /// Waits before each automatic retry; their count bounds the retries.
    delays: Mutex<Option<Vec<Duration>>>,
}

#[derive(Default)]
struct Attempts {
    /// The kind of the last failure and how many runs in a row had it.
    last: Option<FailureKind>,
    repeats: u32,
    /// Automatic retries since the user last acted.
    automatic: usize,
}

/// Automatic retries of transient failures wait this long, in turn.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(15), Duration::from_secs(60)];

impl InstallService {
    /// The stage a running install is in, from its last progress report.
    pub(super) fn running_stage(&self, id: &str) -> FailureStage {
        let Some(job) = self.job(id) else {
            return FailureStage::Install;
        };
        match (job.phase, job.progress.as_ref().map(|p| p.message.as_str())) {
            (Some(JobPhase::Download), _) => FailureStage::Download,
            (_, Some("finalize_started" | "finalize_completed")) => FailureStage::Finalize,
            (_, Some("commit_started" | "committed" | "completed")) => FailureStage::Commit,
            (Some(JobPhase::Install), _) => FailureStage::Install,
            _ => FailureStage::Inspect,
        }
    }

    /// Classifies a pipeline error, records it on job `id` and returns the
    /// error for the job's end, redacted.
    pub(super) fn failed(
        &self,
        id: &str,
        error: &(dyn std::error::Error + 'static),
        probe: Probe<'_>,
    ) -> Error {
        let failure = failure::classify(error, &probe);
        self.record_failure(id, failure);
        Error::Invalid(failure::redact(&error.to_string()))
    }

    /// Puts `failure` on the running job.
    pub(super) fn record_failure(&self, id: &str, mut failure: Failure) {
        let kind = lock(&self.inner.jobs).get(id).map(|job| job.snapshot.kind);
        if let Some(kind) = kind {
            self.settle(id, kind, &mut failure);
        }
        self.update(id, |job| job.failure = Some(failure));
    }

    /// Counts runs of job `id` in a row that failed like `failure`, and
    /// leaves out what an update can't do.
    fn settle(&self, id: &str, kind: JobKind, failure: &mut Failure) {
        if kind == JobKind::Update {
            // An update replaces the game where it is.
            failure
                .actions
                .retain(|action| *action != RecoveryAction::ChooseDestination);
        }
        let mut attempts = lock(&self.inner.recovery.attempts);
        let attempts = attempts.entry(id.to_owned()).or_default();
        attempts.repeats = if attempts.last == Some(failure.kind) {
            attempts.repeats + 1
        } else {
            1
        };
        attempts.last = Some(failure.kind);
        failure.occurrences = attempts.repeats;
        super::failure_text::translate(failure);
    }

    /// A failure for an error nothing classified on its way out.
    pub(super) fn unclassified(&self, id: &str, error: &Error) -> Option<Failure> {
        let job = self.job(id)?;
        if job.failure.is_some() || !matches!(job.kind, JobKind::Install | JobKind::Update) {
            return None;
        }
        let remote = pipeline::input::downloads(&job.input);
        let probe = Probe {
            stage: self.running_stage(id),
            source: (!remote).then(|| Path::new(&job.input)),
            remote,
            root: None,
        };
        let error: &(dyn std::error::Error + 'static) = match error {
            Error::Io(io) => io,
            other => other,
        };
        let mut failure = failure::classify(error, &probe);
        self.settle(id, job.kind, &mut failure);
        Some(failure)
    }

    /// When a run of job `id` that failed like `failure` should run again
    /// on its own: only transient failures of installs and updates, and
    /// only as many times as there are delays.
    pub(super) fn next_retry(&self, id: &str, failure: Option<&Failure>) -> Option<Duration> {
        let job = self.job(id)?;
        if !failure.is_some_and(|failure| failure.transient)
            || job.status.is_finished()
            || !matches!(job.kind, JobKind::Install | JobKind::Update)
        {
            return None;
        }
        let delays = lock(&self.inner.recovery.delays)
            .clone()
            .unwrap_or_else(|| RETRY_DELAYS.to_vec());
        let mut attempts = lock(&self.inner.recovery.attempts);
        let attempts = attempts.entry(id.to_owned()).or_default();
        let delay = delays.get(attempts.automatic).copied()?;
        attempts.automatic += 1;
        Some(delay)
    }

    /// After a run ends: forgets a finished job's bookkeeping, or waits
    /// `retry` and runs the failed job again, unless the user did anything
    /// with it meanwhile (cancelling it stops the wait).
    pub(super) fn after_run(&self, ended: &JobSnapshot, retry: Option<Duration>) {
        if matches!(ended.status, JobStatus::Completed | JobStatus::Cancelled) {
            lock(&self.inner.recovery.attempts).remove(&ended.id);
            return;
        }
        let Some(delay) = retry.filter(|_| ended.status == JobStatus::Failed) else {
            return;
        };
        let Some(cancel) = lock(&self.inner.jobs)
            .get(&ended.id)
            .map(|job| job.cancel.clone())
        else {
            return;
        };
        let service = self.clone();
        let (id, revision) = (ended.id.clone(), ended.revision);
        tokio::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(delay) => {}
            }
            let control = lock(&service.inner.jobs)
                .get(&id)
                .filter(|job| {
                    job.snapshot.revision == revision && job.snapshot.status == JobStatus::Failed
                })
                .map(|job| job.control.clone());
            if let Some(control) = control
                && let Err(error) = service.restart(&id, None, control).await
            {
                tracing::warn!(%error, job_id = %id, "automatic retry not started");
            }
        });
    }

    /// The user acted: automatic retries start counting again.
    pub(super) fn user_acted(&self, id: &str) {
        if let Some(attempts) = lock(&self.inner.recovery.attempts).get_mut(id) {
            attempts.automatic = 0;
        }
    }

    /// Waits `delays` before automatic retries instead of the defaults.
    #[cfg(all(test, target_os = "linux", feature = "net"))]
    pub(crate) fn set_retry_delays(&self, delays: Vec<Duration>) {
        *lock(&self.inner.recovery.delays) = Some(delays);
    }

    /// Recovers failed or cancelled install or update `id` as the user
    /// chose. The new input or root is checked like a new install's; the
    /// run plans again and asks again before running installer code.
    pub async fn recover(&self, id: &str, recovery: Recovery) -> Result<JobSnapshot> {
        let snapshot = self
            .job(id)
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown job")))?;
        if !matches!(snapshot.status, JobStatus::Failed | JobStatus::Cancelled)
            || !matches!(snapshot.kind, JobKind::Install | JobKind::Update)
        {
            return Err(Error::Invalid(crate::tr!(
                "only a failed or cancelled install or update can recover"
            )));
        }
        self.user_acted(id);
        match recovery {
            Recovery::Retry => return self.retry(id, None).await,
            Recovery::Redownload => {
                if !pipeline::input::downloads(&snapshot.input) {
                    return Err(Error::Invalid(crate::tr!(
                        "the job didn't download anything"
                    )));
                }
                if let Ok(root) = self.root(snapshot.root_id.as_deref()).await {
                    self.discard_download(id, &root).await;
                }
            }
            Recovery::LocateSource { input } => {
                let input = input.trim().to_owned();
                if input.is_empty() || input.len() > 32_768 || input.contains('\0') {
                    return Err(Error::Invalid(crate::tr!(
                        "input must be a nonempty path or URL"
                    )));
                }
                if pipeline::input::downloads(&snapshot.input)
                    && let Ok(root) = self.root(snapshot.root_id.as_deref()).await
                {
                    self.discard_download(id, &root).await;
                }
                // Nothing chosen for the old input applies to the new one.
                let mut jobs = lock(&self.inner.jobs);
                let job = jobs
                    .get_mut(id)
                    .ok_or_else(|| Error::Invalid(crate::tr!("unknown job")))?;
                job.snapshot.input = input;
                job.control = pipeline::RunControl::default();
            }
            Recovery::ChooseDestination { root_id } => {
                if snapshot.kind != JobKind::Install {
                    return Err(Error::Invalid(crate::tr!(
                        "an update installs where the game is"
                    )));
                }
                let target = self.root(Some(&root_id)).await?;
                if let Ok(old) = self.root(snapshot.root_id.as_deref()).await
                    && old.id != target.id
                {
                    self.discard_download(id, &old).await;
                }
                let mut jobs = lock(&self.inner.jobs);
                let job = jobs
                    .get_mut(id)
                    .ok_or_else(|| Error::Invalid(crate::tr!("unknown job")))?;
                job.snapshot.root_id = Some(target.id);
            }
        }
        let control = lock(&self.inner.jobs)
            .get(id)
            .map(|job| job.control.clone())
            .unwrap_or_default();
        self.restart(id, None, control).await
    }

    /// Deletes job `id`'s download folder in `root`: job-owned, nothing
    /// else is touched.
    async fn discard_download(&self, id: &str, root: &StorageRoot) {
        #[cfg(feature = "net")]
        {
            let ctx = self.context(id, CancellationToken::new());
            let result = match pipeline::input::fetch_engine(false, &ctx) {
                Ok(engine) => engine
                    .after_install(&root.path, id)
                    .await
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = result {
                tracing::warn!(%error, job_id = id, "download not discarded");
            }
        }
        #[cfg(not(feature = "net"))]
        let _ = (id, root);
    }

    /// Rolls back installs the app stopped in the middle of, in every
    /// reachable root: staged files go, and a native installer's recorded
    /// effects are undone, leaving installed games and other files alone.
    /// Roots that are offline wait for the next start (and every install
    /// does the same for its root before it runs).
    pub async fn recover_interrupted_installs(&self) {
        let roots = match self.inner.store.roots().await {
            Ok(roots) => roots,
            Err(error) => {
                tracing::warn!(%error, "interrupted installs not checked");
                return;
            }
        };
        let executor = caskade_installer::exec::Executor::new(
            Arc::new(caskade_installer::runtime::NativeRuntime::new()),
            Arc::new(caskade_installer::disk::FileSystemDiskSpace),
            Arc::new(Quiet),
        );
        for root in roots {
            if !root.path.join(".caskade").is_dir() {
                continue;
            }
            match executor.cleanup(&root.path).await {
                Ok(0) => {}
                Ok(count) => {
                    tracing::info!(count, root = %root.id, "interrupted installs rolled back");
                }
                Err(error) => {
                    tracing::warn!(%error, root = %root.id, "interrupted installs left for the next run");
                }
            }
        }
    }
}

/// Progress nobody follows.
struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _: JobEvent) {}
}

#[cfg(all(test, target_os = "linux"))]
#[path = "recovery_tests.rs"]
mod tests;
