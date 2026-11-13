//! Runs the selected sources in parallel so that no source can break or stall
//! the scan (#68).
//!
//! Sources share at most two workers (one on single-core hosts). Errors and panics
//! become `SourceReport` errors. When the caller cancels or a budget runs out,
//! the orchestrator cancels the source's context, reports it as cancelled or
//! timed out and stops waiting for it; a source that ignores its context is
//! left to finish on its own thread and its late result is dropped. Budgets
//! use real time; reported durations use the `HostEnv` clock, so fixture
//! scans (frozen clock) give byte-identical reports.

use crate::{
    env::HostEnv,
    types::{
        Cancel, Confidence, DetectedGame, ExternalId, GameGroup, LauncherStatus, ScanCtx,
        ScanReport, Source, SourceError, SourceId, SourceReport, builtin_sources,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    any::Any,
    collections::{BTreeSet, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

/// What to scan and how long to spend on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default)]
pub struct Config {
    pub sources: SourceSelection,
    pub deep: DeepScans,
    pub exclusions: Exclusions,
    /// Preferred metadata sources; others retain launcher/OS/deep class order.
    pub source_priority: Vec<SourceId>,
    pub classifier: ClassifierConfig,
    /// The whole scan.
    pub budget_ms: u64,
    /// One source; the whole-scan budget still applies.
    pub source_budget_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sources: SourceSelection::All,
            deep: DeepScans::default(),
            exclusions: Exclusions::default(),
            source_priority: Vec::new(),
            classifier: ClassifierConfig::default(),
            budget_ms: 10_000,
            source_budget_ms: 10_000,
        }
    }
}

/// Which sources run, mirroring the Settings toggles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SourceSelection {
    #[default]
    All,
    Only(BTreeSet<SourceId>),
    AllExcept(BTreeSet<SourceId>),
}

impl SourceSelection {
    pub fn only<I: IntoIterator<Item = SourceId>>(ids: I) -> Self {
        Self::Only(ids.into_iter().collect())
    }

    pub fn includes(&self, id: &SourceId) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.contains(id),
            Self::AllExcept(ids) => !ids.contains(id),
        }
    }
}

/// Opt-in deep scans (§7, #98). All off by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default)]
pub struct DeepScans {
    pub folders: Vec<PathBuf>,
    pub external_drives: bool,
    pub portable: bool,
    pub wine_prefixes: bool,
    pub full_disk: bool,
}

/// The user's exclusion list (§8.3), applied by the merge step (#69).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default)]
pub struct Exclusions {
    /// Globs matched against install dirs and launch programs.
    pub paths: Vec<String>,
    pub source_keys: Vec<SourceKey>,
    pub external_ids: Vec<ExternalId>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Type,
)]
pub struct SourceKey {
    pub source: SourceId,
    pub key: String,
}

/// Classifier settings (§6.7, #79).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default)]
pub struct ClassifierConfig {
    /// Results below this are dropped; `Maybe` keeps the review list.
    pub min_confidence: Confidence,
    /// Minimum evidence score for a review candidate.
    pub maybe_score: i32,
    /// Minimum evidence score for a likely game.
    pub likely_score: i32,
    /// Maximum folder entries read by one classification.
    pub max_entries: u64,
    /// Maximum directory descent below the candidate root.
    pub max_depth: u32,
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            min_confidence: Confidence::Maybe,
            maybe_score: 20,
            likely_score: 60,
            max_entries: 20_000,
            max_depth: 12,
        }
    }
}

/// Every install of every built-in launcher, in source priority order.
pub fn detect_launchers(env: &HostEnv) -> Vec<LauncherStatus> {
    detect_launchers_with_sources(env, &builtin())
}

/// `detect_launchers` over the given sources, in their order.
pub fn detect_launchers_with_sources(
    env: &HostEnv,
    sources: &[Arc<dyn Source>],
) -> Vec<LauncherStatus> {
    let budget = Duration::from_millis(Config::default().source_budget_ms);
    let runs = run_all(
        env,
        &host_sources(env, sources),
        Arc::default(),
        budget,
        budget,
        &Cancel::new(),
        |source, env, _| Ok(source.detect(env)),
    );
    runs.into_iter()
        .flat_map(|run| run.result.unwrap_or_default())
        .collect()
}

/// Scans the built-in sources the config selects.
pub fn scan(env: &HostEnv, config: &Config, cancel: &Cancel) -> crate::Result<ScanReport> {
    scan_with_sources(env, config, cancel, &builtin())
}

/// `scan` over the given sources (fakes, replayed fixtures). Their order is
/// the merge priority order.
pub fn scan_with_sources(
    env: &HostEnv,
    config: &Config,
    cancel: &Cancel,
    sources: &[Arc<dyn Source>],
) -> crate::Result<ScanReport> {
    // Sources look up volumes per path they report: read the list once.
    let env = &env.with_volume_snapshot();
    let collected = collect(env, config, cancel, sources);
    let entries = collected
        .entries
        .into_iter()
        .flat_map(|(_, games)| games)
        .collect();
    Ok(ScanReport {
        groups: group(entries, config, env)?,
        launchers: collected
            .launchers
            .into_iter()
            .flat_map(|(_, launchers)| launchers)
            .collect(),
        sources: collected.sources,
    })
}

