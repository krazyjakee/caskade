use super::{ProcessIdentity, ProcessSnapshot, ProcessTracker, SystemProcesses};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

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

    /// A tracking poll: see [`SystemProcesses::snapshot_past`].
    pub async fn snapshot_past(
        &self,
        baseline: Arc<BTreeSet<ProcessIdentity>>,
    ) -> Result<ProcessSnapshot, TrackingError> {
        self.run(move |source| source.snapshot_past(&baseline))
            .await
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
    baseline: Arc<BTreeSet<ProcessIdentity>>,
}
impl LiveTracker {
    /// `tracker` as it is now: its launcher and recovered processes are in,
    /// and only what is outside its baseline can join the session.
    pub fn new(tracker: ProcessTracker) -> Self {
        Self {
            baseline: Arc::new(tracker.baseline().clone()),
            tracker: Arc::new(Mutex::new(tracker)),
        }
    }

    /// The session's processes running now, from a look at the system.
    pub async fn poll(
        &self,
        monitor: &ProcessMonitor,
    ) -> Result<Vec<ProcessIdentity>, TrackingError> {
        let snapshot = monitor.snapshot_past(self.baseline.clone()).await?;
        let tracker = Arc::clone(&self.tracker);
        // Path canonicalization may touch a disconnected mount.
        tokio::task::spawn_blocking(move || crate::lock(&tracker).observe(&snapshot))
            .await
            .map_err(|e| TrackingError::Worker(e.to_string()))
    }
}
