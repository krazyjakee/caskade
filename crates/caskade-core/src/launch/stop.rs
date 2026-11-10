//! Stopping a running game (#149, the guide quick menu's "Stop game").
//!
//! Only the session's game processes are touched: those stored with it and
//! those its tracker finds at that moment, as the stored list is as old as
//! the last heartbeat and the tracker's own view as old as its last poll. It
//! looks again while the game quits, because a game can start a process until
//! its last one ends. All are matched exactly by [`ProcessIdentity`], which a
//! stepped clock doesn't change, so a reused PID is never hit. The launcher is left alone: it can be a store client (Steam)
//! that must keep running. The tracker sees the processes exit and ends the
//! session as usual, so playtime is recorded.

use super::{LaunchError, LaunchService, Result};
use crate::{
    sessions::RecoveryState,
    tracking::{LiveTracker, ProcessIdentity, ProcessMonitor},
};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, Signal, System};

impl LaunchService {
    /// Asks the session's game processes to quit, and ends any still
    /// running after `grace`. Returns how many were running.
    pub async fn stop(&self, session_id: &str, grace: Duration) -> Result<usize> {
        if let Some(count) = self.stop_stream(session_id).await {
            return Ok(count);
        }
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
        let mut processes = session
            .tracking
            .and_then(|value| serde_json::from_value::<RecoveryState>(value).ok())
            .map(|state| state.processes)
            .unwrap_or_default();
        let tracker = crate::lock(&self.live).get(session_id).cloned();
        let monitor = self.monitor.clone();
        processes.extend(look(tracker.as_ref(), &monitor).await);
        if processes.is_empty() {
            return Err(LaunchError::Invalid(crate::tr!(
                "the game's processes aren't known yet"
            )));
        }
        crate::lock(&self.stopped).insert(session.id);
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            terminate(processes, grace, || {
                runtime.block_on(look(tracker.as_ref(), &monitor))
            })
        })
        .await
        .map_err(|error| LaunchError::Invalid(error.to_string()))
    }

    /// Whether [`Self::stop`] ended `session_id`, forgetting it: how the
    /// program that asked for the launch tells a Stop from the game's own
    /// exit (`caskade --launch`, #256).
    pub fn take_stopped(&self, session_id: &str) -> bool {
        crate::lock(&self.stopped).remove(session_id)
    }

    /// Whether [`Self::stop`] ended `session_id` and nobody took the record
    /// yet, without taking it (tests waiting on the taker).
    pub fn is_stopped(&self, session_id: &str) -> bool {
        crate::lock(&self.stopped).contains(session_id)
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
            if ProcessIdentity::of(id.pid, process) != **id
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

/// How long Stop waits for one look at the system. Tracking can touch a
/// disconnected mount, and that must not keep Stop from its signals.
const LOOK_LIMIT: Duration = Duration::from_secs(2);

/// The session's processes by its tracker, none once nothing follows it. A
/// failed look leaves Stop with what it has: stopping those still matters.
async fn look(tracker: Option<&LiveTracker>, monitor: &ProcessMonitor) -> Vec<ProcessIdentity> {
    let Some(tracker) = tracker else {
        return Vec::new();
    };
    match tokio::time::timeout(LOOK_LIMIT, tracker.poll(monitor)).await {
        Ok(Ok(processes)) => processes,
        Ok(Err(error)) => {
            tracing::warn!(%error, "couldn't look for the game's processes");
            Vec::new()
        }
        Err(_) => {
            tracing::warn!("looking for the game's processes took too long");
            Vec::new()
        }
    }
}

/// Asks `identities` to quit, then whatever `look` finds that they started,
/// until none is left, and ends any still running after `grace`.
fn terminate(
    identities: Vec<ProcessIdentity>,
    grace: Duration,
    mut look: impl FnMut() -> Vec<ProcessIdentity>,
) -> usize {
    let deadline = Instant::now() + grace;
    let mut asked = BTreeSet::new();
    let mut left = Vec::new();
    let mut found = identities;
    let mut running = 0;
    loop {
        found.retain(|id| asked.insert(*id));
        let quitting = signal(&found, Some(Signal::Term));
        running += quitting.len();
        left.extend(quitting);
        if left.is_empty() || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        left = signal(&left, None);
        found = look();
    }
    if !left.is_empty() {
        tracing::warn!(processes = left.len(), "game didn't quit; ending it");
        signal(&left, Some(Signal::Kill));
    }
    running
}

#[cfg(all(test, unix))]
mod tests {
    use super::{
        super::{LaunchConfig, LaunchHandle, test_support::*},
        *,
    };
    use crate::{db::LibraryStore, model::PlaySession, tracking::TrackingTarget};
    use std::sync::Arc;

    fn identity(pid: u32) -> ProcessIdentity {
        ProcessIdentity::lookup(pid).unwrap()
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
            started_at: identity(bystander).started_at + 1,
            ..identity(bystander)
        };
        let other = session(&store, vec![reused]).await;
        assert_eq!(service.stop(&other.id, Duration::ZERO).await.unwrap(), 0);
        assert!(children.0[2].try_wait().unwrap().is_none());

        assert!(service.stop("missing", Duration::ZERO).await.is_err());
    }

    /// A stepped clock must not cost Stop the game it is stopping.
    #[cfg(target_os = "linux")]
    #[test]
    fn stop_still_knows_its_game_after_the_clock_steps() {
        for seconds in [7200, -7200] {
            let mut game = Children(vec![spawn("exec sleep 30")]);
            std::thread::sleep(Duration::from_millis(200));
            let recorded = identity(game.0[0].id());

            let _stepped = crate::tracking::ClockStep::by(seconds);
            assert_eq!(
                terminate(vec![recorded], Duration::from_secs(5), Vec::new),
                1,
                "Stop lost the game after a step of {seconds} s"
            );
            assert!(game.0[0].wait().is_ok());
        }
    }

    /// What replaces the wall clock must be as exact as it was: however the
    /// clock moves, a record of another process isn't this one.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_stepped_clock_never_brings_a_reused_pid_into_stop() {
        let mut children = Children(vec![spawn("exec sleep 30")]);
        std::thread::sleep(Duration::from_millis(200));
        let now = identity(children.0[0].id());
        let earlier_process = ProcessIdentity {
            started_at: now.started_at - 1,
            ..now
        };
        let earlier_boot = ProcessIdentity {
            boot: now.boot ^ 1,
            ..now
        };
        for seconds in [0, 7200, -7200] {
            let _stepped = crate::tracking::ClockStep::by(seconds);
            for record in [earlier_process, earlier_boot] {
                assert_eq!(terminate(vec![record], Duration::ZERO, Vec::new), 0);
            }
        }
        assert!(children.0[0].try_wait().unwrap().is_none());
    }

    fn alive(pid: u32) -> bool {
        let pids = [Pid::from_u32(pid)];
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing(),
        );
        system
            .process(pids[0])
            .is_some_and(|process| !matches!(process.status(), ProcessStatus::Zombie))
    }

    /// A process the game started, killed with the test when it panics.
    struct Joined(u32);
    impl Drop for Joined {
        fn drop(&mut self) {
            let pids = [Pid::from_u32(self.0)];
            let mut system = System::new();
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&pids),
                true,
                ProcessRefreshKind::nothing(),
            );
            if let Some(process) = system.process(pids[0]) {
                process.kill();
            }
        }
    }

    /// The pid the game writes to `file`.
    async fn written_pid(file: &std::path::Path) -> Joined {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let written = std::fs::read_to_string(file).unwrap_or_default();
            if let Ok(pid) = written.trim().parse() {
                return Joined(pid);
            }
            assert!(Instant::now() < deadline, "the game never started it");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    struct Playing {
        service: LaunchService,
        session: String,
        recorded: Vec<ProcessIdentity>,
        handle: LaunchHandle,
    }

    /// `script` running as a game from `folder`, found by its session's
    /// first poll. The next poll and heartbeat are far off, so whatever the
    /// game starts from here is news to everything but a look at the system.
    /// The game is a copy of `sh`: `sh` itself would match every shell
    /// started on the machine.
    async fn play(folder: &std::path::Path, script: String) -> Playing {
        let program = folder.join("game");
        crate::test_programs::copy_program("/bin/sh", &program);
        let game = entry(program, vec!["-c".into(), script], Some(folder.to_owned()));
        let store = Arc::new(MemoryStore::default());
        store
            .entries
            .lock()
            .unwrap()
            .insert(game.id.clone(), game.clone());
        let events = Arc::new(Events::default());
        let service = service(store.clone(), events.clone()).with_config(LaunchConfig {
            poll_interval: Duration::from_secs(60),
            exit_grace: Duration::ZERO,
            heartbeat_interval: Duration::from_secs(3600),
            ..fast_config()
        });
        let handle = service.launch(&game.id).await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while events.0.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "the game never started");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let session = handle.ticket.session_id.clone();
        let stored = store.sessions.lock().unwrap()[&session].clone();
        let recorded = serde_json::from_value::<RecoveryState>(stored.tracking.unwrap())
            .unwrap()
            .processes;
        Playing {
            service,
            session,
            recorded,
            handle,
        }
    }

    /// The session ends with the game, as a stopped one should.
    async fn ended(handle: LaunchHandle) {
        tokio::time::timeout(Duration::from_secs(5), handle.wait())
            .await
            .expect("the session outlived its game")
            .unwrap();
    }

    #[tokio::test]
    async fn stop_ends_a_process_the_game_started_since_the_last_look() {
        let folder = tempfile::tempdir().unwrap();
        let go = folder.path().join("go");
        let pid = folder.path().join("child");
        let mut bystander = Children(vec![spawn("exec sleep 30")]);
        let game = play(
            folder.path(),
            format!(
                "until [ -e '{go}' ]; do :; done; sleep 30 & echo $! > '{pid}'; wait",
                go = go.display(),
                pid = pid.display(),
            ),
        )
        .await;
        assert_eq!(game.recorded.len(), 1, "only the game was running");

        std::fs::write(&go, "").unwrap();
        let child = written_pid(&pid).await;
        let stopped = game
            .service
            .stop(&game.session, Duration::from_secs(5))
            .await
            .unwrap();

        assert!(
            !alive(child.0),
            "the child the game just started survived Stop"
        );
        assert_eq!(stopped, 2);
        assert!(
            bystander.0[0].try_wait().unwrap().is_none(),
            "only the session's processes are touched"
        );
        ended(game.handle).await;
    }

    #[tokio::test]
    async fn stop_ends_a_process_the_game_starts_while_it_quits() {
        let folder = tempfile::tempdir().unwrap();
        let first = folder.path().join("first");
        let late = folder.path().join("late");
        let game = play(
            folder.path(),
            format!(
                "trap 'sleep 30 & echo $! > \"{late}\"; wait; exit 0' TERM; \
             sleep 30 & echo $! > '{first}'; wait",
                late = late.display(),
                first = first.display(),
            ),
        )
        .await;
        let first = written_pid(&first).await;
        let stopped = game
            .service
            .stop(&game.session, Duration::from_secs(5))
            .await
            .unwrap();
        let late = written_pid(&late).await;

        assert!(!alive(first.0));
        assert!(
            !alive(late.0),
            "the process started while quitting survived Stop"
        );
        assert_eq!(stopped, 3);
        ended(game.handle).await;
    }
}
