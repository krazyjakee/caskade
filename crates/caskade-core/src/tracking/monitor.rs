use super::{ProcessIdentity, ProcessSnapshot, ProcessTracker, SystemProcesses};
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum TrackingError {
    #[error("process snapshot worker failed: {0}")]
    Worker(String),
}

/// Clones share one sysinfo cache, with filesystem/process reads off Tokio's
/// worker threads. Multiple game sessions reuse the same monitor safely.
#[derive(Clone, Default)]
pub struct ProcessMonitor {
    source: Arc<Mutex<SystemProcesses>>,
}
impl ProcessMonitor {
    pub async fn snapshot(&self) -> Result<ProcessSnapshot, TrackingError> {
        self.run(|source| source.snapshot()).await
    }

    /// A tracking poll: see [`SystemProcesses::snapshot_since`].
    pub async fn snapshot_since(&self, since: u64) -> Result<ProcessSnapshot, TrackingError> {
        self.run(move |source| source.snapshot_since(since)).await
    }

    async fn run(
        &self,
        read: impl FnOnce(&mut SystemProcesses) -> ProcessSnapshot + Send + 'static,
    ) -> Result<ProcessSnapshot, TrackingError> {
        let source = Arc::clone(&self.source);
        // A read that panicked leaves the cache usable: carry on with it
        // rather than failing every later launch until a restart (#160).
        tokio::task::spawn_blocking(move || {
            let mut source = crate::lock(&source);
            read(&mut source)
        })
        .await
        .map_err(|e| TrackingError::Worker(e.to_string()))
    }
}

/// A followed session's tracker, shared by the watch that polls it and by
/// Stop, which can't wait for the next poll (#22).
#[derive(Clone)]
pub struct LiveTracker {
    tracker: Arc<Mutex<ProcessTracker>>,
    since: u64,
}
impl LiveTracker {
    /// The baseline was taken just before `started_at_ms` (or before a
    /// recovery); only processes started since then can join the session.
    /// Two seconds of slack cover sysinfo's whole-second start times.
    pub fn new(tracker: ProcessTracker, started_at_ms: i64) -> Self {
        let since = u64::try_from(started_at_ms.min(crate::now_ms()) / 1000)
            .unwrap_or_default()
            .saturating_sub(2);
        Self {
            tracker: Arc::new(Mutex::new(tracker)),
            since,
        }
    }

    /// The session's processes running now, from a look at the system.
    pub async fn poll(
        &self,
        monitor: &ProcessMonitor,
    ) -> Result<Vec<ProcessIdentity>, TrackingError> {
        let snapshot = monitor.snapshot_since(self.since).await?;
        let tracker = Arc::clone(&self.tracker);
        // Path canonicalization may touch a disconnected mount.
        tokio::task::spawn_blocking(move || crate::lock(&tracker).observe(&snapshot))
            .await
            .map_err(|e| TrackingError::Worker(e.to_string()))
    }
}
