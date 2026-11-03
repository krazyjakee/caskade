//! Copies of games from another device (#233) as install jobs: the files
//! come from the device that has the game (a [`PeerFetcher`], over the
//! network), then the folder they make is installed like any other, as a
//! Caskade-managed install with a `caskade-install` id of its own, on the
//! game the user copied.
//!
//! While the device can't be reached (the network went, only a relay
//! reaches it), the job waits and tries again, and what arrived stays: a
//! copy resumes where it stopped.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use caskade_installer::discover::{DiscoveryHints, LaunchHint};
use caskade_installer::exec::CancellationToken;

use super::InstallService;
use super::jobs::{JobKind, JobPhase, JobProgress, JobSnapshot, JobStatus};
use crate::{Error, Result, lock, model::StorageRoot};

/// What a copy brings, and onto which game here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCopyTarget {
    pub device_id: String,
    pub device_name: String,
    /// The release on that device: its entry id there.
    pub entry_id: String,
    /// The game here the copy becomes a release of.
    pub game_id: String,
    pub title: String,
}

/// The folder a copy arrived as, and what the sender said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub folder: PathBuf,
    pub title: String,
    /// The program, relative to the folder and `/`-separated.
    pub launch: Option<String>,
}

/// How a fetch is going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerProgress {
    /// The other device is reading the game's files.
    Preparing {
        done: u64,
        total: u64,
    },
    Receiving {
        done: u64,
        total: u64,
    },
}

/// Why a fetch stopped.
#[derive(Debug)]
pub enum FetchFailure {
    /// It can go on later (the device is out of reach, or only a relay
    /// reaches it); what arrived stays.
    Waiting(String),
    Failed(Error),
}

/// Brings a game's files from another device.
#[async_trait]
pub trait PeerFetcher: Send + Sync {
    /// Fetches `target` into a folder under `root`, for job `job`.
    async fn fetch(
        &self,
        target: &PeerCopyTarget,
        root: &Path,
        job: &str,
        cancel: CancellationToken,
        progress: Arc<dyn Fn(PeerProgress) + Send + Sync>,
    ) -> std::result::Result<Fetched, FetchFailure>;
    /// The job is done with what it fetched: its files are installed.
    async fn finished(&self, root: &Path, job: &str);
    /// Whether this device is sending the release `entry_id` to another.
    fn sending(&self, entry_id: &str) -> bool;
}

/// How long a waiting copy waits before trying again, at most.
const MAX_WAIT: Duration = Duration::from_secs(60);

impl InstallService {
    /// Where copies from other devices come from.
    pub fn set_peer_fetcher(&self, fetcher: Arc<dyn PeerFetcher>) {
        *lock(&self.inner.peer_fetcher) = Some(fetcher);
    }

    fn peer_fetcher(&self) -> Result<Arc<dyn PeerFetcher>> {
        lock(&self.inner.peer_fetcher)
            .clone()
            .ok_or_else(|| Error::Invalid(crate::tr!("copies between devices aren't available")))
    }

    /// Refuses moving or removing a release while it's being sent.
    pub(super) fn not_sending(&self, entry_id: &str) -> Result<()> {
        let sending = lock(&self.inner.peer_fetcher)
            .as_ref()
            .is_some_and(|fetcher| fetcher.sending(entry_id));
        if sending {
            return Err(Error::Invalid(crate::tr!(
                "the game is being copied to another device; wait for it to finish or cancel it there"
            )));
        }
        Ok(())
    }

    /// Copies `target` from its device into a storage root here (the
    /// default when `root_id` is `None`), as a `peer_copy` job.
    pub async fn copy_from_device(
        &self,
        target: PeerCopyTarget,
        root_id: Option<String>,
    ) -> Result<JobSnapshot> {
        self.peer_fetcher()?;
        self.queue_peer_copy(target, root_id, None).await
    }

    /// [`copy_from_device`](Self::copy_from_device), with the files from
    /// `fetcher` (a share received from someone, #246): the same job,
    /// waiting, resume, cancel and install. The job keeps `fetcher` for its
    /// retries.
    pub async fn copy_with(
        &self,
        fetcher: Arc<dyn PeerFetcher>,
        target: PeerCopyTarget,
        root_id: Option<String>,
    ) -> Result<JobSnapshot> {
        self.queue_peer_copy(target, root_id, Some(fetcher)).await
    }

