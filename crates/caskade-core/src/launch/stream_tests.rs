//! Streamed launches (#264) with a fake client: a script that writes what
//! Moonlight writes on stderr, and a fake [`StreamLauncher`].
use super::{test_support::*, *};
use crate::model::{GameEntry, LaunchSpec};
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Like the client: says the host answered (or fails), then streams until
/// it is asked to quit or the host's game ends.
const CLIENT: &str = r#"#!/bin/sh
# Not the test runner's files (the build lane's lock among them).
for fd in 3 4 5 6 7 8 9; do eval "exec $fd>&-"; done
log="$1"; mode="$2"
echo "pid $$" >> "$log"
trap 'echo term >> "$log"; exit 0' TERM
case "$mode" in
  stream) echo "00:00:06 - Qt Info: Launch response: ok" >&2 ;;
  fail) echo "00:00:11 - Qt Critical: Failed to find application Celeste" >&2 ;;
  late) echo "Launch response: ok" >&2; sleep 0.05; echo "Qt Critical: Connection failed" >&2 ;;
  ends) echo "Launch response: ok" >&2; sleep 0.6; exit 0 ;;
  gone) exit 0 ;;
esac
while true; do sleep 0.02; done
"#;

struct Fake {
    dir: tempfile::TempDir,
    mode: &'static str,
    busy: bool,
    ready: Duration,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn new(mode: &'static str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("client"), CLIENT).unwrap();
        Self {
            dir,
            mode,
            busy: false,
            ready: Duration::from_secs(10),
            calls: Mutex::default(),
        }
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("client.log")
    }

    fn client_log(&self) -> String {
        std::fs::read_to_string(self.log()).unwrap_or_default()
    }

    fn client_pid(&self) -> u32 {
        let log = self.client_log();
        let line = log.lines().find(|line| line.starts_with("pid ")).unwrap();
        line[4..].parse().unwrap()
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl StreamLauncher for Fake {
    async fn prepare(&self, host: &str, app: &str, stop_host: bool) -> Result<StreamStart> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("prepare {host} {app} {stop_host}"));
        if self.busy && !stop_host {
            return Err(LaunchError::HostBusy(Box::new(HostBusy {
                entry_id: String::new(),
                device: "Den".into(),
                playing: Some("Hades".into()),
            })));
        }
        let mut command = std::process::Command::new("sh");
        command
            .arg(self.dir.path().join("client"))
            .arg(self.log())
            .arg(self.mode);
        Ok(StreamStart {
            command,
            hold: None,
        })
    }

    fn read(&self, _host: &str, _app: &str, line: &str) -> ClientLine {
        if line.contains("Launch response:") {
            ClientLine::Streaming
        } else if let Some(at) = line.find("Qt Critical: ") {
            ClientLine::Failed(format!("Den: {}", &line[at + 13..]))
        } else {
            ClientLine::Other
        }
    }

    async fn finish(&self, host: &str, app: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("finish {host} {app}"));
    }

    fn ready_within(&self) -> Duration {
        self.ready
    }

    fn settle(&self) -> Duration {
        Duration::from_millis(200)
    }

    fn stop_grace(&self) -> Duration {
        Duration::from_millis(500)
    }
}

fn streamed() -> GameEntry {
    let mut entry = GameEntry::new(
        "game",
        caskade_discovery::SourceId::new("stream:den"),
        "881",
    );
    entry.state = caskade_discovery::InstallState::Partial;
    entry.launch = Some(LaunchSpec::Stream {
        host: "den".into(),
        app: "Celeste".into(),
    });
    entry
}

fn setup(
    fake: Fake,
) -> (
    Arc<Fake>,
    Arc<MemoryStore>,
    Arc<Events>,
    LaunchService,
    String,
) {
    let entry = streamed();
    let store = Arc::new(MemoryStore::default());
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let events = Arc::new(Events::default());
    let service = service(Arc::clone(&store), Arc::clone(&events));
    let fake = Arc::new(fake);
    service.set_streamer(fake.clone());
    (fake, store, events, service, entry.id)
}

fn alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

fn names(events: &Events) -> Vec<String> {
    events
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

#[tokio::test]
async fn a_stream_is_followed_like_a_session_but_never_stored() {
    let (fake, store, events, service, entry_id) = setup(Fake::new("stream"));
    let handle = service.launch(&entry_id).await.unwrap();
    assert_eq!(names(&events), ["stream.starting", "game.started"]);
    // One session id from the start.
    assert!(
        events
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|(_, event)| event.session_id == handle.ticket.session_id)
    );
    let running = service.running_sessions().await.unwrap();
    assert_eq!(running, std::slice::from_ref(&handle.ticket));
    let streams = service.running_streams(Some("game"));
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].ended_at_ms, None);
    assert!(service.running_streams(Some("other")).is_empty());
    // A second Play while it streams is refused, as for any game.
    assert!(matches!(
        service.launch(&entry_id).await,
        Err(LaunchError::AlreadyRunning)
    ));
    // Another game can't stream meanwhile: one stream at a time.
    let mut other = streamed();
    other.id = "other".into();
    other.game_id = "game2".into();
    store
        .entries
        .lock()
        .unwrap()
        .insert(other.id.clone(), other.clone());
    let Err(LaunchError::Refused(text)) = service.launch(&other.id).await else {
        panic!("a second stream should be refused");
    };
    assert_eq!(text, "Another game is streaming. End that stream first.");

    // End stream: the host's game stops first, then the client quits.
    let session = handle.ticket.session_id.clone();
    assert_eq!(
        service
            .stop(&session, Duration::from_secs(1))
            .await
            .unwrap(),
        1
    );
    let (ended, status) = handle.wait_status().await.unwrap();
    assert_eq!(status, SessionStatus::Exited);
    assert!(ended.ended_at_ms.is_some());
    assert!(service.take_stopped(&session));
    assert_eq!(
        fake.calls(),
        ["prepare den Celeste false", "finish den Celeste"]
    );
    assert!(fake.client_log().contains("term"), "{}", fake.client_log());
    assert!(!alive(fake.client_pid()));
    // Playtime is the host's: nothing here, and no after-hooks (saves).
    assert!(store.sessions.lock().unwrap().is_empty());
    assert_eq!(
        names(&events),
        ["stream.starting", "game.started", "game.exited"]
    );
    let exited = events.0.lock().unwrap()[2].1.clone();
    assert_eq!(exited.session_id, session);
    assert_eq!(exited.command, None);
    assert!(service.running_sessions().await.unwrap().is_empty());
    // The lease is free again.
    let again = service.launch(&entry_id).await.unwrap();
    service
        .stop(&again.ticket.session_id, Duration::from_secs(1))
        .await
        .unwrap();
}

