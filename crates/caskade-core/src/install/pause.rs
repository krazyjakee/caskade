//! Pausing installs and updates while they download (#131).
//!
//! Pausing stops the job's run the way cancelling does, but the job ends
//! [`JobStatus::Paused`] instead of cancelled, and is kept in `paused_jobs`
//! so it outlives a restart. Its download stays where every stopped run's
//! does: a partial HTTP file and its validators under the root's
//! `.caskade/downloads/<job id>`, a torrent's pieces under its info hash.
//! Resuming runs the job again under the same id, so the HTTP download
//! continues with a range request (or starts over when the server's file
//! changed) and the torrent checks the pieces it has and fetches the rest.
//!
//! Only a download pauses: once the files are being installed, stopping
//! rolls the install back, so that is a cancel. Copies from another device,
//! fetches and local installs don't pause.

use super::*;
use crate::launch::LaunchService;
use caskade_installer::{Candidate, Question};
use serde::{Deserialize, Serialize};

/// A paused job as `paused_jobs` keeps it: the job as it was, and what its
/// run needs again.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    snapshot: JobSnapshot,
    #[serde(default)]
    hints: DiscoveryHints,
    #[serde(default)]
    attach: Option<AttachTo>,
    /// An update's entry and release.
    #[serde(default)]
    update: Option<UpdateRecord>,
    /// The alternative the user picked, and their answers so far.
    #[serde(default)]
    candidate: Option<Candidate>,
    #[serde(default)]
    answers: Vec<(Question, u32)>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRecord {
    entry_id: String,
    version: String,
}

/// How long [`InstallService::pause`] waits for the run to stop before
/// answering; the pause is still kept when it stops later.
const STOPPING: Duration = Duration::from_secs(30);

fn gone() -> Error {
    Error::Invalid(crate::tr!("This job is no longer in Activity."))
}

impl Job {
    /// A running install or update still downloading, or waiting to.
    fn pausable(&self) -> bool {
        self.active && !self.pausing && self.snapshot.can_pause()
    }

    fn record(&self) -> Record {
        Record {
            snapshot: self.snapshot.clone(),
            hints: self.hints.clone(),
            attach: self.attach.clone(),
            update: self.update.as_ref().map(|target| UpdateRecord {
                entry_id: target.entry_id.clone(),
                version: target.version.clone(),
            }),
            candidate: self.control.candidate.clone(),
            answers: self.control.answers.clone(),
        }
    }
}

impl InstallService {
    /// Pauses an install or update that is downloading or waiting to: its
    /// run stops, what it downloaded stays, and it waits, paused, across
    /// restarts until it is resumed or cancelled.
    pub async fn pause(&self, id: &str) -> Result<JobSnapshot> {
        let cancel = {
            let mut jobs = lock(&self.inner.jobs);
            let job = jobs.get_mut(id).ok_or_else(gone)?;
            if !job.pausable() {
                return Err(Error::Invalid(crate::tr!(
                    "Only a download can be paused; this job isn't downloading."
                )));
            }
            job.pausing = true;
            job.cancel.clone()
        };
        cancel.cancel();
        if !self.stopped(id, STOPPING).await {
            // Kept once it stops.
            let service = self.clone();
            let stopping = id.to_owned();
            tokio::spawn(async move {
                if service.stopped(&stopping, STOPPING * 20).await {
                    service.keep_paused(&stopping).await;
                }
            });
            return self.job(id).ok_or_else(gone);
        }
        self.keep_paused(id).await;
        self.job(id).ok_or_else(gone)
    }

    /// Waits up to `bound` for job `id`'s run to have ended, paused or
    /// otherwise. `false` when it is still running.
    async fn stopped(&self, id: &str, bound: Duration) -> bool {
        let started = Instant::now();
        loop {
            let ended = lock(&self.inner.jobs).get(id).is_none_or(|job| {
                !job.active
                    && !matches!(
                        job.snapshot.status,
                        JobStatus::Queued | JobStatus::Downloading | JobStatus::Installing
                    )
            });
            if ended {
                return true;
            }
            if started.elapsed() >= bound {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Saves job `id` to `paused_jobs` if it is paused.
    async fn keep_paused(&self, id: &str) {
        let record = lock(&self.inner.jobs)
            .get(id)
            .filter(|job| job.snapshot.status == JobStatus::Paused)
            .map(Job::record);
        let Some(record) = record else { return };
        let saved = match serde_json::to_string(&record) {
            Ok(json) => self.inner.store.save_paused_job(id, &json, now_ms()).await,
            Err(error) => Err(error.into()),
        };
        if let Err(error) = saved {
            tracing::warn!(%error, job_id = id, "paused job not kept for the next start");
        }
    }

    /// Resumes a paused job: it runs again from where its download stopped.
    pub async fn resume(&self, id: &str) -> Result<JobSnapshot> {
        self.user_acted(id);
        let control = {
            let jobs = lock(&self.inner.jobs);
            let job = jobs.get(id).ok_or_else(gone)?;
            if job.snapshot.status != JobStatus::Paused {
                return Err(Error::Invalid(crate::tr!(
                    "Only a paused job can be resumed."
                )));
            }
            job.control.clone()
        };
        self.inner.store.delete_paused_job(id).await?;
        self.restart(id, None, control).await
    }

    /// A paused job that was cancelled is no longer kept.
    pub(super) fn forget_paused(&self, id: &str) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let store = self.inner.store.clone();
        let id = id.to_owned();
        runtime.spawn(async move {
            if let Err(error) = store.delete_paused_job(&id).await {
                tracing::warn!(%error, job_id = id, "cancelled paused job still kept");
            }
        });
    }

    /// Lists the jobs paused before the last exit again, paused. `launch`
    /// keeps an updated game from starting while its update runs; without
    /// it, paused updates wait for a start that has one.
    pub async fn restore_paused(&self, launch: Option<LaunchService>) -> usize {
        let rows = match self.inner.store.paused_jobs().await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "paused jobs not restored");
                return 0;
            }
        };
        let mut restored = 0;
        for (id, json) in rows {
            let record = match serde_json::from_str::<Record>(&json) {
                Ok(record) if record.snapshot.id == id => record,
                Ok(_) | Err(_) => {
                    tracing::warn!(job_id = id, "paused job unreadable; dropped");
                    if let Err(error) = self.inner.store.delete_paused_job(&id).await {
                        tracing::warn!(%error, job_id = id, "unreadable paused job kept");
                    }
                    continue;
                }
            };
            let update = match (record.update, &launch) {
                (Some(update), Some(launch)) => Some(update::UpdateTarget {
                    entry_id: update.entry_id,
                    version: update.version,
                    launch: launch.clone(),
                }),
                (Some(_), None) => continue,
                (None, _) => None,
            };
            let mut snapshot = record.snapshot;
            snapshot.status = JobStatus::Paused;
            snapshot.question = None;
            let mut control = pipeline::RunControl::default();
            control.candidate = record.candidate;
            control.answers = record.answers;
            {
                let mut jobs = lock(&self.inner.jobs);
                if jobs.contains_key(&id) {
                    continue;
                }
                jobs.insert(
                    id.clone(),
                    Job {
                        snapshot: snapshot.clone(),
                        cancel: CancellationToken::new(),
                        control,
                        active: false,
                        update,
                        hints: record.hints,
                        peer: None,
                        peer_fetcher: None,
                        attach: record.attach,
                        pausing: false,
                    },
                );
            }
            self.publish(&snapshot);
            restored += 1;
        }
        restored
    }
}

#[cfg(all(test, target_os = "linux", feature = "net"))]
#[path = "pause_tests.rs"]
mod tests;
