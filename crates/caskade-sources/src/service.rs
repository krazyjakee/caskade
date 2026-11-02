use crate::{ImportOptions, ImportOutcome, Result};
use caskade_core::{
    bus::{Bus, PLUGIN_SOURCE_PREFIX},
    db::LibraryStore,
};
use caskade_discovery::{
    Cancel, Config, HostEnv, ScanReport, Source, SourceError, SourceId, SourceReport,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

#[derive(Clone)]
pub struct DiscoveryService {
    pub(crate) store: LibraryStore,
    pub(crate) bus: Bus,
    pub(crate) env: HostEnv,
    pub(crate) sources: Vec<Arc<dyn Source>>,
    pub(crate) imports: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub scan: ScanReport,
    pub import: ImportOutcome,
}
impl DiscoveryService {
    pub fn new(store: LibraryStore, bus: Bus, env: HostEnv) -> Self {
        Self::with_sources(store, bus, env, crate::adapter::sources())
    }
    pub fn with_sources(
        store: LibraryStore,
        bus: Bus,
        env: HostEnv,
        sources: Vec<Arc<dyn Source>>,
    ) -> Self {
        Self {
            store,
            bus,
            env,
            sources,
            imports: Arc::default(),
        }
    }
    pub fn env(&self) -> &HostEnv {
        &self.env
    }
    pub async fn scan(
        &self,
        config: &Config,
        options: &ImportOptions,
        cancel: &Cancel,
    ) -> Result<ScanOutcome> {
        let mut scan = self.discover(config, cancel).await?;
        self.add_plugin_games(&mut scan, config).await?;
        let import = self.import_report(&scan, options, cancel).await?;
        self.remove_unscanned(config, cancel).await?;
        Ok(ScanOutcome { scan, import })
    }
    /// Read-only discovery using this service's injected environment and sources.
    pub async fn discover(&self, config: &Config, cancel: &Cancel) -> Result<ScanReport> {
        self.discover_with_progress(config, cancel, |_| {}).await
    }
    /// Reports each selected source as its worker detects and scans it.
    /// Callbacks run on discovery worker threads and must not block.
    pub async fn discover_with_progress(
        &self,
        config: &Config,
        cancel: &Cancel,
        report: impl Fn(crate::DiscoveryProgress) + Send + Sync + 'static,
    ) -> Result<ScanReport> {
        let report = Arc::new(report);
        let sources: Vec<Arc<dyn Source>> = self
            .sources
            .iter()
            .map(|source| {
                Arc::new(crate::progress::ReportingSource {
                    source: source.clone(),
                    report: report.clone(),
                }) as Arc<dyn Source>
            })
            .collect();
        let (env, config, sources, token) =
            (self.env.clone(), config.clone(), sources, cancel.clone());
        Ok(caskade_jobs::blocking("Scanning library", move || {
            caskade_discovery::scan_with_sources(&env, &config, &token, &sources)
        })
        .await??)
    }
    /// Adds the games of plugin library sources (#126) to a scan, merged
    /// with the same engine as built-in sources, so they go through the
    /// same import path. A failing plugin is reported, never fatal.
    pub async fn add_plugin_games(&self, scan: &mut ScanReport, config: &Config) -> Result<()> {
        let providers: Vec<_> = self
            .bus
            .library_sources()
            .into_iter()
            .filter(|provider| provider.id().starts_with(PLUGIN_SOURCE_PREFIX))
            .collect();
        if providers.is_empty() {
            return Ok(());
        }
        let mut entries = Vec::new();
        for provider in providers {
            let started = std::time::Instant::now();
            let source = SourceId::from(provider.id().to_owned());
            let (games, error) = match provider.scan().await {
                Ok(games) => (games, None),
                Err(message) => (Vec::new(), Some(SourceError::Other { message })),
            };
            scan.sources.push(SourceReport {
                source: source.clone(),
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                count: u32::try_from(games.len()).unwrap_or(u32::MAX),
                warnings: Vec::new(),
                error,
            });
            // A plugin speaks only for itself.
            entries.extend(games.into_iter().map(|mut game| {
                game.source = source.clone();
                game
            }));
        }
        let env = self.env.clone();
        let merge = caskade_discovery::merge::MergeConfig {
            exclusions: config.exclusions.clone(),
            ..Default::default()
        };
        let groups = caskade_jobs::blocking("Merging library sources", move || {
            caskade_discovery::merge::merge(&env, entries, &merge)
        })
        .await??;
        scan.groups.extend(groups);
        Ok(())
    }

    /// Launcher detection shares the same fixture/live boundary as scanning.
    pub async fn detect_launchers(&self) -> Result<Vec<caskade_discovery::LauncherStatus>> {
        let (env, sources) = (self.env.clone(), self.sources.clone());
        Ok(caskade_jobs::blocking("Detecting launchers", move || {
            caskade_discovery::detect_launchers_with_sources(&env, &sources)
        })
        .await?)
    }
}

/// Register one bus provider per source family and return the aggregate
/// import service. The caller owns its scan/watch lifecycle and settings.
pub fn register(bus: &Bus, store: LibraryStore, env: HostEnv, config: &Config) -> DiscoveryService {
    let sources = crate::adapters(&env, config);
    for source in &sources {
        bus.register_library_source(source.clone());
    }
    DiscoveryService::with_sources(
        store,
        bus.clone(),
        env,
        sources.into_iter().map(|s| s.source.clone()).collect(),
    )
}
