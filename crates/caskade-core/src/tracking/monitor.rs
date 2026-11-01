use super::{ProcessSnapshot, SystemProcesses};
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
