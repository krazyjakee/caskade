//! Following a started or recovered session until its game exits.
use super::{GameLease, LaunchHandle, LaunchService, event, millis, store_error, ticket};
use crate::{
    launch::{LaunchError, Result, SessionStatus},
    model::PlaySession,
    now_ms,
    sessions::{RecoveryState, SessionPhase},
    tracking::{ProcessTracker, TrackingMode},
};
use std::time::{Duration, Instant};
use tokio::{process::Child, sync::oneshot};

impl LaunchService {
    /// Called once at app startup before accepting launch commands.
    pub async fn recover(&self) -> Result<Vec<LaunchHandle>> {
        let sessions = self.repository.in_flight().await?;
        let snapshot = self.monitor.snapshot().await?;
        let mut handles = vec![];
        for mut session in sessions {
            let lease = match self.try_game_lease(&session.game_id) {
                Ok(lease) => lease,
                Err(LaunchError::AlreadyRunning) => continue,
                Err(e) => return Err(e),
            };
            let Some(mut state) = super::recovery(&session).filter(|s| s.version == 1) else {
                self.interrupted(&mut session).await?;
                continue;
            };
            let mut tracker = ProcessTracker::new(state.target.clone(), &snapshot);
            tracker.recover(&state.processes);
            if let Some(launcher) = state.launcher {
                tracker.launcher(launcher);
            }
            let (tracker, active) = tracker.observe_async(snapshot.clone()).await?;
            if active.is_empty() {
                self.interrupted(&mut session).await?;
                continue;
            }
            if state.phase == SessionPhase::Playing {
                // An exact identity still alive proves this game survived the
                // restart, so elapsed downtime is known rather than guessed.
                session.duration_ms = session
                    .duration_ms
                    .saturating_add(now_ms().saturating_sub(session.heartbeat_at_ms).max(0) as u64);
            }
            state.processes = active;
            handles.push(self.follow(Watch {
                session,
                state,
                tracker,
                child: None,
                direct: false,
                lease,
                resumed: true,
                requested: Instant::now(),
            }));
        }
        Ok(handles)
    }

    async fn interrupted(&self, session: &mut PlaySession) -> Result<()> {
        session.ended_at_ms = Some(session.heartbeat_at_ms.max(session.started_at_ms));
        self.repository.finish(session).await?;
        // Startup waits for recovery, never for scripts.
        self.ended(session, SessionStatus::Interrupted, true).await;
        Ok(())
    }

    /// Watches the session on its own task; the handle hears how it ended.
    pub(super) fn follow(&self, watch: Watch) -> LaunchHandle {
        let ticket = ticket(&watch.session);
        let (tx, rx) = oneshot::channel();
        let service = self.clone();
        tokio::spawn(async move {
            let _ = tx.send(service.watch(watch).await);
        });
        LaunchHandle {
            ticket,
            completed: rx,
        }
    }

