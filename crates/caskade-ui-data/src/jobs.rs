//! Jobs for Activity (#101): installs, downloads, moves, copies and the
//! other background jobs, live from `JobChanged` (`Key::Jobs`): listing,
//! cancelling, pausing and resuming downloads (#131), removing, retrying
//! (as before or another way), answering a
//! job's question and recovering a failed one; through the app layer's
//! commands as the webview's `api/installNative.ts` does.
//!
//! A [`JobQueue`] is the running app's install service ([`InstallQueue`])
//! or, in fixtures and tests, [`FakeQueue`]: jobs a test puts there, with
//! what each action asked for.

pub mod requests;

use std::sync::{Arc, Mutex};

use caskade_app::commands::game_edit;
use caskade_app::commands::install as commands;
use caskade_app::events::GamesRefreshChanged;
use caskade_app::events::{Event, Events, JobChanged};
use caskade_core::install::{InstallService, JobAnswer, JobSnapshot, JobStatus, Recovery};
use caskade_core::metadata::{Metadata, RefreshMode, RefreshStatus};
use caskade_core::store_accounts::async_trait;

use caskade_app::commands::install_requests::InstallRequest;
use requests::InstallInbox;

type Result<T> = std::result::Result<T, String>;

/// What Activity does with jobs.
#[async_trait]
pub trait JobQueue: Send + Sync {
    fn list(&self) -> Vec<JobSnapshot>;
    fn cancel(&self, id: &str) -> Result<JobSnapshot>;
    /// Pauses a download, keeping it (#131).
    async fn pause(&self, id: &str) -> Result<JobSnapshot>;
    /// Resumes a paused download from where it stopped.
    async fn resume(&self, id: &str) -> Result<JobSnapshot>;
    /// Takes a finished job out of Activity; its files stay.
    fn remove(&self, id: &str) -> Result<()>;
    /// Runs a failed or cancelled job again: as before, or with the
    /// install way at `candidate`.
    async fn retry(&self, id: &str, candidate: Option<u32>) -> Result<JobSnapshot>;
    /// Answers the job's question at `revision`.
    async fn answer(&self, id: &str, revision: u32, answer: JobAnswer) -> Result<JobSnapshot>;
    /// Recovers a failed or cancelled install or update (#158).
    async fn recover(&self, id: &str, recovery: Recovery) -> Result<JobSnapshot>;
}

/// The running app's install service.
pub struct InstallQueue(pub InstallService);

#[async_trait]
impl JobQueue for InstallQueue {
    fn list(&self) -> Vec<JobSnapshot> {
        commands::jobs_list(&self.0)
    }
    fn cancel(&self, id: &str) -> Result<JobSnapshot> {
        commands::job_cancel(&self.0, id.to_owned())
    }
    async fn pause(&self, id: &str) -> Result<JobSnapshot> {
        commands::job_pause(&self.0, id.to_owned()).await
    }
    async fn resume(&self, id: &str) -> Result<JobSnapshot> {
        commands::job_resume(&self.0, id.to_owned()).await
    }
    fn remove(&self, id: &str) -> Result<()> {
        commands::job_remove(&self.0, id.to_owned())
    }
    async fn retry(&self, id: &str, candidate: Option<u32>) -> Result<JobSnapshot> {
        commands::job_retry(&self.0, id.to_owned(), candidate).await
    }
    async fn answer(&self, id: &str, revision: u32, answer: JobAnswer) -> Result<JobSnapshot> {
        commands::job_answer(&self.0, id.to_owned(), revision, answer).await
    }
    async fn recover(&self, id: &str, recovery: Recovery) -> Result<JobSnapshot> {
        commands::job_recover(&self.0, id.to_owned(), recovery).await
    }
}

/// The refresh of chosen games' details (#176), at the top of Jobs.
#[async_trait]
pub trait DetailsRefresh: Send + Sync {
    /// The last refresh's status, or `None` when none ran.
    fn status(&self) -> Option<RefreshStatus>;
    async fn start(&self, game_ids: Vec<String>, mode: RefreshMode) -> Result<()>;
    /// Stops it after its current batch; whether one was running.
    fn cancel(&self) -> bool;
}

/// The running app's metadata service.
pub struct MetadataRefresh(pub Metadata);

#[async_trait]
impl DetailsRefresh for MetadataRefresh {
    fn status(&self) -> Option<RefreshStatus> {
        game_edit::games_refresh_status(&self.0)
    }
    async fn start(&self, game_ids: Vec<String>, mode: RefreshMode) -> Result<()> {
        game_edit::games_refresh_start(&self.0, game_ids, mode)
            .await
            .map_err(|error| error.summary)
    }
    fn cancel(&self) -> bool {
        game_edit::games_refresh_cancel(&self.0)
    }
}

/// Jobs and the details refresh, over one app's services.
#[derive(Clone)]
pub struct Jobs {
    queue: Option<Arc<dyn JobQueue>>,
    refresh: Option<Arc<dyn DetailsRefresh>>,
    requests: Option<Arc<dyn InstallInbox>>,
}

