use crate::{DiscoveryService, ImportOptions, ImportOutcome, Result};
use caskade_discovery::{
    Cancel, Config,
    watch::{WatchOptions, Watcher},
};
use tokio::sync::{mpsc, watch};

/// Owns discovery's OS watcher and a serialized async import worker. Status
/// retains the most recent import/error for settings and progress consumers.
pub struct DiscoveryWatch {
    watcher: Watcher,
    worker: tokio::task::JoinHandle<()>,
    pub status: watch::Receiver<Option<std::result::Result<ImportOutcome, String>>>,
}
impl DiscoveryWatch {
    pub async fn stop(self) {
        let Self {
            watcher, worker, ..
        } = self;
        let _ = tokio::task::spawn_blocking(move || watcher.stop()).await;
        let _ = worker.await;
    }
}
impl DiscoveryService {
    /// The watcher emits an initial delta and subsequent incremental changes.
    /// Use `scan` before starting it when restoring an existing DB, so absent
    /// entries can be reconciled against the complete successful source scan.
    pub async fn watch(
        &self,
        config: Config,
        options: ImportOptions,
        watch_options: WatchOptions,
    ) -> Result<DiscoveryWatch> {
        let (sender, mut receiver) = mpsc::channel(64);
        let (status, updates) = watch::channel(None);
        let service = self.clone();
        let worker = tokio::spawn(async move {
            while let Some(delta) = receiver.recv().await {
                let result = service
                    .apply_delta(&delta, &options, &Cancel::new())
                    .await
                    .map_err(|e| e.to_string());
                status.send_replace(Some(result));
            }
        });
        let (env, sources) = (self.env.clone(), self.sources.clone());
        let watcher = caskade_jobs::blocking("Starting library watch", move || {
            caskade_discovery::watch::watch_with_sources(
                env,
                config,
                watch_options,
                sources,
                move |delta| {
                    let _ = sender.blocking_send(delta);
                },
            )
        })
        .await??;
        Ok(DiscoveryWatch {
            watcher,
            worker,
            status: updates,
        })
    }
}