    async fn watch(&self, watch: Watch) -> Result<(PlaySession, SessionStatus)> {
        let Watch {
            mut session,
            mut state,
            mut tracker,
            mut child,
            direct,
            lease,
            resumed,
            requested,
        } = watch;
        let mut started = None;
        let mut last_seen = None;
        let mut last_elapsed = Duration::ZERO;
        let base_duration = session.duration_ms;
        let mut last_heartbeat = Instant::now();
        let mut child_status = None;
        let mut child_ended = None;
        // The baseline was taken just before `started_at_ms` (or before this
        // recovery); only processes started since then can join the session.
        // Two seconds of slack cover sysinfo's whole-second start times.
        let since = u64::try_from(session.started_at_ms.min(now_ms()) / 1000)
            .unwrap_or_default()
            .saturating_sub(2);
        if state.phase == SessionPhase::Playing {
            started = Some(Instant::now());
            last_seen = started;
            if resumed {
                self.hooks
                    .started(&event(&session, SessionStatus::Running))
                    .await;
            }
        }
        loop {
            if lease.cancel.is_cancelled() {
                if let Some(mut child) = child.take() {
                    tokio::spawn(async move {
                        let _ = child.wait().await;
                    });
                }
                return Err(LaunchError::Cancelled);
            }
            if let Some(process) = child.as_mut()
                && let Some(status) = process.try_wait()?
            {
                child_status = Some(status);
                child_ended = Some(Instant::now());
                child = None;
            }
            let snapshot = self.monitor.snapshot_since(since).await?;
            let (updated, active) = tracker.observe_async(snapshot).await?;
            tracker = updated;
            let alive = !active.is_empty() || (direct && child.is_some());
            // Very short direct executables (including fixture-game) can exit
            // before the first sysinfo poll. The child handle is proof of launch.
            if started.is_none() && (alive || (direct && child_status.is_some())) {
                session.started_at_ms = if direct {
                    session.started_at_ms
                } else {
                    now_ms()
                };
                started = Some(if direct { requested } else { Instant::now() });
                state.phase = SessionPhase::Playing;
                state.processes = active.clone();
                // After-start scripts run once per session, even when a
                // restart finds the game again (#179).
                let after_start = state.scripts.as_mut().filter(|s| !s.started).map(|s| {
                    s.started = true;
                    s.context.pid = active.first().map(|process| process.pid);
                    s.context.clone()
                });
                session.tracking = Some(serde_json::to_value(&state).map_err(store_error)?);
                session.heartbeat_at_ms = now_ms();
                self.repository.activate(&session).await?;
                self.hooks
                    .started(&event(&session, SessionStatus::Running))
                    .await;
                if let Some(context) = after_start {
                    self.after_start(context, lease.cancel.clone());
                }
            }
            if let Some(started) = started {
                if alive {
                    last_seen = Some(Instant::now());
                    last_elapsed = started.elapsed();
                    state.processes = active;
                } else if direct && let Some(ended) = child_ended {
                    let ended = last_seen.map_or(ended, |seen| seen.max(ended));
                    last_seen = Some(ended);
                    last_elapsed = ended.saturating_duration_since(started);
                }
                session.duration_ms = base_duration.saturating_add(millis(last_elapsed));
                // When only the started process is the game, its exit ends
                // the game: no other process can take over, so there is no
                // handoff to wait for.
                let ended = direct
                    && child_status.is_some()
                    && state.target.mode == TrackingMode::OriginalProcess;
                if !alive
                    && (ended
                        || last_seen.is_none_or(|seen| seen.elapsed() >= self.config.exit_grace))
                {
                    let unseen = millis(last_seen.map(|s| s.elapsed()).unwrap_or_default())
                        .min(i64::MAX as u64) as i64;
                    session.ended_at_ms =
                        Some(now_ms().saturating_sub(unseen).max(session.started_at_ms));
                    let status = if direct && child_status.is_some_and(|s| !s.success()) {
                        SessionStatus::Crashed
                    } else {
                        SessionStatus::Exited
                    };
                    self.repository.finish(&session).await?;
                    self.ended(&session, status, false).await;
                    return Ok((session, status));
                }
            } else if requested.elapsed() >= self.config.launch_timeout {
                session.ended_at_ms = Some(now_ms());
                self.repository.finish(&session).await?;
                self.ended(&session, SessionStatus::NotStarted, false).await;
                return Err(LaunchError::NotStarted);
            }
            if last_heartbeat.elapsed() >= self.config.heartbeat_interval {
                session.heartbeat_at_ms = now_ms();
                session.tracking = Some(serde_json::to_value(&state).map_err(store_error)?);
                self.repository.heartbeat(&session).await?;
                last_heartbeat = Instant::now();
            }
            // Poll fast until the game is found and while it may be exiting,
            // so start and exit times stay accurate; back off while it runs.
            let interval = if started.is_some() && alive {
                self.config.playing_poll_interval()
            } else {
                self.config.poll_interval
            };
            // A direct child's exit wakes the loop at once (`wait` is cancel
            // safe, and `try_wait` above then reports the kept status).
            let child_exit = async {
                match child.as_mut() {
                    Some(process) => {
                        let _ = process.wait().await;
                    }
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                _ = lease.cancel.cancelled() => {}
                _ = child_exit => {}
                _ = tokio::time::sleep(interval.max(Duration::from_millis(1))) => {}
            }
        }
    }
}

/// A session to follow, from a launch or from recovery.
pub(super) struct Watch {
    pub session: PlaySession,
    pub state: RecoveryState,
    pub tracker: ProcessTracker,
    /// The process Caskade started, when this launch started one.
    pub child: Option<Child>,
    /// The started process is the game (`PreparedLaunch::direct`).
    pub direct: bool,
    pub lease: GameLease,
    /// Recovered after a restart: announce `game.started` again.
    pub resumed: bool,
    /// When the launch was asked for; a direct game's start time.
    pub requested: Instant,
}
