//! #55: the installer's events and interceptor points on the bus.

use super::*;
use crate::{
    AppPaths,
    bus::{Event, Intercept, Interceptor},
    db::LibraryStore,
    install::{InstallOptions, InstallService, JobSnapshot, JobStatus},
    launch::LaunchService,
    settings::SettingsService,
    storage::StorageService,
};
use async_trait::async_trait;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

async fn finished(service: &InstallService, id: &str) -> JobSnapshot {
    for _ in 0..600 {
        let job = service.job(id).unwrap();
        if job.status.is_finished() || job.status == JobStatus::NeedsUser {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {id} did not finish: {:?}", service.job(id));
}

// The move test is Linux-only (its fixture is an ELF game).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct Setup {
    _temp: tempfile::TempDir,
    bus: Bus,
    service: InstallService,
    storage: StorageService,
    source: std::path::PathBuf,
    second_root: String,
}

/// Two storage roots and a portable game folder to install.
async fn setup() -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(&temp.path().join("app"));
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let storage = StorageService::new(store.clone(), settings, LaunchService::new(store.clone()))
        .unwrap()
        .with_paths(paths);
    storage
        .register_root(temp.path().join("one"), None)
        .await
        .unwrap();
    let second = storage
        .register_root(temp.path().join("two"), None)
        .await
        .unwrap();
    let source = temp.path().join("Fixture Runner");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("data.pak"), vec![7u8; 1024]).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"),
        source.join("fixture-runner.x86_64"),
    )
    .unwrap();
    let bus = Bus::default();
    let service = InstallService::new(
        store,
        bus.clone(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    )
    .with_storage(storage.clone());
    Setup {
        _temp: temp,
        bus,
        service,
        storage,
        source,
        second_root: second.root.id,
    }
}

/// Records what a point saw; cancels with `cancel` when set.
struct Step<P: InterceptPoint> {
    seen: Arc<Mutex<Vec<P::Payload>>>,
    cancel: Option<&'static str>,
}

#[async_trait]
impl<P: InterceptPoint> Interceptor<P> for Step<P> {
    async fn intercept(&self, payload: P::Payload) -> Result<Intercept<P::Payload>, String> {
        self.seen.lock().unwrap().push(payload.clone());
        Ok(match self.cancel {
            Some(reason) => Intercept::Cancel(reason.into()),
            None => Intercept::Continue(payload),
        })
    }
}

fn step<P: InterceptPoint>(bus: &Bus, cancel: Option<&'static str>) -> Arc<Mutex<Vec<P::Payload>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    bus.intercept::<P>(
        "test",
        0,
        Arc::new(Step::<P> {
            seen: seen.clone(),
            cancel,
        }),
    );
    seen
}

fn names(events: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<&'static str> {
    let mut names = Vec::new();
    while let Ok(event) = events.try_recv() {
        names.push(event.name());
    }
    names
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn installs_and_moves_emit_events_and_run_their_interceptors() {
    let s = setup().await;
    let planned = step::<InstallPlanned>(&s.bus, None);
    let after = step::<InstallAfter>(&s.bus, Some("no config to patch"));
    let moved = step::<InstallMoved>(&s.bus, None);
    let mut events = s.bus.stream();

    let queued = s
        .service
        .install(s.source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let job = finished(&s.service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");

    // install.plan saw the ranked plans before anything ran.
    let planned = planned.lock().unwrap().clone();
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].job_id, job.id);
    assert_eq!(planned[0].plans.candidates, job.candidates);
    // install.after saw the game; its cancel is a warning, not a failure.
    let after = after.lock().unwrap().clone();
    assert_eq!(after[0].game_id, job.game_id.clone().unwrap());
    assert_eq!(Some(&after[0].dir), job.target_dir.as_ref());
    assert!(
        job.warnings
            .iter()
            .any(|warning| warning.code == "install.after"
                && warning.message == "test: no config to patch"),
        "{:?}",
        job.warnings
    );
    let installed = names(&mut events);
    for name in ["install.queued", "install.progress", "install.completed"] {
        assert!(installed.contains(&name), "{name} in {installed:?}");
    }
    assert!(!installed.contains(&"install.downloaded"), "a local path");

    // Moving tells install.moved where the game went.
    let game_id = job.game_id.clone().unwrap();
    let entry = s
        .service
        .inner
        .store
        .entries_for_game(&game_id)
        .await
        .unwrap()
        .remove(0);
    let queued = s
        .service
        .move_game(entry.id, s.second_root.clone(), s.storage.clone())
        .await
        .unwrap();
    let job = finished(&s.service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    let moved = moved.lock().unwrap().clone();
    assert_eq!(moved[0].game_id, game_id);
    assert_eq!(Some(&moved[0].to), job.target_dir.as_ref());
    assert_ne!(moved[0].from, moved[0].to);
    assert!(!moved[0].from.exists() && moved[0].to.join("data.pak").is_file());
    assert!(names(&mut events).contains(&"game.moved"));
}

#[tokio::test(flavor = "multi_thread")]
async fn install_plan_can_stop_an_install() {
    let s = setup().await;
    step::<InstallPlanned>(&s.bus, Some("not on this machine"));
    let mut events = s.bus.stream();
    let queued = s
        .service
        .install(s.source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let job = finished(&s.service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Failed, "{job:?}");
    let error = job.error.unwrap_or_default();
    assert!(error.contains("not on this machine"), "{error}");
    assert!(names(&mut events).contains(&"install.failed"));
}
