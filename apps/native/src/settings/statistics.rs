//! Settings → Statistics (#156, #105), as the webview's `statistics/`:
//! this machine's CPU, GPUs, memory, network and storage, live. It watches
//! the core's sampler only while it is shown and the window is visible
//! (`useSystemStats`): a watch renewed every ten seconds, each sample as it
//! comes, staleness checked between samples.

use crate::data::sleep;
use std::time::Duration;

use caskade_system_stats::{
    DiskSample, GpuSample, GpuVendor, InterfaceKind, SystemSample, SystemStats, SystemStatsSnapshot,
};
use caskade_ui_i18n::format;
use caskade_ui_i18n::intl::Clock;
use caskade_ui_i18n::{t, tn, tp};
use dioxus_native::prelude::*;

use crate::ui::icons::{CIRCLE_SLASH, CPU, HARD_DRIVE, MEMORY_STICK, MONITOR_COG, NETWORK};
use crate::ui::{Icon, Svg};

mod cards;
mod graph;
use cards::{Frame, StatisticsDashboard};
use graph::{Graph, Series, nice_ceiling, percent_of, series};

/// How often a watch is renewed; the core lets one lapse after 30 s.
const RENEW: Duration = Duration::from_secs(10);
/// Samples older than this many intervals make the view stale.
const STALE_INTERVALS: i64 = 3;
/// At or above this a gauge says "High" and turns to the warning colour.
const HIGH: f64 = 90.0;

/// The app's sampler: a test's, else one for the app, made the first time.
pub fn use_system_stats() -> SystemStats {
    try_use_context::<SystemStats>().unwrap_or_else(|| {
        static APP: std::sync::OnceLock<SystemStats> = std::sync::OnceLock::new();
        APP.get_or_init(SystemStats::new).clone()
    })
}

/// What the dashboard shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Loading,
    Live,
    Stale,
    Error,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Live => "live",
            Self::Stale => "stale",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, PartialEq)]
struct Stats {
    /// Oldest first, at most `history`.
    samples: Vec<SystemSample>,
    interval_ms: i64,
    history: usize,
    /// Why the last watch failed.
    error: Option<String>,
    /// When the last sample came, Unix milliseconds.
    received_at: Option<i64>,
}

impl Stats {
    /// Adds `incoming` once each (by `seq`), oldest first, keeping
    /// `history`.
    fn merge(&mut self, incoming: Vec<SystemSample>) {
        for sample in incoming {
            match self
                .samples
                .iter_mut()
                .find(|known| known.seq == sample.seq)
            {
                Some(known) => *known = sample,
                None => self.samples.push(sample),
            }
        }
        self.samples.sort_by_key(|sample| sample.seq);
        let keep = self.history.max(1);
        if self.samples.len() > keep {
            self.samples.drain(..self.samples.len() - keep);
        }
    }

    fn status(&self, now: i64) -> Status {
        if self.samples.is_empty() {
            return if self.error.is_some() {
                Status::Error
            } else {
                Status::Loading
            };
        }
        match self.received_at {
            Some(at) if now - at <= self.interval_ms * STALE_INTERVALS => {
                if self.error.is_some() {
                    Status::Stale
                } else {
                    Status::Live
                }
            }
            _ => Status::Stale,
        }
    }
}

