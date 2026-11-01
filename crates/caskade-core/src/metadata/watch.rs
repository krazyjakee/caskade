//! [`Metadata::watch`]: runs after library changes and now and again, and
//! waits longer after each failed one.

use super::Metadata;
use crate::bus::Event;
use std::{
    hash::{BuildHasher, RandomState},
    time::Duration,
};

/// How often [`Metadata::watch`] looks for work without being asked.
const RECHECK: Duration = Duration::from_secs(60 * 60);
/// How soon it tries again after a failure, doubling with each failure in
/// a row up to [`MAX_RETRY`], or [`MAX_OFFLINE_RETRY`] when the run failed
/// to reach the server or an image host (offline, so it should notice the
/// network coming back soon).
const RETRY: Duration = Duration::from_secs(10 * 60);
pub(super) const MAX_RETRY: Duration = Duration::from_secs(6 * 60 * 60);
pub(super) const MAX_OFFLINE_RETRY: Duration = Duration::from_secs(60 * 60);
/// Library events come in bursts during a scan.
const SETTLE: Duration = Duration::from_secs(5);

impl Metadata {
    /// Runs now, after library changes, and every [`RECHECK`]; failures try
    /// again after a wait that grows with each failure in a row, and no
    /// sooner than the server allows.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        use tokio::sync::broadcast::error::{RecvError, TryRecvError};
        let this = self.clone();
        let mut events = self.inner.bus.stream();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(RECHECK);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut failures = 0u32;
            loop {
                tokio::select! {
                    _ = timer.tick() => {}
                    event = events.recv() => match event {
                        Ok(event) if relevant(&event) => tokio::time::sleep(SETTLE).await,
                        Ok(_) => continue,
                        Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => break,
                    },
                }
                // One run covers every event that arrived meanwhile.
                loop {
                    match events.try_recv() {
                        Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Closed) => return,
                    }
                }
                // `Some(offline)` after a failed run.
                let mut failed = match this.run().await {
                    Ok(report) => (report.deferred > 0).then_some(report.unreachable),
                    Err(error) => {
                        tracing::warn!(%error, "IGDB matching failed");
                        Some(false)
                    }
                };
                // Scores have their own daily TTL; sorting only reads the
                // last copy. Do this after matching so new ids are included.
                match this.inner.igdb.refresh_popularity(&this.inner.store).await {
                    Ok(true) => this.inner.bus.emit(Event::LibraryPopularityUpdated),
                    Ok(false) => {}
                    Err(error) => {
                        tracing::debug!(%error, "popularity refresh deferred");
                        failed = Some(true);
                    }
                }
                if let Some(offline) = failed {
                    failures = failures.saturating_add(1);
                    let cap = if offline {
                        MAX_OFFLINE_RETRY
                    } else {
                        MAX_RETRY
                    };
                    let random = RandomState::new().hash_one(failures);
                    let wait = retry_delay(failures, random, cap)
                        .max(this.inner.igdb.retry_in().unwrap_or_default());
                    tracing::debug!(failures, seconds = wait.as_secs(), "IGDB matching retry");
                    timer.reset_after(wait);
                } else if failures > 0 {
                    failures = 0;
                    timer.reset();
                }
            }
        })
    }
}

/// The wait after `failures` failed runs in a row: [`RETRY`], doubling up
/// to `cap`, give or take a fifth (from `random`) so clients that failed
/// together don't come back together.
pub(super) fn retry_delay(failures: u32, random: u64, cap: Duration) -> Duration {
    let doubled = RETRY.saturating_mul(1 << failures.saturating_sub(1).min(16));
    let wait = doubled.min(cap).as_secs_f64();
    // A fraction in [-0.2, 0.2).
    let jitter = (random % 4000) as f64 / 10_000.0 - 0.2;
    Duration::from_secs_f64(wait * (1.0 + jitter))
}

/// Events that bring games to match.
fn relevant(event: &Event) -> bool {
    matches!(
        event,
        Event::LibraryScanCompleted { .. } | Event::LibraryGameAdded { .. }
    )
}
