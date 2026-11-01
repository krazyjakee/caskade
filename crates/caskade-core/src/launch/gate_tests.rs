//! The play gate (#209): asked before anything a launch does, and told
//! only when the game started.
use super::{test_support::*, *};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Gate {
    refuse: bool,
    log: Mutex<Vec<String>>,
}
#[async_trait]
impl PlayGate for Gate {
    async fn admit(&self, game_id: &str) -> Result<()> {
        self.log.lock().unwrap().push(format!("admit {game_id}"));
        if self.refuse {
            return Err(LaunchError::AgeRestricted(Box::new(
                crate::accounts::AgeRestriction {
                    account_id: "kid".into(),
                    account_name: "Kid".into(),
                    game_id: game_id.into(),
                    title: "Grown-up Game".into(),
                    ratings: vec![],
                    min_age: Some(18),
                    max_age: 12,
                },
            )));
        }
        Ok(())
    }
    fn started(&self, game_id: &str) {
        self.log.lock().unwrap().push(format!("started {game_id}"));
    }
}

/// Records when it runs: it would download a RomM ROM.
struct Preparer(Arc<Gate>);
#[async_trait]
impl LaunchPreparer for Preparer {
    async fn prepare(&self, entry_id: &str) -> Result<()> {
        self.0
            .log
            .lock()
            .unwrap()
            .push(format!("prepare {entry_id}"));
        Ok(())
    }
}

fn gated(store: Arc<MemoryStore>, gate: Arc<Gate>) -> LaunchService {
    service(store, Arc::new(Events::default()))
        .with_preparer(Arc::new(Preparer(Arc::clone(&gate))))
        .with_play_gate(gate)
}

fn stored(entry: &crate::model::GameEntry) -> Arc<MemoryStore> {
    let store = Arc::new(MemoryStore::default());
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    store
}

#[tokio::test]
async fn a_refused_launch_stops_before_anything_runs() {
    let t = tempfile::tempdir().unwrap();
    let token = t.path().join("token");
    let entry = entry(
        "/bin/sh".into(),
        vec!["-c".into(), format!("touch '{}'", token.display())],
        Some(t.path().into()),
    );
    let gate = Arc::new(Gate {
        refuse: true,
        ..Gate::default()
    });
    let store = stored(&entry);
    let error = gated(store.clone(), Arc::clone(&gate))
        .launch(&entry.id)
        .await
        .err()
        .unwrap();
    let LaunchError::AgeRestricted(restriction) = &error else {
        panic!("{error}");
    };
    assert_eq!(restriction.game_id, "game");
    assert_eq!(
        error.to_string(),
        "an admin PIN is needed to play Grown-up Game"
    );
    // Not prepared, no session, nothing ran.
    assert_eq!(*gate.log.lock().unwrap(), ["admit game"]);
    assert!(store.sessions.lock().unwrap().is_empty());
    assert!(!token.exists());
}

#[tokio::test]
async fn the_gate_hears_only_of_a_game_that_started() {
    let t = tempfile::tempdir().unwrap();
    let mut broken = entry("/bin/sh".into(), vec![], Some(t.path().into()));
    broken.launch = None;
    let gate = Arc::new(Gate::default());
    let service = gated(stored(&broken), Arc::clone(&gate));
    assert!(service.launch(&broken.id).await.is_err());
    assert_eq!(
        *gate.log.lock().unwrap(),
        ["admit game", format!("prepare {}", broken.id).as_str()]
    );

    let entry = entry(
        "/bin/sh".into(),
        vec!["-c".into(), "exit 0".into()],
        Some(t.path().into()),
    );
    let gate = Arc::new(Gate::default());
    let service = gated(stored(&entry), Arc::clone(&gate));
    let handle = service.launch(&entry.id).await.unwrap();
    assert_eq!(
        *gate.log.lock().unwrap(),
        [
            "admit game".to_owned(),
            format!("prepare {}", entry.id),
            "started game".to_owned()
        ]
    );
    let _ = handle.wait().await;
}
