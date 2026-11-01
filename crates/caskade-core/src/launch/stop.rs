//! Stopping a running game (#149, the guide quick menu's "Stop game").
//!
//! Only the game processes tracking recorded for the session are touched,
//! matched by PID and start time so a reused PID is never hit. The launcher
//! is left alone: it can be a store client (Steam) that must keep running.
//! The tracker sees the processes exit and ends the session as usual, so
//! playtime is recorded.

use super::{LaunchError, LaunchService, Result};
use crate::{sessions::RecoveryState, tracking::ProcessIdentity};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, Signal, System};

impl LaunchService {
    /// Asks the session's game processes to quit, and ends any still
    /// running after `grace`. Returns how many were running.
    pub async fn stop(&self, session_id: &str, grace: Duration) -> Result<usize> {
        let session = self
            .repository
            .in_flight()
            .await?
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| {
                LaunchError::Invalid(crate::tr!(
                    "no running session {session}",
                    session = session_id
                ))
            })?;
        let processes = session
            .tracking
            .and_then(|value| serde_json::from_value::<RecoveryState>(value).ok())
            .map(|state| state.processes)
            .unwrap_or_default();
        if processes.is_empty() {
            return Err(LaunchError::Invalid(crate::tr!(
                "the game's processes aren't known yet"
            )));
        }
        tokio::task::spawn_blocking(move || terminate(&processes, grace))
            .await
            .map_err(|error| LaunchError::Invalid(error.to_string()))
    }
}

/// The identities still running, signalling each with `signal` (none: just look).
fn signal(identities: &[ProcessIdentity], signal: Option<Signal>) -> Vec<ProcessIdentity> {
    let pids: Vec<Pid> = identities.iter().map(|id| Pid::from_u32(id.pid)).collect();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing(),
    );
    identities
        .iter()
        .filter(|id| {
            let Some(process) = system.process(Pid::from_u32(id.pid)) else {
                return false;
            };
            if process.start_time() != id.started_at
                || matches!(
                    process.status(),
                    ProcessStatus::Zombie | ProcessStatus::Dead
                )
            {
                return false;
            }
            // Hosts without the signal (Windows has no SIGTERM) end it.
            if let Some(signal) = signal
                && process.kill_with(signal).is_none()
            {
                process.kill();
            }
            true
        })
        .copied()
        .collect()
}

fn terminate(identities: &[ProcessIdentity], grace: Duration) -> usize {
    let running = signal(identities, Some(Signal::Term));
    let deadline = Instant::now() + grace;
    let mut left = running.clone();
    while !left.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        left = signal(&left, None);
    }
    if !left.is_empty() {
        tracing::warn!(processes = left.len(), "game didn't quit; ending it");
        signal(&left, Some(Signal::Kill));
    }
    running.len()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{db::LibraryStore, model::PlaySession, tracking::TrackingTarget};

    fn identity(pid: u32) -> ProcessIdentity {
        let mut system = System::new();
        let pids = [Pid::from_u32(pid)];
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing(),
        );
        ProcessIdentity {
            pid,
            started_at: system.process(pids[0]).unwrap().start_time(),
        }
    }

    async fn session(store: &LibraryStore, processes: Vec<ProcessIdentity>) -> PlaySession {
        let game = store
            .upsert_game(&crate::model::Game::new("Stoppable"))
            .await
            .unwrap();
        let entry = crate::model::GameEntry::new(
            &game.id,
            caskade_discovery::SourceId::new("fixture"),
            format!("stop-{}", processes.len()),
        );
        store.upsert_entry(&entry).await.unwrap();
        let mut state = RecoveryState::pending(TrackingTarget::default());
        state.processes = processes;
        let mut session = PlaySession::new(&game.id, &entry.id, 1);
        session.tracking = Some(serde_json::to_value(state).unwrap());
        store.start_session(&session).await.unwrap()
    }

    /// Children killed with the test, even when it panics: they inherit the
    /// build lock's descriptor and would hold it.
    struct Children(Vec<std::process::Child>);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// `exec` leaves one process per child, with no shell behind it (an
    /// ignored SIGTERM survives the exec).
    fn spawn(script: &str) -> std::process::Child {
        std::process::Command::new("sh")
            .args(["-c", script])
            .spawn()
            .unwrap()
    }

    #[tokio::test]
    async fn stop_ends_only_the_tracked_processes() {
        let mut children = Children(Vec::new());
        children.0.push(spawn("exec sleep 30"));
        children.0.push(spawn("trap '' TERM; exec sleep 30"));
        children.0.push(spawn("exec sleep 30"));
        std::thread::sleep(Duration::from_millis(200));
        let [polite, stubborn, bystander] = [0, 1, 2].map(|i| children.0[i].id());

        let store = LibraryStore::in_memory().await.unwrap();
        let service = LaunchService::new(store.clone());
        let playing = session(&store, vec![identity(polite), identity(stubborn)]).await;
        let stopped = service
            .stop(&playing.id, Duration::from_millis(500))
            .await
            .unwrap();
        assert_eq!(stopped, 2);
        assert!(children.0[0].wait().is_ok());
        assert!(children.0[1].wait().is_ok());
        assert!(
            children.0[2].try_wait().unwrap().is_none(),
            "untracked processes stay"
        );

        // A reused PID (same pid, another start time) is never touched.
        let reused = ProcessIdentity {
            pid: bystander,
            started_at: identity(bystander).started_at + 1,
        };
        let other = session(&store, vec![reused]).await;
        assert_eq!(service.stop(&other.id, Duration::ZERO).await.unwrap(), 0);
        assert!(children.0[2].try_wait().unwrap().is_none());

        assert!(service.stop("missing", Duration::ZERO).await.is_err());
    }
}