/// Settings → Statistics.
#[component]
pub fn StatisticsSettings() -> Element {
    let stats = use_system_stats();
    let mut state = use_signal(|| Stats {
        samples: Vec::new(),
        interval_ms: 1000,
        history: 60,
        error: None,
        received_at: None,
    });
    let mut now = use_signal(caskade_core::now_ms);
    let watcher = use_hook(|| {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        format!("native-stats-{}", NEXT.fetch_add(1, Ordering::Relaxed))
    });
    {
        let (stats, watcher) = (stats.clone(), watcher.clone());
        use_hook(move || {
            // Each sample as it comes.
            let mut samples = stats.subscribe();
            spawn(async move {
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    match samples.recv().await {
                        Ok(sample) => {
                            let mut current = state.write();
                            current.merge(vec![sample]);
                            current.received_at = Some(caskade_core::now_ms());
                        }
                        Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => break,
                    }
                }
            });
            // The watch, renewed while shown; stopped while the window is
            // hidden or minimized.
            spawn(async move {
                let mut watching = false;
                let mut fresh = true;
                let mut since_renew = Duration::ZERO;
                loop {
                    let shown = crate::shell::main_window_shown();
                    if shown && (!watching || since_renew >= RENEW) {
                        let history = fresh;
                        let (stats, watcher) = (stats.clone(), watcher.clone());
                        let answer =
                            caskade_app::runtime::spawn(
                                async move { stats.watch(&watcher, history) },
                            )
                            .await;
                        match answer {
                            Ok(SystemStatsSnapshot {
                                interval_ms,
                                history,
                                samples,
                            }) => {
                                fresh = false;
                                watching = true;
                                let mut current = state.write();
                                current.interval_ms = i64::from(interval_ms);
                                current.history = history as usize;
                                current.error = None;
                                if let Some(newest) = samples.last() {
                                    // A history is as fresh as its newest sample.
                                    current
                                        .received_at
                                        .get_or_insert(newest.at.min(caskade_core::now_ms()));
                                }
                                current.merge(samples);
                            }
                            Err(error) => {
                                fresh = true;
                                state.write().error = Some(error.to_string());
                            }
                        }
                        since_renew = Duration::ZERO;
                    } else if !shown && watching {
                        watching = false;
                        fresh = true;
                        stats.unwatch(&watcher);
                    }
                    let tick = Duration::from_millis(state.peek().interval_ms.max(250) as u64);
                    sleep(tick).await;
                    since_renew += tick;
                    // Re-checks staleness between samples.
                    now.set(caskade_core::now_ms());
                }
            });
        });
    }
    {
        let (stats, watcher) = (stats.clone(), watcher.clone());
        use_drop(move || stats.unwatch(&watcher));
    }
    let current = state.read().clone();
    rsx! {
        StatisticsDashboard { stats: current, now: now() }
    }
}

/// `60 s`, `2 min`.
fn window_label(ms: i64) -> String {
    let seconds = (ms as f64 / 1000.0).round() as i64;
    let minutes = (seconds as f64 / 60.0).round() as i64;
    if seconds < 120 {
        t!(
            note = "A span of time in seconds, abbreviated",
            "{seconds} s",
            seconds = seconds
        )
    } else {
        t!(
            note = "A span of time in minutes, abbreviated",
            "{minutes} min",
            minutes = minutes
        )
    }
}

fn bytes(value: u64) -> String {
    format::bytes(Some(value))
}

fn rate(bytes_per_second: Option<f64>) -> String {
    match bytes_per_second {
        None => "—".to_owned(),
        Some(rate) => t!(
            note = "A rate: bytes per second",
            "{size}/s",
            size = format::bytes(Some(rate.max(0.0) as u64))
        ),
    }
}

/// `error` as the end of a sentence.
fn clause(error: &str) -> String {
    error.trim().trim_end_matches('.').to_owned()
}

fn status_text(stats: &Stats, status: Status) -> String {
    let shown = window_label(stats.history as i64 * stats.interval_ms);
    match status {
        Status::Loading => t!("Waiting for the first reading…"),
        Status::Live if stats.interval_ms == 1000 => t!(
            "Live: updates every second, graphs show the last {span}.",
            span = shown
        ),
        Status::Live => t!(
            "Live: updates every {interval}, graphs show the last {span}.",
            interval = window_label(stats.interval_ms),
            span = shown
        ),
        Status::Stale => {
            let when = match stats.samples.last() {
                Some(latest) => format::time(
                    crate::library::local_date_time(latest.at),
                    Clock::System,
                    true,
                ),
                None => t!("a while ago"),
            };
            match &stats.error {
                None => t!(
                    "Not updating: the last reading is from {when}. Trying again.",
                    when = when
                ),
                Some(error) => t!(
                    note = "{reason} is the sampler's error",
                    "Not updating: the last reading is from {when}. {reason}. Trying again.",
                    when = when,
                    reason = clause(error)
                ),
            }
        }
        Status::Error => {
            let reason = clause(
                &stats
                    .error
                    .clone()
                    .unwrap_or_else(|| t!("the sampler didn't answer")),
            );
            t!(
                "Statistics aren't available: {reason}. Trying again.",
                reason = reason
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::graph::{nice_ceiling, percent_of, runs};

    #[test]
    fn rate_graphs_top_out_at_a_round_number() {
        assert_eq!(nice_ceiling(0.0), 1024.0);
        assert_eq!(nice_ceiling(3_000.0), 5_000.0);
        assert_eq!(nice_ceiling(12_000.0), 20_000.0);
        assert_eq!(percent_of(5, 0), None);
        assert_eq!(percent_of(50, 200), Some(25.0));
    }

    #[test]
    fn a_gap_or_a_missing_value_breaks_the_line() {
        let points = [
            (0, Some(1.0)),
            (1000, Some(2.0)),
            (2000, None),
            (3000, Some(3.0)),
            (9000, Some(4.0)),
        ];
        let lines = runs(&points, |at| at as f64, |value| value, 2500);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].len(), 2);
    }
}
