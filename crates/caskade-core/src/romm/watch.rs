//! The background syncs: servers with "sync on startup" at start, each
//! server with an interval when it is due, offline ones less and less often.
//! A server this device can't sign in to waits for a new password or token.

use super::{ConnectionState, RommService, lock, now_ms, sync::Trigger};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// How often the watcher looks for servers due for a sync, and the first
/// wait before an offline server is tried again.
pub(super) const CHECK: Duration = Duration::from_secs(5 * 60);
/// The longest wait between tries of an offline server.
const MAX_RETRY: Duration = Duration::from_secs(60 * 60);

/// The wait before trying an offline server again after `failures` failed
/// syncs in a row: [`CHECK`], doubling up to [`MAX_RETRY`].
fn retry_delay(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(8);
    CHECK.saturating_mul(1 << doublings).min(MAX_RETRY)
}

/// The watcher's failed tries of an offline server.
struct Backoff {
    failures: u32,
    next: Instant,
}

impl RommService {
    /// Syncs servers with "sync on startup" now, and each server with an
    /// interval when it is due. An offline server is tried again after
    /// [`CHECK`], then after twice as long each time up to [`MAX_RETRY`].
    /// A server without a saved password or token on this device, or whose
    /// server refused it, isn't tried until a new one is saved (which
    /// resets its state) or a sync of the user's gets through.
    /// Runs until the runtime stops.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut offline: HashMap<String, Backoff> = HashMap::new();
            if let Ok(settings) = this.settings().await {
                for server in settings
                    .servers
                    .iter()
                    .filter(|server| server.sync_on_startup)
                {
                    this.background_sync(&server.id, &mut offline).await;
                }
            }
            let mut timer = tokio::time::interval(this.inner.watch_every);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            timer.tick().await;
            loop {
                timer.tick().await;
                let Ok(settings) = this.settings().await else {
                    continue;
                };
                for server in &settings.servers {
                    let Some(interval) = server.interval() else {
                        continue;
                    };
                    if this.needs_sign_in(&server.id) {
                        continue;
                    }
                    let (last, is_offline) = {
                        let servers = lock(&this.inner.servers);
                        let state = servers.get(&server.id);
                        (
                            state.and_then(|state| state.last_sync_ms),
                            state
                                .is_some_and(|state| state.state == Some(ConnectionState::Offline)),
                        )
                    };
                    let due = if is_offline {
                        offline
                            .get(&server.id)
                            .is_none_or(|backoff| Instant::now() >= backoff.next)
                    } else {
                        // Back online (a launch or a sync of the user's).
                        offline.remove(&server.id);
                        last.is_none_or(|last| {
                            now_ms().saturating_sub(last)
                                >= i64::try_from(interval.as_millis()).unwrap_or(i64::MAX)
                        })
                    };
                    if due {
                        this.background_sync(&server.id, &mut offline).await;
                    }
                }
            }
        })
    }

    /// A sync of the watcher's. Failures while the server stays offline
    /// push its next try back and log quietly after the first. A server
    /// without a saved sign-in isn't asked at all, unless it answered
    /// without one this session.
    async fn background_sync(&self, server: &str, offline: &mut HashMap<String, Backoff>) {
        if !self.connected(server)
            && let Ok(config) = self.server_config(server).await
            && self.sign_in_missing(&config).await
        {
            offline.remove(server);
            self.set_state(server, |state| {
                state.state = Some(ConnectionState::SignInNeeded);
                state.error = None;
            });
            tracing::info!(
                server,
                "RomM server has no sign-in on this device; background syncs wait for one"
            );
            return;
        }
        let Err(error) = self.sync_as(server, Trigger::Background).await else {
            offline.remove(server);
            return;
        };
        if self.needs_sign_in(server) {
            offline.remove(server);
            tracing::warn!(server, %error, "RomM server refused the sign-in; background syncs wait for a new one");
            return;
        }
        if !self.offline(server) {
            offline.remove(server);
            tracing::warn!(server, %error, "RomM sync failed");
            return;
        }
        let failures = offline.get(server).map_or(0, |backoff| backoff.failures) + 1;
        let wait = retry_delay(failures);
        offline.insert(
            server.to_owned(),
            Backoff {
                failures,
                next: Instant::now() + wait,
            },
        );
        if failures == 1 {
            tracing::warn!(server, %error, retry_in_s = wait.as_secs(), "RomM server offline");
        } else {
            tracing::debug!(server, %error, failures, retry_in_s = wait.as_secs(), "RomM server still offline");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offline_server_is_tried_less_and_less_often() {
        let minutes: Vec<u64> = (1..=7).map(|n| retry_delay(n).as_secs() / 60).collect();
        assert_eq!(minutes, [5, 10, 20, 40, 60, 60, 60]);
        assert_eq!(retry_delay(u32::MAX), MAX_RETRY);
    }
}