fn no_queue() -> String {
    caskade_core::tr!("Storage service is unavailable")
}

impl Jobs {
    pub fn new(queue: Option<Arc<dyn JobQueue>>, refresh: Option<Arc<dyn DetailsRefresh>>) -> Self {
        Self {
            queue,
            refresh,
            requests: None,
        }
    }

    /// With install requests to confirm.
    pub fn with_requests(self, requests: Arc<dyn InstallInbox>) -> Self {
        Self {
            requests: Some(requests),
            ..self
        }
    }

    /// The running app's jobs, details refresh and install requests, which
    /// announce through `events`.
    pub fn of(services: &caskade_app::start::Services, events: Events) -> Self {
        let jobs = Self::new(
            services
                .install
                .clone()
                .map(|install| Arc::new(InstallQueue(install)) as Arc<dyn JobQueue>),
            Some(Arc::new(MetadataRefresh(services.metadata.clone()))),
        );
        match requests::AppInbox::of(services, events) {
            Some(inbox) => jobs.with_requests(Arc::new(inbox)),
            None => jobs,
        }
    }

    /// Whether this app has a job queue (installs).
    pub fn has_queue(&self) -> bool {
        self.queue.is_some()
    }

    fn queue(&self) -> Result<&Arc<dyn JobQueue>> {
        self.queue.as_ref().ok_or_else(no_queue)
    }

    /// Every job, newest first.
    pub fn list(&self) -> Vec<JobSnapshot> {
        let mut jobs = self
            .queue
            .as_ref()
            .map(|queue| queue.list())
            .unwrap_or_default();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.created_at_ms));
        jobs
    }

    pub fn cancel(&self, id: &str) -> Result<JobSnapshot> {
        self.queue()?.cancel(id)
    }

    pub async fn pause(&self, id: &str) -> Result<JobSnapshot> {
        self.queue()?.pause(id).await
    }

    pub async fn resume(&self, id: &str) -> Result<JobSnapshot> {
        self.queue()?.resume(id).await
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        self.queue()?.remove(id)
    }

    pub async fn retry(&self, id: &str, candidate: Option<u32>) -> Result<JobSnapshot> {
        self.queue()?.retry(id, candidate).await
    }

    pub async fn answer(&self, id: &str, revision: u32, answer: JobAnswer) -> Result<JobSnapshot> {
        self.queue()?.answer(id, revision, answer).await
    }

    pub async fn recover(&self, id: &str, recovery: Recovery) -> Result<JobSnapshot> {
        self.queue()?.recover(id, recovery).await
    }

    /// Whether this app takes install requests (no confirmation without).
    pub fn takes_requests(&self) -> bool {
        self.requests.is_some()
    }

    fn requests(&self) -> Result<&Arc<dyn InstallInbox>> {
        self.requests.as_ref().ok_or_else(no_queue)
    }

    /// The install requests waiting for the user, oldest first.
    pub fn pending_requests(&self) -> Vec<InstallRequest> {
        self.requests
            .as_ref()
            .map(|requests| requests.pending())
            .unwrap_or_default()
    }

    pub async fn request_install(&self, value: String) -> Result<InstallRequest> {
        self.requests()?.request(value).await
    }

    pub async fn request_dropped(&self, path: String) -> Result<InstallRequest> {
        self.requests()?.dropped(path).await
    }

    pub async fn confirm_request(&self, id: &str, root_id: Option<String>) -> Result<JobSnapshot> {
        self.requests()?.confirm(id, root_id).await
    }

    pub async fn add_shared_collection(&self, id: &str) -> Result<()> {
        self.requests()?.add_collection(id).await
    }

    pub fn dismiss_request(&self, id: &str) -> Result<()> {
        self.requests()?.dismiss(id)
    }

    pub fn refresh_status(&self) -> Option<RefreshStatus> {
        self.refresh.as_ref().and_then(|refresh| refresh.status())
    }

    pub async fn refresh_start(&self, game_ids: Vec<String>, mode: RefreshMode) -> Result<()> {
        match &self.refresh {
            Some(refresh) => refresh.start(game_ids, mode).await,
            None => Err(no_queue()),
        }
    }

    pub fn refresh_cancel(&self) -> bool {
        self.refresh
            .as_ref()
            .is_some_and(|refresh| refresh.cancel())
    }
}

impl PartialEq for Jobs {
    /// One queue per app: a view's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Jobs a test puts in place, and what Activity asked of them: `cancel
/// <id>`, `pause <id>`, `resume <id>`, `remove <id>`, `retry <id> <candidate>`, `answer <id> <index>`,
/// `recover <id> <how>`. Every change is announced as
/// the install service announces its own.
pub struct FakeQueue {
    jobs: Mutex<Vec<JobSnapshot>>,
    calls: Mutex<Vec<String>>,
    events: Events,
}

impl FakeQueue {
    pub fn new(events: Events) -> Self {
        Self {
            jobs: Mutex::default(),
            calls: Mutex::default(),
            events,
        }
    }

    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts `job` in the queue, or replaces it, and announces it.
    pub fn put(&self, mut job: JobSnapshot) {
        {
            let mut jobs = Self::lock(&self.jobs);
            match jobs.iter_mut().find(|known| known.id == job.id) {
                Some(known) => {
                    job.revision = job.revision.max(known.revision + 1);
                    *known = job.clone();
                }
                None => jobs.push(job.clone()),
            }
        }
        self.events.send(Event::JobChanged(JobChanged(job)));
    }

