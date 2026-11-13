//! Install commands on the real core: the picker answers, job errors,
//! a fixture install with its revisioned progress events, a move, an
//! uninstall and the game's removal; and the service starting without its
//! 7-Zip sidecar.
use super::*;
use caskade_core::{bus::Bus, db::LibraryStore, install::InstallOptions};
use std::sync::Arc;
#[cfg(target_os = "linux")]
use {
    crate::events::{Emit, Event, Events, JobChanged},
    caskade_core::{
        install::{JobPhase, JobStatus},
        model::StorageRoot,
    },
    std::{
        sync::Mutex,
        time::{Duration, Instant},
    },
};

fn picker() -> InstallPicker {
    InstallPicker {
        files: Arc::new(|| {
            Box::pin(async {
                Ok(vec![
                    "/fixture/Café game.zip".into(),
                    "/fixture/second.7z".into(),
                ])
            })
        }),
        folder: Arc::new(|| Box::pin(async { Ok(None) })),
    }
}

/// What reached the UI.
#[cfg(target_os = "linux")]
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<Event>>>);

#[cfg(target_os = "linux")]
impl Emit for Sink {
    fn emit(&self, event: Event) -> Result<(), String> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Sink {
    /// The job snapshots sent so far, in order.
    fn jobs(&self) -> Vec<JobSnapshot> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::JobChanged(JobChanged(job)) => Some(job.clone()),
                _ => None,
            })
            .collect()
    }
}