#[tokio::test]
async fn the_stream_ends_with_the_hosts_game() {
    let (fake, store, events, service, entry_id) = setup(Fake::new("ends"));
    let handle = service.launch(&entry_id).await.unwrap();
    let (_, status) = handle.wait_status().await.unwrap();
    assert_eq!(status, SessionStatus::Exited);
    assert_eq!(
        fake.calls(),
        ["prepare den Celeste false", "finish den Celeste"]
    );
    assert!(!fake.client_log().contains("term"));
    assert!(store.sessions.lock().unwrap().is_empty());
    assert_eq!(
        names(&events),
        ["stream.starting", "game.started", "game.exited"]
    );
    assert!(!service.take_stopped(&handle_session(&events)));
}

fn handle_session(events: &Events) -> String {
    events.0.lock().unwrap()[0].1.session_id.clone()
}

#[tokio::test]
async fn a_client_error_fails_the_launch_in_caskades_words_and_leaves_no_client() {
    for mode in ["fail", "late"] {
        let (fake, store, events, service, entry_id) = setup(Fake::new(mode));
        let Err(LaunchError::Refused(text)) = service.launch(&entry_id).await else {
            panic!("{mode}: the launch should fail");
        };
        assert!(text.starts_with("Den: "), "{mode}: {text}");
        assert!(fake.client_log().contains("term"), "{mode}");
        assert!(!alive(fake.client_pid()), "{mode}");
        assert_eq!(
            fake.calls(),
            ["prepare den Celeste false", "finish den Celeste"],
            "{mode}"
        );
        // What covered the client stops covering it.
        assert_eq!(names(&events), ["stream.starting", "game.exited"], "{mode}");
        assert_eq!(
            events.0.lock().unwrap()[1].1.status,
            SessionStatus::NotStarted
        );
        assert!(store.sessions.lock().unwrap().is_empty());
        assert!(service.running_sessions().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn a_client_that_quits_or_never_streams_fails_the_launch() {
    let (fake, _, events, service, entry_id) = setup(Fake::new("gone"));
    assert!(matches!(
        service.launch(&entry_id).await,
        Err(LaunchError::Refused(_))
    ));
    assert_eq!(names(&events), ["stream.starting", "game.exited"]);
    assert!(!alive(fake.client_pid()));

    let mut silent = Fake::new("silent");
    silent.ready = Duration::from_millis(300);
    let (fake, _, events, service, entry_id) = setup(silent);
    let Err(LaunchError::Refused(text)) = service.launch(&entry_id).await else {
        panic!("a silent client should time out");
    };
    assert!(text.contains("didn't start"), "{text}");
    assert!(fake.client_log().contains("term"));
    assert!(!alive(fake.client_pid()));
    assert_eq!(names(&events), ["stream.starting", "game.exited"]);
}

#[tokio::test]
async fn a_busy_host_asks_before_its_game_is_stopped() {
    let mut busy = Fake::new("stream");
    busy.busy = true;
    let (fake, _, _, service, entry_id) = setup(busy);
    let Err(LaunchError::HostBusy(asked)) = service.launch(&entry_id).await else {
        panic!("a busy host should ask first");
    };
    assert_eq!(
        *asked,
        HostBusy {
            entry_id: entry_id.clone(),
            device: "Den".into(),
            playing: Some("Hades".into()),
        }
    );
    // Nothing started, and the lease is free for the answer.
    assert!(service.running_sessions().await.unwrap().is_empty());
    let handle = service
        .launch_with(
            &entry_id,
            &LaunchChoice {
                stop_host: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        fake.calls(),
        ["prepare den Celeste false", "prepare den Celeste true"]
    );
    service
        .stop(&handle.ticket.session_id, Duration::from_secs(1))
        .await
        .unwrap();
}

#[tokio::test]
async fn without_the_client_a_stream_cant_start() {
    let entry = streamed();
    let store = Arc::new(MemoryStore::default());
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let service = service(store, Arc::new(Events::default()));
    assert!(matches!(
        service.launch(&entry.id).await,
        Err(LaunchError::Unsupported(_))
    ));
}
