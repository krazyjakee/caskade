//! Settings → Statistics (#156): the core's system sampler. The dashboard
//! watches while it is open and visible, renews its watch every few
//! seconds, and gets each sample as a `SystemStatsSampled` event. Read-only:
//! nothing here changes the machine or the library.

use caskade_system_stats::{SystemSample, SystemStats, SystemStatsSnapshot};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{Event, Events};
/// Each sample, while anyone watches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SystemStatsSampled(pub SystemSample);

/// Starts or renews `watcher`'s watch (it lasts 30 s unless renewed) and
/// answers how sampling runs, with the history so far when `history`.
pub async fn system_stats_watch(
    stats: &SystemStats,
    watcher: String,
    history: bool,
) -> Result<SystemStatsSnapshot, String> {
    Ok(stats.watch(&watcher, history))
}

/// Ends `watcher`'s watch; sampling stops when no watch is left.
pub async fn system_stats_unwatch(stats: &SystemStats, watcher: String) -> Result<(), String> {
    stats.unwatch(&watcher);
    Ok(())
}

/// The statistics sampler, its samples sent to `events` while someone
/// watches.
pub fn setup(events: Events) -> SystemStats {
    let stats = SystemStats::new();
    let mut samples = stats.subscribe();
    crate::runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match samples.recv().await {
                Ok(sample) => events.send(Event::SystemStatsSampled(SystemStatsSampled(sample))),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
    stats
}
