#![cfg(target_os = "linux")]
//! #64: a location's games move as one job, then the location goes.
use super::*;
use crate::{
    AppPaths,
    bus::{Intercept, Interceptor},
    install::hooks::{InstallMoved, MovedGame},
    launch::LaunchService,
    model::{GameEntry, InstallLocation},
    settings::SettingsService,
};
use async_trait::async_trait;
use caskade_discovery::SourceId;
use std::{path::Path, sync::Mutex, time::Duration};

struct Setup {
    _temp: tempfile::TempDir,
    store: LibraryStore,
    bus: Bus,
    launch: LaunchService,
    service: InstallService,
    storage: StorageService,
    /// The default location, where the games move to.
    target: StorageRoot,
    /// The location the games start on.
    source: StorageRoot,
}

async fn stopped(service: &InstallService, id: &str) -> JobSnapshot {
    for _ in 0..600 {
        let job = service.job(id).unwrap();
        let active = lock(&service.inner.jobs).get(id).unwrap().active;
        if !active && job.status.is_finished() {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job {id} did not finish: {:?}", service.job(id));
}

/// Two locations, with the games `titles` installed on the second.
async fn setup(titles: &[&str]) -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(&temp.path().join("app"));
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let launch = LaunchService::new(store.clone()).with_aliases(None);
    let storage = StorageService::new(store.clone(), settings, launch.clone())
        .unwrap()
        .with_paths(paths);
    let target = storage
        .register_root(temp.path().join("one"), Some("Archive".into()))
        .await
        .unwrap()
        .root;
    let source = storage
        .register_root(temp.path().join("two"), Some("Fast SSD".into()))
        .await
        .unwrap()
        .root;
    let bus = Bus::default();
    let service = InstallService::new(
        store.clone(),
        bus.clone(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    )
    .with_storage(storage.clone());
    for (index, title) in titles.iter().enumerate() {
        let folder = temp.path().join("sources").join(title);
        std::fs::create_dir_all(&folder).unwrap();
        // Sizes that differ, so the games have one order.
        std::fs::write(folder.join("data.pak"), vec![7u8; 1024 * (index + 1)]).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"),
            folder.join(format!("{title}.x86_64")),
        )
        .unwrap();
        let job = service
            .install(
                folder.to_string_lossy().into_owned(),
                Some(source.id.clone()),
            )
            .await
            .unwrap();
        let installed = stopped(&service, &job.id).await;
        assert_eq!(installed.status, JobStatus::Completed, "{installed:?}");
    }
    Setup {
        _temp: temp,
        store,
        bus,
        launch,
        service,
        storage,
        target,
        source,
    }
}

impl Setup {
    /// `(game id, game folder)` of each game on `root`.
    async fn games_on(&self, root: &StorageRoot) -> Vec<(String, std::path::PathBuf)> {
        self.store
            .entries_for_root(&root.id)
            .await
            .unwrap()
            .into_iter()
            .map(|entry| {
                let Some(InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
                    panic!("{entry:?}");
                };
                (entry.game_id.clone(), root.path.join(rel_dir))
            })
            .collect()
    }

    async fn start(&self) -> crate::Result<JobSnapshot> {
        self.service
            .move_location(
                self.source.id.clone(),
                self.target.id.clone(),
                self.storage.clone(),
            )
            .await
    }

    /// Runs `then` when the first game has moved, before the next starts.
    fn after_first_move(&self, then: impl FnOnce(&MovedGame) + Send + 'static) {
        self.bus
            .intercept::<InstallMoved>("test", 0, Arc::new(AfterFirst(Mutex::new(Some(then)))));
    }
}

struct AfterFirst<F>(Mutex<Option<F>>);

#[async_trait]
impl<F: FnOnce(&MovedGame) + Send + 'static> Interceptor<InstallMoved> for AfterFirst<F> {
    async fn intercept(
        &self,
        moved: MovedGame,
    ) -> std::result::Result<Intercept<MovedGame>, String> {
        let then = self.0.lock().unwrap().take();
        if let Some(then) = then {
            then(&moved);
        }
        Ok(Intercept::Continue(moved))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_game_moves_then_the_location_goes() {
    let s = setup(&["Alpha", "Bravo", "Charlie"]).await;
    let games = s.games_on(&s.source).await;
    assert_eq!(games.len(), 3);
    let job = s.start().await.unwrap();
    assert_eq!(job.kind, JobKind::MoveLocation);
    assert_eq!(job.title, "Games on Fast SSD");
    let done = stopped(&s.service, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");

    let progress = done.progress.unwrap();
    assert!(progress.bytes > 0);
    assert_eq!(Some(progress.bytes), progress.total_bytes, "the whole job");
    assert!(
        progress.message.ends_with("(3 of 3)"),
        "{}",
        progress.message
    );
    assert_eq!(done.target_dir.as_ref(), Some(&s.target.path));

    let moved = s.games_on(&s.target).await;
    let mut ids: Vec<_> = moved.iter().map(|(id, _)| id.clone()).collect();
    let mut before: Vec<_> = games.iter().map(|(id, _)| id.clone()).collect();
    ids.sort();
    before.sort();
    assert_eq!(ids, before, "the same library games");
    for (_, folder) in moved {
        assert!(folder.join("data.pak").is_file());
    }
    assert!(s.store.get_root(&s.source.id).await.unwrap().is_none());
    assert!(s.source.path.is_dir(), "its folder stays on disk");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_keeps_the_location_and_the_games_already_moved() {
    let s = setup(&["Alpha", "Bravo", "Charlie"]).await;
    let (service, job_id) = (s.service.clone(), Arc::new(Mutex::new(String::new())));
    let id = job_id.clone();
    // The job is known by the time its first game has moved.
    s.after_first_move(move |moved| {
        *id.lock().unwrap() = moved.job_id.clone();
        service.cancel(&moved.job_id).unwrap();
    });
    let job = s.start().await.unwrap();
    let ended = stopped(&s.service, &job.id).await;
    assert_eq!(ended.status, JobStatus::Cancelled, "{ended:?}");
    assert_eq!(*job_id.lock().unwrap(), job.id);
    assert_eq!(
        ended.error.as_deref(),
        Some("Cancelled. One game moved to Archive. Fast SSD was kept.")
    );
    assert!(s.store.get_root(&s.source.id).await.unwrap().is_some());
    assert_eq!(s.games_on(&s.target).await.len(), 1);
    let left = s.games_on(&s.source).await;
    assert_eq!(left.len(), 2);
    for (_, folder) in left {
        assert!(folder.join("data.pak").is_file());
    }
    assert!(
        s.service.retry(&job.id, None).await.is_err(),
        "pressing Move again starts a new job"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_move_keeps_the_location_and_says_what_moved() {
    let s = setup(&["Alpha", "Bravo", "Charlie"]).await;
    let games = s.games_on(&s.source).await;
    let held = Arc::new(Mutex::new(Vec::new()));
    let (launch, leases) = (s.launch.clone(), held.clone());
    // A game starts while the others move.
    s.after_first_move(move |moved| {
        for (game, _) in &games {
            if *game != moved.game_id {
                leases
                    .lock()
                    .unwrap()
                    .push(launch.try_game_lease(game).unwrap());
            }
        }
    });
    let job = s.start().await.unwrap();
    let ended = stopped(&s.service, &job.id).await;
    assert_eq!(ended.status, JobStatus::Failed, "{ended:?}");
    let error = ended.error.unwrap();
    assert!(error.starts_with("One game moved to Archive. “"), "{error}");
    assert!(error.contains("wasn't moved:"), "{error}");
    assert!(error.ends_with("Fast SSD was kept."), "{error}");
    assert!(s.store.get_root(&s.source.id).await.unwrap().is_some());
    assert_eq!(s.games_on(&s.target).await.len(), 1);
    assert_eq!(s.games_on(&s.source).await.len(), 2);

    // Once they're free, moving again takes the rest and the location.
    held.lock().unwrap().clear();
    let again = s.start().await.unwrap();
    let done = stopped(&s.service, &again.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert_eq!(s.games_on(&s.target).await.len(), 3);
    assert!(s.store.get_root(&s.source.id).await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn what_cant_finish_is_refused_before_anything_moves() {
    let s = setup(&["Alpha", "Bravo"]).await;
    let refused = |error: crate::Error| error.to_string();
    // The default location can't be removed, nor a location moved onto itself.
    let default = s
        .service
        .move_location(s.target.id.clone(), s.source.id.clone(), s.storage.clone())
        .await;
    assert!(default.is_err());
    let itself = s
        .service
        .move_location(s.source.id.clone(), s.source.id.clone(), s.storage.clone())
        .await;
    assert!(itself.is_err());
    // A game that's running.
    let (game, _) = s.games_on(&s.source).await.remove(0);
    let lease = s.launch.try_game_lease(&game).unwrap();
    let running = refused(s.start().await.unwrap_err());
    assert!(running.contains("is running"), "{running}");
    drop(lease);
    // A game that can't move: its manifest is gone.
    let (_, folder) = s.games_on(&s.source).await.remove(1);
    std::fs::remove_file(folder.join(caskade_installer::manifest::INSTALL_MANIFEST_PATH)).unwrap();
    let blocked = refused(s.start().await.unwrap_err());
    assert!(blocked.contains("can't move"), "{blocked}");

    assert!(
        s.service
            .jobs()
            .iter()
            .all(|job| job.kind != JobKind::MoveLocation)
    );
    assert_eq!(s.games_on(&s.source).await.len(), 2);
    // A location that still has games isn't removed as emptied.
    assert!(s.storage.remove_emptied(&s.source.id).await.is_err());
    assert!(s.store.get_root(&s.source.id).await.unwrap().is_some());
}

/// What Settings → Storage says happens to a location's games (#64).
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_location_takes_its_games_out_and_leaves_their_files() {
    let s = setup(&["Alpha", "Bravo"]).await;
    let games = s.games_on(&s.source).await;
    let (only_here, also_elsewhere) = (&games[0], &games[1]);
    let other = GameEntry::new(&also_elsewhere.0, SourceId::new("manual"), "other-copy");
    let other = s.store.upsert_entry(&other).await.unwrap();

    s.storage.remove_keep_files(&s.source.id).await.unwrap();

    assert!(s.store.game(&only_here.0).await.unwrap().is_none());
    assert!(s.store.game(&also_elsewhere.0).await.unwrap().is_some());
    let left = s.store.entries_for_game(&also_elsewhere.0).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, other.id, "only the location's copy went");
    for (_, folder) in &games {
        assert!(folder.join("data.pak").is_file());
    }
}
