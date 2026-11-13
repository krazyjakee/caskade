//! Installer progress as job progress, throttled.

use super::*;
use std::time::{Duration, Instant};

/// How often a job's progress reaches the bus at most. The fetch engine
/// reports every network chunk (64 KiB) and extraction every file, so an
/// unthrottled sink would publish thousands of job snapshots a second.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// Installer progress as job progress, throttled to [`PROGRESS_INTERVAL`].
///
/// The first report, a phase or message change (`started`, `verifying`,
/// `completed`, …) and a report that reaches its total are published straight
/// away. Other reports inside the interval only replace the pending report,
/// which the next publish supersedes and dropping the sink flushes while the
/// job still runs, so the newest numbers are never lost.
pub(super) struct Progress {
    service: InstallService,
    job_id: String,
    throttle: Mutex<Throttle>,
}

#[derive(Default)]
struct Throttle {
    published_at: Option<Instant>,
    /// The phase and message of the last report, published or not.
    last: Option<(JobPhase, String)>,
    /// The phase whose completed report was published, so a stream of
    /// reports at their total (or a zero total) is still throttled.
    completed: Option<JobPhase>,
    pending: Option<(JobPhase, JobProgress)>,
}

impl Progress {
    pub(super) fn new(service: InstallService, job_id: String) -> Self {
        Self {
            service,
            job_id,
            throttle: Mutex::default(),
        }
    }

    fn publish(&self, phase: JobPhase, progress: JobProgress) {
        // Cheap when the plan set is unchanged: no snapshot is published.
        self.service.sync_plans(&self.job_id);
        self.service.update(&self.job_id, |job| {
            job.phase = Some(phase);
            job.progress = Some(progress.clone());
        });
        self.service.inner.bus.emit(Event::InstallProgress {
            job_id: self.job_id.clone(),
            phase,
            progress,
        });
    }
}

impl EventSink for Progress {
    fn emit(&self, event: JobEvent) {
        let phase = JobPhase::from(event.phase);
        let progress = JobProgress {
            bytes: event.bytes,
            total_bytes: event.total_bytes,
            files: event.files,
            eta_secs: event.eta_secs,
            message: event.message,
        };
        let now = Instant::now();
        {
            let mut throttle = lock(&self.throttle);
            let changed = throttle
                .last
                .as_ref()
                .is_none_or(|(last, message)| *last != phase || *message != progress.message);
            let complete = progress
                .total_bytes
                .is_some_and(|total| progress.bytes >= total);
            let completes = complete && throttle.completed != Some(phase);
            let due = throttle
                .published_at
                .is_none_or(|at| now.duration_since(at) >= PROGRESS_INTERVAL);
            if changed {
                throttle.last = Some((phase, progress.message.clone()));
            }
            if !(changed || completes || due) {
                throttle.pending = Some((phase, progress));
                return;
            }
            if complete {
                throttle.completed = Some(phase);
            }
            throttle.published_at = Some(now);
            throttle.pending = None;
        }
        self.publish(phase, progress);
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let pending = self
            .throttle
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
            .take();
        // A finished job keeps its last snapshot: its outcome is the news.
        let running = self
            .service
            .job(&self.job_id)
            .is_some_and(|job| !job.status.is_finished());
        if let Some((phase, progress)) = pending.filter(|_| running) {
            self.publish(phase, progress);
        }
    }
}
