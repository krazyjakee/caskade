//! Watching and incremental rescans (§9, #99).
//!
//! [`Library`] keeps the last scan per source. Rescanning a few sources
//! merges all entries again and returns a [`ScanDelta`] against the previous
//! groups. [`Debouncer`] coalesces bursts of file events per source. The
//! `watch` feature adds [`watch`], which drives both from `notify` events
//! on every source's watch paths and polls sources without any (registry
//! sources) at the refresh interval.

mod diff;
#[cfg(feature = "watch")]
mod notify_watch;

pub use diff::diff;
#[cfg(feature = "watch")]
pub use notify_watch::{Watcher, watch, watch_with_sources};

use crate::{
    env::HostEnv,
    orchestrator::{Config, builtin, collect, group},
    types::{
        Cancel, DetectedGame, GameGroup, LauncherStatus, ScanDelta, ScanReport, Source, SourceId,
        SourceReport,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

/// Timing and resource bounds for [`watch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchOptions {
    /// Quiet time after a source's last event before it is rescanned.
    pub debounce: Duration,
    /// A source with a steady stream of events is rescanned at least this
    /// often.
    pub max_delay: Duration,
    /// How often sources without watch paths (registry sources) are polled.
    pub poll_interval: Duration,
    /// Most paths registered with the OS watcher; the rest are polled.
    pub max_paths: usize,
    /// Pending OS events; on overflow every source is rescanned.
    pub queue: usize,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(500),
            max_delay: Duration::from_secs(5),
            poll_interval: Duration::from_secs(300),
            max_paths: 1024,
            queue: 4096,
        }
    }
}

/// The last scan of every source, for incremental rescans.
pub struct Library {
    env: HostEnv,
    config: Config,
    sources: Vec<Arc<dyn Source>>,
    entries: BTreeMap<SourceId, Vec<DetectedGame>>,
    launchers: BTreeMap<SourceId, Vec<LauncherStatus>>,
    reports: BTreeMap<SourceId, SourceReport>,
    groups: Vec<GameGroup>,
}

impl Library {
    /// An empty library over the built-in sources.
    pub fn new(env: HostEnv, config: Config) -> Self {
        Self::with_sources(env, config, builtin())
    }

    /// An empty library over the given sources, in priority order.
    pub fn with_sources(env: HostEnv, config: Config, sources: Vec<Arc<dyn Source>>) -> Self {
        Self {
            env,
            config,
            sources,
            entries: BTreeMap::new(),
            launchers: BTreeMap::new(),
            reports: BTreeMap::new(),
            groups: Vec::new(),
        }
    }

    pub fn env(&self) -> &HostEnv {
        &self.env
    }

