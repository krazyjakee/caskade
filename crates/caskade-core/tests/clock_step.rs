//! Stop and session recovery across a real step of the system clock, which
//! the unit tests can only stand in for: on Linux a step moves the start time
//! sysinfo reports for a running process; on Windows it must not. CI's `rust`
//! and `windows-elevation` jobs run this on throwaway runners, on its own, as
//! it steps the machine's clock for everything else running there.

use caskade_core::{
    db::LibraryStore,
    launch::LaunchService,
    model::{Game, GameEntry, PlaySession},
    sessions::{RecoveryState, SessionPhase},
    tracking::{ProcessIdentity, TrackingTarget},
};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// More than any start time's rounding, less than breaks a runner's sign-ins.
const STEP: i64 = 600;

fn step(seconds: i64) {
    #[cfg(windows)]
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command"])
        .arg(format!(
            "Set-Date -Adjust ([TimeSpan]::FromSeconds({seconds})) | Out-Null"
        ))
        .status();
    #[cfg(unix)]
    let status = {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let then = now.as_secs().checked_add_signed(seconds).unwrap();
        Command::new("sudo")
            .args(["-n", "date", "-u", "-s"])
            .arg(format!("@{then}.{:09}", now.subsec_nanos()))
            .stdout(Stdio::null())
            .status()
    };
    assert!(status.unwrap().success(), "couldn't step the clock");
}

/// The clock stepped until this is dropped, even by a failed assertion.
struct Stepped(i64);

impl Stepped {
    fn by(seconds: i64) -> Self {
        let before = SystemTime::now();
        step(seconds);
        let stepped = Self(seconds);
        let moved = match SystemTime::now().duration_since(before) {
            Ok(ahead) => ahead.as_secs_f64(),
            Err(behind) => -behind.duration().as_secs_f64(),
        };
        assert!(
            (moved - seconds as f64).abs() < 30.0,
            "asked for {seconds} s, moved {moved} s"
        );
        stepped
    }
}

impl Drop for Stepped {
    fn drop(&mut self) {
        step(-self.0);
    }
}

struct Running(Child);

impl Running {
    fn start() -> Self {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("ping");
            command.args(["-n", "120", "127.0.0.1"]);
            command
        };
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sleep");
            command.arg("120");
            command
        };
        Self(
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn sysinfo_start(pid: u32) -> u64 {
    let id = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[id]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system.process(id).unwrap().start_time()
}

/// A session recorded by a Caskade that has since gone away.
async fn session(store: &LibraryStore, name: &str, process: ProcessIdentity) -> PlaySession {
    let game = store.upsert_game(&Game::new(name)).await.unwrap();
    let entry = GameEntry::new(
        &game.id,
        caskade_discovery::SourceId::new("fixture"),
        name.to_owned(),
    );
    store.upsert_entry(&entry).await.unwrap();
    let mut state = RecoveryState::pending(TrackingTarget::default());
    state.processes = vec![process];
    state.phase = SessionPhase::Playing;
    let mut session = PlaySession::new(&game.id, &entry.id, 1);
    session.tracking = Some(serde_json::to_value(state).unwrap());
    store.start_session(&session).await.unwrap()
}

async fn stepped_by(seconds: i64) {
    let store = LibraryStore::in_memory().await.unwrap();
    let mut game = Running::start();
    let mut bystander = Running::start();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let recorded = ProcessIdentity::lookup(game.pid()).unwrap();
    let playing = session(&store, "Stepped", recorded).await;
    let beside = ProcessIdentity::lookup(bystander.pid()).unwrap();
    let reused = session(
        &store,
        "Reused",
        ProcessIdentity {
            started_at: beside.started_at + 1,
            ..beside
        },
    )
    .await;
    let reported = sysinfo_start(game.pid());

    let clock = Stepped::by(seconds);
    let after = sysinfo_start(game.pid());
    println!("stepped {seconds} s: sysinfo said {reported}, now says {after}");
    if cfg!(target_os = "linux") {
        // The bug's premise, seen for real: it is why sysinfo's start can't be
        // the identity, and a clock that didn't move would prove nothing.
        assert_ne!(after, reported, "the step didn't reach sysinfo");
    } else {
        // What lets Windows keep sysinfo's start as the identity.
        assert_eq!(after, reported, "a step moved a running process's start");
    }
    assert_eq!(ProcessIdentity::lookup(game.pid()), Some(recorded));

    // A new Caskade, as after a restart, finds the game and only the game.
    let service = LaunchService::new(store.clone());
    let handles = service.recover().await.unwrap();
    assert_eq!(handles.len(), 1, "recovery didn't find exactly the game");
    assert_eq!(handles[0].ticket.session_id, playing.id);
    let open = store.in_flight_sessions().await.unwrap();
    assert!(
        open.iter().all(|session| session.id != reused.id),
        "the session with the reused PID stayed open"
    );

    let stopped = service
        .stop(&playing.id, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(stopped, 1);
    assert!(game.0.wait().is_ok());
    drop(clock);
    assert!(bystander.0.try_wait().unwrap().is_none(), "bystander hit");
}

#[tokio::test]
#[ignore = "steps this machine's clock; CASKADE_TEST_NATIVE=1 on a throwaway runner"]
async fn stop_and_recovery_survive_a_real_clock_step() {
    assert_eq!(
        std::env::var("CASKADE_TEST_NATIVE").as_deref(),
        Ok("1"),
        "this steps the machine's clock"
    );
    stepped_by(STEP).await;
    stepped_by(-STEP).await;
}