#[tokio::test]
async fn dialogs_and_job_errors_round_trip() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = InstallService::new(store, Bus::default(), InstallOptions::default());
    let picker = picker();
    assert_eq!(
        choose_install_files(&picker).await.unwrap(),
        vec![
            "/fixture/Café game.zip".to_owned(),
            "/fixture/second.7z".to_owned()
        ]
    );
    assert_eq!(choose_install_folder(&picker).await.unwrap(), None);
    assert!(jobs_list(&service).is_empty());
    assert!(job_get(&service, "unknown".into()).is_err());
    assert!(job_cancel(&service, "unknown".into()).is_err());
    assert!(install_start(&service, String::new(), None).await.is_err());
    assert!(
        install_start(&service, "/fixture/game.zip".into(), Some("missing".into()))
            .await
            .is_err()
    );
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn installed_fixture_reports_revisioned_progress_and_completion() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Games");
    std::fs::create_dir_all(root.join(".caskade")).unwrap();
    std::fs::write(
        root.join(".caskade/root.json"),
        br#"{"schema_version":1,"root_id":"ipc-root"}"#,
    )
    .unwrap();
    let source = temp.path().join("Fixture Game");
    std::fs::create_dir(&source).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        source.join("fixture-game.x86_64"),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        source.join("fixture-game.x86_64"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(source.join("data.pak"), b"fixture payload").unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    store
        .upsert_root(&StorageRoot {
            id: "ipc-root".into(),
            path: root.clone(),
            is_default: true,
            label: None,
        })
        .await
        .unwrap();
    let bus = Bus::default();
    let service = InstallService::new(
        store.clone(),
        bus.clone(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let launch = LaunchService::new(store.clone()).with_aliases(None);
    let storage = StorageService::new(
        store.clone(),
        caskade_core::settings::SettingsService::new(store.clone()),
        launch.clone(),
    )
    .unwrap()
    .with_paths(caskade_core::AppPaths::under(&temp.path().join("App")));
    let second_root = temp.path().join("Other Games");
    std::fs::create_dir_all(second_root.join(".caskade")).unwrap();
    std::fs::write(
        second_root.join(".caskade/root.json"),
        br#"{"schema_version":1,"root_id":"second-root"}"#,
    )
    .unwrap();
    store
        .upsert_root(&StorageRoot {
            id: "second-root".into(),
            path: second_root.clone(),
            is_default: false,
            label: None,
        })
        .await
        .unwrap();
    let sink = Sink::default();
    crate::events::bridge(Events::new(sink.clone()), &bus);

    let queued = install_start(
        &service,
        source.to_str().unwrap().into(),
        Some("ipc-root".into()),
    )
    .await
    .unwrap();
    assert_eq!(queued.status, JobStatus::Queued);
    let id = queued.id.clone();
    let deadline = Instant::now() + Duration::from_secs(20);
    let finished = loop {
        let job = job_get(&service, id.clone()).unwrap();
        if job.status == JobStatus::Completed {
            break job;
        }
        assert_ne!(job.status, JobStatus::Failed, "{job:?}");
        assert!(Instant::now() < deadline, "{job:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let game_id = finished.game_id.clone().unwrap();
    let entries = store.entries_for_game(&game_id).await.unwrap();
    assert_eq!(entries.len(), 1);
    let listed = jobs_list(&service);
    assert_eq!(listed[0].id, id);
    let deadline = Instant::now() + Duration::from_secs(3);
    while sink
        .jobs()
        .last()
        .is_none_or(|job| job.status != JobStatus::Completed)
    {
        assert!(
            Instant::now() < deadline,
            "completion event missing: {:?}",
            sink.jobs()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        job_answer(
            &service,
            id.clone(),
            0,
            caskade_core::install::JobAnswer::Select { index: 999 }
        )
        .await
        .is_err()
    );
    assert!(job_retry(&service, id.clone(), Some(999)).await.is_err());
    let entry_id = entries[0].id.clone();
    let moved = move_game(&service, &storage, entry_id.clone(), "second-root".into())
        .await
        .unwrap();
    let moved = terminal(&service, &moved.id).await;
    assert_eq!(moved.status, JobStatus::Completed, "{moved:?}");
    assert_eq!(moved.phase, Some(JobPhase::Move));
    let updated = store.get_entry(&entry_id).await.unwrap().unwrap();
    assert!(
        matches!(updated.location,Some(caskade_core::model::InstallLocation::Managed{ref root_id,..}) if root_id=="second-root")
    );
    let removed = uninstall_game(&service, &launch, entry_id.clone())
        .await
        .unwrap();
    let removed = terminal(&service, &removed.id).await;
    assert_eq!(removed.status, JobStatus::Completed, "{removed:?}");
    assert!(store.get_entry(&entry_id).await.unwrap().is_none());
    assert!(source.exists());
    // The uninstall left the game without entries; removing it needs no job.
    let game_id = updated.game_id.clone();
    let gone = remove_game(&service, &launch, game_id.clone())
        .await
        .unwrap();
    assert!(gone.is_none());
    assert!(store.game(&game_id).await.unwrap().is_none());
    let events = sink.jobs();
    assert!(
        events
            .iter()
            .any(|job| job.phase == Some(JobPhase::Install))
    );
    let original = events
        .iter()
        .filter(|event| event.id == id)
        .collect::<Vec<_>>();
    assert!(
        original
            .windows(2)
            .all(|pair| pair[0].revision < pair[1].revision)
    );
}

#[cfg(target_os = "linux")]
async fn terminal(service: &InstallService, id: &str) -> JobSnapshot {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = job_get(service, id.into()).unwrap();
        if matches!(
            job.status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled | JobStatus::NeedsUser
        ) {
            return job;
        }
        assert!(Instant::now() < deadline, "job did not finish: {job:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A missing or rejected 7-Zip sidecar costs the archive formats that need
/// it, not the app: the install service still starts with an empty queue.
/// Not a tokio test: `services` waits on the app's runtime itself.
#[test]
fn services_without_the_7zip_sidecar_still_start() {
    let temp = tempfile::tempdir().unwrap();
    let paths = caskade_core::AppPaths::under(&temp.path().join("App"));
    let store = crate::runtime::block_on(LibraryStore::in_memory()).unwrap();
    let settings = caskade_core::settings::SettingsService::new(store.clone());
    let launch = LaunchService::new(store.clone()).with_aliases(None);
    let storage = StorageService::new(store.clone(), settings.clone(), launch)
        .unwrap()
        .with_paths(paths.clone());
    // Not there at all.
    let (_source, service) = services(
        store.clone(),
        Bus::default(),
        &paths,
        settings.clone(),
        Some(&storage),
        Ok(temp.path().join("7zz")),
    )
    .unwrap();
    assert!(service.jobs().is_empty());
    // Not even a path for it.
    let storage = StorageService::new(
        store.clone(),
        settings.clone(),
        LaunchService::new(store.clone()),
    )
    .unwrap()
    .with_paths(paths.clone());
    let (_source, service) = services(
        store,
        Bus::default(),
        &paths,
        settings,
        Some(&storage),
        Err(std::io::Error::other("no executable path")),
    )
    .unwrap();
    assert!(service.jobs().is_empty());
}