    pub fn sources(&self) -> &[Arc<dyn Source>] {
        &self.sources
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Scans every source; the delta against an empty library (or the
    /// previous state) is returned.
    pub fn scan(&mut self, cancel: &Cancel) -> crate::Result<ScanDelta> {
        let ids = self.sources.iter().map(|source| source.id()).collect();
        self.rescan(&ids, cancel)
    }

    /// Rescans only `ids`, merges every source's entries again and returns
    /// what changed.
    pub fn rescan(
        &mut self,
        ids: &BTreeSet<SourceId>,
        cancel: &Cancel,
    ) -> crate::Result<ScanDelta> {
        let selected: Vec<_> = self
            .sources
            .iter()
            .filter(|source| ids.contains(&source.id()))
            .cloned()
            .collect();
        let started = self.env.monotonic();
        // Sources look up volumes per path they report: read the list once.
        let env = &self.env.with_volume_snapshot();
        let collected = collect(env, &self.config, cancel, &selected);
        for (id, games) in collected.entries {
            self.entries.insert(id, games);
        }
        let mut launchers = Vec::new();
        for (id, statuses) in collected.launchers {
            launchers.extend(statuses.iter().cloned());
            self.launchers.insert(id, statuses);
        }
        for report in &collected.sources {
            self.reports.insert(report.source.clone(), report.clone());
        }
        // Merge in source priority order, as a full scan would.
        let entries = self
            .sources
            .iter()
            .filter_map(|source| self.entries.get(&source.id()))
            .flatten()
            .cloned()
            .collect();
        let groups = group(entries, &self.config, env)?;
        let (added, updated, removed) = diff(&self.groups, &groups);
        self.groups = groups;
        tracing::debug!(
            sources = ids.len(),
            added = added.len(),
            updated = updated.len(),
            removed = removed.len(),
            elapsed_ms = self.env.monotonic().saturating_sub(started).as_millis() as u64,
            "incremental rescan"
        );
        Ok(ScanDelta {
            added,
            updated,
            removed,
            sources: collected.sources,
            launchers,
        })
    }

    /// The current state as a full report.
    pub fn report(&self) -> ScanReport {
        ScanReport {
            groups: self.groups.clone(),
            launchers: self
                .sources
                .iter()
                .filter_map(|source| self.launchers.get(&source.id()))
                .flatten()
                .cloned()
                .collect(),
            sources: self
                .sources
                .iter()
                .filter_map(|source| self.reports.get(&source.id()))
                .cloned()
                .collect(),
        }
    }
}

/// Coalesces file events per source: a source is due `debounce` after its
/// last event, and never later than `max_delay` after its first pending
/// one. Times are durations on any monotonic clock.
#[derive(Debug, Clone)]
pub struct Debouncer {
    debounce: Duration,
    max_delay: Duration,
    /// Source → (first pending event, due time).
    pending: BTreeMap<SourceId, (Duration, Duration)>,
}

impl Debouncer {
    pub fn new(debounce: Duration, max_delay: Duration) -> Self {
        Self {
            debounce,
            max_delay: max_delay.max(debounce),
            pending: BTreeMap::new(),
        }
    }

    pub fn event(&mut self, source: SourceId, now: Duration) {
        let (first, _) = self.pending.get(&source).copied().unwrap_or((now, now));
        let due = (now + self.debounce).min(first + self.max_delay);
        self.pending.insert(source, (first, due));
    }

    /// Sources whose quiet time has passed; they are no longer pending.
    pub fn due(&mut self, now: Duration) -> BTreeSet<SourceId> {
        let due: BTreeSet<_> = self
            .pending
            .iter()
            .filter(|(_, (_, due))| *due <= now)
            .map(|(source, _)| source.clone())
            .collect();
        self.pending.retain(|source, _| !due.contains(source));
        due
    }

    /// When the next source becomes due.
    pub fn next_due(&self) -> Option<Duration> {
        self.pending.values().map(|(_, due)| *due).min()
    }

    pub fn is_idle(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn debouncer_coalesces_bursts_per_source() {
        let steam = SourceId::new("steam");
        let heroic = SourceId::new("heroic");
        let mut debouncer = Debouncer::new(ms(100), ms(1000));
        assert_eq!(debouncer.next_due(), None);
        debouncer.event(steam.clone(), ms(0));
        debouncer.event(steam.clone(), ms(50));
        debouncer.event(heroic.clone(), ms(60));
        assert!(debouncer.due(ms(149)).is_empty());
        assert_eq!(debouncer.next_due(), Some(ms(150)));
        assert_eq!(debouncer.due(ms(150)), BTreeSet::from([steam.clone()]));
        assert_eq!(debouncer.due(ms(160)), BTreeSet::from([heroic]));
        assert!(debouncer.is_idle());
        // A steady stream still fires after max_delay.
        for t in (0..2000).step_by(50) {
            debouncer.event(steam.clone(), ms(1000 + t));
            if t + 1000 >= 2000 {
                break;
            }
        }
        assert_eq!(debouncer.next_due(), Some(ms(2000)));
        assert_eq!(debouncer.due(ms(2000)), BTreeSet::from([steam]));
    }
}
