use async_trait::async_trait;
use caskade_core::bus::LibrarySource;
use caskade_discovery::{Config, HostEnv, ScanCtx, Source, SourceId, types::builtin_sources};
use std::{sync::Arc, time::Duration};

/// One provider for each discovery source family, including OS and deep
/// sources. The aggregate service scans/merges families together; this
/// provider preserves raw proxy records for bus consumers doing their own merge.
#[derive(Clone)]
pub struct DiscoverySource {
    id: SourceId,
    env: HostEnv,
    config: Config,
    pub(crate) source: Arc<dyn Source>,
}
impl DiscoverySource {
    pub fn new(env: HostEnv, config: Config, source: Arc<dyn Source>) -> Self {
        Self {
            id: source.id(),
            env,
            config,
            source,
        }
    }
}
#[async_trait]
impl LibrarySource for DiscoverySource {
    fn id(&self) -> &str {
        self.id.as_str()
    }
    async fn scan(&self) -> std::result::Result<Vec<caskade_discovery::DetectedGame>, String> {
        if !self.source.hosts().contains(self.env.os()) || !self.config.sources.includes(&self.id) {
            return Ok(Vec::new());
        }
        let (source, env, config) = (self.source.clone(), self.env.clone(), self.config.clone());
        let budget = Duration::from_millis(config.source_budget_ms.min(config.budget_ms));
        let cancel = caskade_discovery::Cancel::new();
        let token = cancel.clone();
        // Source parsing belongs on a blocking thread, never the async/UI
        // thread. Standalone providers enforce the same configured time bound;
        // the aggregate service additionally has source reports and cancellation.
        let result = caskade_jobs::global()
            .run("Scanning library source", async move {
                let task = caskade_jobs::blocking("Scanning library source", move || {
                    source.scan(&env, &ScanCtx::with_config(token, Arc::new(config)))
                });
                // Queue waiting time is not part of a source's execution budget.
                tokio::time::timeout(budget, task).await
            })
            .await;
        match result {
            Ok(result) => result
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string()),
            Err(_) => {
                cancel.cancel();
                Err("source timed out".into())
            }
        }
    }
}

pub(crate) struct StaticSource(&'static dyn Source);
impl Source for StaticSource {
    fn id(&self) -> SourceId {
        self.0.id()
    }
    fn hosts(&self) -> caskade_discovery::OsSet {
        self.0.hosts()
    }
    fn detect(&self, env: &HostEnv) -> Vec<caskade_discovery::LauncherStatus> {
        self.0.detect(env)
    }
    fn scan(
        &self,
        env: &HostEnv,
        ctx: &ScanCtx,
    ) -> std::result::Result<Vec<caskade_discovery::DetectedGame>, caskade_discovery::SourceError>
    {
        self.0.scan(env, ctx)
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<caskade_discovery::WatchPath> {
        self.0.watch_paths(env)
    }
    fn launcher_locations(&self, env: &HostEnv) -> Vec<std::path::PathBuf> {
        self.0.launcher_locations(env)
    }
}

pub(crate) fn sources() -> Vec<Arc<dyn Source>> {
    builtin_sources()
        .iter()
        .map(|s| Arc::new(StaticSource(*s)) as Arc<dyn Source>)
        .collect()
}
pub fn adapters(env: &HostEnv, config: &Config) -> Vec<Arc<DiscoverySource>> {
    sources()
        .into_iter()
        .map(|source| Arc::new(DiscoverySource::new(env.clone(), config.clone(), source)))
        .collect()
}
