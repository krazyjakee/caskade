#![cfg(target_os = "linux")]
use super::{test_support::*, *};
use crate::bus::{Bus, Event, Intercept, Interceptor};
use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Rewrite {
    token: String,
}
#[async_trait]
impl Interceptor<LaunchBefore> for Rewrite {
    async fn intercept(
        &self,
        mut command: LaunchCommand,
    ) -> std::result::Result<Intercept<LaunchCommand>, String> {
        command.args = vec!["--token".into(), self.token.clone()];
        command
            .env
            .insert("CASKADE_LAUNCH_TEST".into(), "literal value".into());
        Ok(Intercept::Continue(command))
    }
}
struct After(Arc<AtomicUsize>);
#[async_trait]
impl Interceptor<LaunchAfter> for After {
    async fn intercept(
        &self,
        event: LaunchEvent,
    ) -> std::result::Result<Intercept<LaunchEvent>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Intercept::Continue(event))
    }
}
#[tokio::test]
async fn bus_rewrite_events_and_after_hook_share_the_persisted_session() {
    let t = tempfile::tempdir().unwrap();
    let exe = t.path().join("fixture");
    crate::test_programs::copy_program(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &exe,
    );
    let entry = entry(exe, vec![], Some(t.path().into()));
    let store = Arc::new(MemoryStore::default());
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let bus = Bus::default();
    let mut events = bus.stream();
    let token = t.path().join("token");
    let after = Arc::new(AtomicUsize::new(0));
    bus.intercept::<LaunchBefore>(
        "test.rewrite",
        0,
        Arc::new(Rewrite {
            token: token.to_str().unwrap().into(),
        }),
    );
    bus.intercept::<LaunchAfter>("test.after", 0, Arc::new(After(after.clone())));
    let session = LaunchService::from_repository(store)
        .with_bus(bus)
        .with_config(fast_config())
        .with_aliases(None)
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(token.is_file());
    assert_eq!(after.load(Ordering::SeqCst), 1);
    assert!(matches!(
        events.recv().await.unwrap(),
        Event::GameLaunching { .. }
    ));
    let Event::GameStarted(started) = events.recv().await.unwrap() else {
        panic!("started event missing")
    };
    let Event::GameExited(exited) = events.recv().await.unwrap() else {
        panic!("exited event missing")
    };
    assert_eq!(started.session_id, session.id);
    assert_eq!(exited.session_id, session.id);
    assert_eq!(exited.duration_ms, session.duration_ms);
    assert!(exited.ended_at_ms.is_some());
}
struct Cancel;
#[async_trait]
impl Interceptor<LaunchBefore> for Cancel {
    async fn intercept(
        &self,
        _: LaunchCommand,
    ) -> std::result::Result<Intercept<LaunchCommand>, String> {
        Ok(Intercept::Cancel("test cancellation".into()))
    }
}
#[tokio::test]
async fn bus_cancel_creates_neither_session_nor_lifecycle_event() {
    let store = Arc::new(MemoryStore::default());
    let entry = entry("/bin/true".into(), vec![], None);
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let bus = Bus::default();
    let mut events = bus.stream();
    bus.intercept::<LaunchBefore>("test.cancel", 0, Arc::new(Cancel));
    let service = LaunchService::from_repository(store.clone()).with_bus(bus);
    assert!(matches!(
        service.launch(&entry.id).await,
        Err(LaunchError::Refused(reason))
            if reason.contains("test.cancel") && reason.contains("test cancellation")
    ));
    assert!(store.sessions.lock().unwrap().is_empty());
    assert!(events.try_recv().is_err());
}