/// What the selected sources returned, before merging; one item per
/// source, in source order.
pub(crate) struct Collected {
    pub sources: Vec<SourceReport>,
    pub launchers: Vec<(SourceId, Vec<LauncherStatus>)>,
    pub entries: Vec<(SourceId, Vec<DetectedGame>)>,
}

/// Runs the sources that the config selects and that run on this host.
pub(crate) fn collect(
    env: &HostEnv,
    config: &Config,
    cancel: &Cancel,
    sources: &[Arc<dyn Source>],
) -> Collected {
    let selected: Vec<_> = host_sources(env, sources)
        .into_iter()
        .filter(|source| config.sources.includes(&source.id()))
        .collect();
    let runs = run_all(
        env,
        &selected,
        Arc::new(config.clone()),
        Duration::from_millis(config.budget_ms),
        Duration::from_millis(config.source_budget_ms),
        cancel,
        |source, env, ctx| {
            let launchers = source.detect(env);
            ctx.check()?;
            Ok((launchers, source.scan(env, ctx)))
        },
    );
    let mut collected = Collected {
        sources: Vec::new(),
        launchers: Vec::new(),
        entries: Vec::new(),
    };
    for run in runs {
        // A scan error keeps the launchers the source detected.
        let (launchers, mut games, error) = match run.result {
            Ok((launchers, Ok(games))) => (launchers, games, run.error),
            Ok((launchers, Err(error))) => {
                tracing::warn!(source = %run.id, %error, "source failed");
                (launchers, Vec::new(), Some(error))
            }
            Err(()) => (Vec::new(), Vec::new(), run.error),
        };
        games.sort_by(|a, b| (&a.key, &a.title).cmp(&(&b.key, &b.title)));
        collected.sources.push(SourceReport {
            source: run.id.clone(),
            duration_ms: run.duration_ms,
            count: u32::try_from(games.len()).unwrap_or(u32::MAX),
            warnings: run.warnings,
            error,
        });
        collected.launchers.push((run.id.clone(), launchers));
        collected.entries.push((run.id, games));
    }
    collected
}

pub(crate) fn group(
    entries: Vec<DetectedGame>,
    config: &Config,
    env: &HostEnv,
) -> crate::Result<Vec<GameGroup>> {
    crate::merge::merge(
        env,
        entries,
        &crate::merge::MergeConfig {
            source_priority: config.source_priority.clone(),
            exclusions: config.exclusions.clone(),
            min_confidence: config.classifier.min_confidence,
        },
    )
}

struct StaticSource(&'static dyn Source);

impl Source for StaticSource {
    fn id(&self) -> SourceId {
        self.0.id()
    }
    fn hosts(&self) -> crate::types::OsSet {
        self.0.hosts()
    }
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        self.0.detect(env)
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        self.0.scan(env, ctx)
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<crate::types::WatchPath> {
        self.0.watch_paths(env)
    }
    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        self.0.launcher_locations(env)
    }
}

/// The built-in sources, in priority order.
pub fn builtin() -> Vec<Arc<dyn Source>> {
    builtin_sources()
        .iter()
        .map(|source| Arc::new(StaticSource(*source)) as Arc<dyn Source>)
        .collect()
}

/// Sources that run natively on the env's OS, first of each id only.
pub(crate) fn host_sources(env: &HostEnv, sources: &[Arc<dyn Source>]) -> Vec<Arc<dyn Source>> {
    let mut seen = BTreeSet::new();
    sources
        .iter()
        .filter(|source| source.hosts().contains(env.os()) && seen.insert(source.id()))
        .cloned()
        .collect()
}

struct Run<T> {
    id: SourceId,
    result: Result<T, ()>,
    error: Option<SourceError>,
    warnings: Vec<String>,
    duration_ms: u64,
}

enum SourceMessage<T> {
    Started(usize, Instant),
    Finished(usize, Result<T, SourceError>, Vec<String>, Duration),
}

const POLL: Duration = Duration::from_millis(10);

