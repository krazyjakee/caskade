//! Progress from the actual source workers, without changing discovery's
//! merge order or making a slow source hold up the others' status updates.
use caskade_discovery::{
    DetectedGame, HostEnv, LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, WatchPath,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub enum DiscoveryProgress {
    Started(SourceId),
    Finished {
        source: SourceId,
        count: u32,
        error: Option<String>,
    },
}

pub(crate) struct ReportingSource {
    pub source: Arc<dyn Source>,
    pub report: Arc<dyn Fn(DiscoveryProgress) + Send + Sync>,
}
impl Source for ReportingSource {
    fn id(&self) -> SourceId {
        self.source.id()
    }
    fn hosts(&self) -> OsSet {
        self.source.hosts()
    }
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        (self.report)(DiscoveryProgress::Started(self.id()));
        self.source.detect(env)
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let result = self.source.scan(env, ctx);
        (self.report)(DiscoveryProgress::Finished {
            source: self.id(),
            count: result
                .as_ref()
                .map(|games| u32::try_from(games.len()).unwrap_or(u32::MAX))
                .unwrap_or(0),
            error: result.as_ref().err().map(ToString::to_string),
        });
        result
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        self.source.watch_paths(env)
    }
    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        self.source.launcher_locations(env)
    }
}