    /// Empties the queue and what it was asked.
    pub fn clear(&self) {
        Self::lock(&self.jobs).clear();
        Self::lock(&self.calls).clear();
    }

    pub fn calls(&self) -> Vec<String> {
        Self::lock(&self.calls).clone()
    }

    fn called(&self, call: String) {
        Self::lock(&self.calls).push(call);
    }

    fn with(&self, id: &str, change: impl FnOnce(&mut JobSnapshot)) -> Result<JobSnapshot> {
        let mut job = Self::lock(&self.jobs)
            .iter()
            .find(|job| job.id == id)
            .cloned()
            .ok_or_else(|| caskade_core::tr!("This job is no longer in Activity."))?;
        change(&mut job);
        self.put(job.clone());
        Ok(Self::lock(&self.jobs)
            .iter()
            .find(|known| known.id == id)
            .cloned()
            .unwrap_or(job))
    }
}

#[async_trait]
impl JobQueue for FakeQueue {
    fn list(&self) -> Vec<JobSnapshot> {
        Self::lock(&self.jobs).clone()
    }
    fn cancel(&self, id: &str) -> Result<JobSnapshot> {
        self.called(format!("cancel {id}"));
        self.with(id, |job| job.status = JobStatus::Cancelled)
    }
    async fn pause(&self, id: &str) -> Result<JobSnapshot> {
        self.called(format!("pause {id}"));
        self.with(id, |job| job.status = JobStatus::Paused)
    }
    async fn resume(&self, id: &str) -> Result<JobSnapshot> {
        self.called(format!("resume {id}"));
        self.with(id, |job| job.status = JobStatus::Queued)
    }
    fn remove(&self, id: &str) -> Result<()> {
        self.called(format!("remove {id}"));
        Self::lock(&self.jobs).retain(|job| job.id != id);
        Ok(())
    }
    async fn retry(&self, id: &str, candidate: Option<u32>) -> Result<JobSnapshot> {
        let how = candidate.map_or("as before".to_owned(), |index| index.to_string());
        self.called(format!("retry {id} {how}"));
        self.with(id, |job| {
            job.status = JobStatus::Queued;
            job.failure = None;
            job.error = None;
        })
    }
    async fn answer(&self, id: &str, _revision: u32, answer: JobAnswer) -> Result<JobSnapshot> {
        let what = match answer {
            JobAnswer::Select { index } => index.to_string(),
            JobAnswer::Cancel => "cancel".to_owned(),
        };
        self.called(format!("answer {id} {what}"));
        self.with(id, |job| {
            job.question = None;
            job.status = JobStatus::Installing;
        })
    }
    async fn recover(&self, id: &str, recovery: Recovery) -> Result<JobSnapshot> {
        let how = serde_json::to_value(&recovery)
            .ok()
            .and_then(|value| {
                value
                    .get("type")
                    .and_then(|kind| kind.as_str().map(str::to_owned))
            })
            .unwrap_or_default();
        self.called(format!("recover {id} {how}"));
        self.with(id, |job| {
            job.status = JobStatus::Queued;
            job.failure = None;
        })
    }
}

/// A details refresh a test sets, and what Activity asked of it: `start
/// <ids> <mode>`, `cancel`.
pub struct FakeRefresh {
    status: Mutex<Option<RefreshStatus>>,
    calls: Mutex<Vec<String>>,
    events: Events,
}

impl FakeRefresh {
    pub fn new(events: Events) -> Self {
        Self {
            status: Mutex::default(),
            calls: Mutex::default(),
            events,
        }
    }

    /// Sets the status and announces it; `None` as if none ran.
    pub fn set(&self, status: Option<RefreshStatus>) {
        *FakeQueue::lock(&self.status) = status.clone();
        if let Some(status) = status {
            self.events
                .send(Event::GamesRefreshChanged(GamesRefreshChanged(status)));
        }
    }

    pub fn calls(&self) -> Vec<String> {
        FakeQueue::lock(&self.calls).clone()
    }

    pub fn clear(&self) {
        FakeQueue::lock(&self.calls).clear();
        *FakeQueue::lock(&self.status) = None;
    }
}

#[async_trait]
impl DetailsRefresh for FakeRefresh {
    fn status(&self) -> Option<RefreshStatus> {
        FakeQueue::lock(&self.status).clone()
    }
    async fn start(&self, game_ids: Vec<String>, mode: RefreshMode) -> Result<()> {
        let mode = serde_json::to_value(mode)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        FakeQueue::lock(&self.calls).push(format!("start {} {mode}", game_ids.join(",")));
        Ok(())
    }
    fn cancel(&self) -> bool {
        FakeQueue::lock(&self.calls).push("cancel".to_owned());
        true
    }
}