    async fn queue_peer_copy(
        &self,
        target: PeerCopyTarget,
        root_id: Option<String>,
        fetcher: Option<Arc<dyn PeerFetcher>>,
    ) -> Result<JobSnapshot> {
        let root = self.root(root_id.as_deref()).await?;
        let snapshot = JobSnapshot {
            phase: Some(JobPhase::Download),
            ..JobSnapshot::queued(
                JobKind::PeerCopy,
                target.title.clone(),
                target.device_name.clone(),
                Some(root.id.clone()),
            )
        };
        let cancel = self.add_job(snapshot.clone(), None, DiscoveryHints::default());
        if let Some(job) = lock(&self.inner.jobs).get_mut(&snapshot.id) {
            job.peer = Some(target);
            job.peer_fetcher = fetcher;
        }
        self.spawn_peer_copy(snapshot.id.clone(), root, cancel);
        Ok(snapshot)
    }

    pub(super) fn spawn_peer_copy(&self, id: String, root: StorageRoot, cancel: CancellationToken) {
        let service = self.clone();
        tokio::spawn(async move {
            let _activity = caskade_jobs::global().activity(crate::tr!("Copying game"));
            let result = service.run_peer_copy(&id, root, cancel.clone()).await;
            service.end_operation(&id, result, cancel.is_cancelled());
        });
    }

    async fn run_peer_copy(
        &self,
        id: &str,
        root: StorageRoot,
        cancel: CancellationToken,
    ) -> Result<()> {
        let (target, own) = lock(&self.inner.jobs)
            .get(id)
            .and_then(|job| Some((job.peer.clone()?, job.peer_fetcher.clone())))
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown job")))?;
        let fetcher = match own {
            Some(fetcher) => fetcher,
            None => self.peer_fetcher()?,
        };
        let service = self.clone();
        let job = id.to_owned();
        let device = target.device_name.clone();
        let progress: Arc<dyn Fn(PeerProgress) + Send + Sync> = Arc::new(move |progress| {
            let (message, done, total) = match progress {
                PeerProgress::Preparing { done, total } => (
                    crate::tr!("{device} is getting it ready", device = device.as_str()),
                    done,
                    total,
                ),
                PeerProgress::Receiving { done, total } => (
                    crate::tr!("From {device}", device = device.as_str()),
                    done,
                    total,
                ),
            };
            service.update(&job, |job| {
                job.status = JobStatus::Downloading;
                job.phase = Some(JobPhase::Download);
                job.progress = Some(JobProgress {
                    bytes: done,
                    total_bytes: Some(total),
                    files: 0,
                    eta_secs: None,
                    message,
                });
            });
        });
        let mut wait = Duration::from_secs(2);
        let fetched = loop {
            let attempt = fetcher.fetch(&target, &root.path, id, cancel.clone(), progress.clone());
            let outcome = tokio::select! {
                _ = cancel.cancelled() => return Err(Error::Invalid(crate::tr!("cancelled"))),
                outcome = attempt => outcome,
            };
            match outcome {
                Ok(fetched) => break fetched,
                Err(FetchFailure::Failed(error)) => return Err(error),
                Err(FetchFailure::Waiting(why)) => {
                    self.update(id, |job| {
                        job.status = JobStatus::Downloading;
                        let mut progress = job.progress.clone().unwrap_or(JobProgress {
                            bytes: 0,
                            total_bytes: None,
                            files: 0,
                            eta_secs: None,
                            message: String::new(),
                        });
                        progress.message = why.clone();
                        job.progress = Some(progress);
                    });
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(Error::Invalid(crate::tr!("cancelled"))),
                        _ = tokio::time::sleep(wait) => {}
                    }
                    wait = (wait * 2).min(MAX_WAIT);
                }
            }
        };
        // What the sender said outranks what the files suggest.
        let hints = DiscoveryHints {
            installer_title: Some(fetched.title.clone()),
            launch_targets: fetched
                .launch
                .iter()
                .map(|path| LaunchHint {
                    path: path.into(),
                    args: Vec::new(),
                    working_dir: None,
                    reason: "copied from another device".into(),
                })
                .collect(),
            ..Default::default()
        };
        if let Some(job) = lock(&self.inner.jobs).get_mut(id) {
            job.hints = hints;
        }
        let input = fetched.folder.display().to_string();
        let installed = self.run(id, input, root.clone(), cancel).await;
        if installed.is_ok() {
            fetcher.finished(&root.path, id).await;
        }
        installed
    }
}