/// Runs `work` on a bounded worker pool and returns one `Run` per
/// source, in the given order.
fn run_all<T, F>(
    env: &HostEnv,
    sources: &[Arc<dyn Source>],
    config: Arc<Config>,
    budget: Duration,
    source_budget: Duration,
    cancel: &Cancel,
    work: F,
) -> Vec<Run<T>>
where
    T: Send + 'static,
    F: Fn(&dyn Source, &HostEnv, &ScanCtx) -> Result<T, SourceError> + Send + Sync + 'static,
{
    let started = Instant::now();
    let env_started = env.monotonic();
    let deadline = started + budget;
    let work = Arc::new(work);
    let (sender, receiver) = mpsc::channel();
    let mut pending: Vec<Option<(Arc<ScanCtx>, Cancel)>> = Vec::with_capacity(sources.len());
    let mut runs: Vec<Option<Run<T>>> = sources.iter().map(|_| None).collect();
    let mut source_started = vec![None; sources.len()];
    let mut jobs = VecDeque::new();
    for (index, source) in sources.iter().enumerate() {
        let source_cancel = Cancel::new();
        let ctx = Arc::new(ScanCtx::with_config(source_cancel.clone(), config.clone()));
        pending.push(Some((ctx.clone(), source_cancel)));
        jobs.push_back((index, source.clone(), ctx));
    }
    let jobs = Arc::new(Mutex::new(jobs));
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(2);
    for worker in 0..workers.min(sources.len()) {
        let (jobs, env, work, sender) = (jobs.clone(), env.clone(), work.clone(), sender.clone());
        let spawned = std::thread::Builder::new()
            .name(format!("discovery-worker-{worker}"))
            .spawn(caskade_jobs::inherit(move || {
                loop {
                    let job = jobs.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                    let Some((index, source, ctx)) = job else {
                        break;
                    };
                    if ctx.is_cancelled() {
                        continue;
                    }
                    let _ = sender.send(SourceMessage::Started(index, Instant::now()));
                    let id = source.id();
                    let span = tracing::info_span!(
                        "source", source = %id, duration_ms = tracing::field::Empty
                    );
                    let _guard = span.enter();
                    let start = env.monotonic();
                    let result = catch_unwind(AssertUnwindSafe(|| work(&*source, &env, &ctx)))
                        .unwrap_or_else(|panic| {
                            Err(SourceError::Panicked {
                                message: panic_message(&*panic),
                            })
                        });
                    let elapsed = env.monotonic().saturating_sub(start);
                    span.record("duration_ms", millis(elapsed));
                    tracing::debug!(ok = result.is_ok(), "source finished");
                    let _ = sender.send(SourceMessage::Finished(
                        index,
                        result,
                        ctx.take_warnings(),
                        elapsed,
                    ));
                }
            }));
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start discovery worker");
        }
    }
    drop(sender);
    let mut disconnected = false;
    while pending.iter().any(Option::is_some) {
        // Read every message already sent before judging deadlines: a source
        // that finished in time stays finished even when this thread was
        // descheduled and reads its result late.
        let mut next = receiver.recv_timeout(POLL);
        loop {
            match next {
                Ok(SourceMessage::Started(index, at)) => source_started[index] = Some(at),
                Ok(SourceMessage::Finished(index, result, warnings, elapsed)) => {
                    if pending[index].take().is_some() {
                        let (result, error) = match result {
                            Ok(value) => (Ok(value), None),
                            Err(error) => {
                                tracing::warn!(source = %sources[index].id(), %error, "source failed");
                                (Err(()), Some(error))
                            }
                        };
                        runs[index] = Some(Run {
                            id: sources[index].id(),
                            result,
                            error,
                            warnings,
                            duration_ms: millis(elapsed),
                        });
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
            next = receiver.try_recv().map_err(|error| match error {
                mpsc::TryRecvError::Empty => mpsc::RecvTimeoutError::Timeout,
                mpsc::TryRecvError::Disconnected => mpsc::RecvTimeoutError::Disconnected,
            });
        }
        if disconnected {
            break;
        }
        let now = Instant::now();
        for (index, slot) in pending.iter_mut().enumerate() {
            // A source gets its execution budget once a worker starts it.
            // Queued sources only consume the whole-scan budget. A cooperative
            // source releases its worker on timeout so later sources still run.
            let reason = if cancel.is_cancelled() {
                Some(SourceError::Cancelled)
            } else if now >= deadline
                || source_started[index].is_some_and(|at| now.duration_since(at) >= source_budget)
            {
                Some(SourceError::TimedOut)
            } else {
                None
            };
            if let Some(reason) = reason
                && let Some((ctx, source_cancel)) = slot.take()
            {
                source_cancel.cancel();
                tracing::warn!(source = %sources[index].id(), error = %reason, "source stopped");
                runs[index] = Some(Run {
                    id: sources[index].id(),
                    result: Err(()),
                    error: Some(reason.clone()),
                    warnings: ctx.take_warnings(),
                    duration_ms: millis(env.monotonic().saturating_sub(env_started)),
                });
            }
        }
    }
    runs.into_iter()
        .zip(sources)
        .map(|(run, source)| {
            run.unwrap_or_else(|| Run {
                id: source.id(),
                result: Err(()),
                error: Some(SourceError::Other {
                    message: "source thread ended without a result".into(),
                }),
                warnings: Vec::new(),
                duration_ms: 0,
            })
        })
        .collect()
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn panic_message(panic: &(dyn Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic".into())
}
