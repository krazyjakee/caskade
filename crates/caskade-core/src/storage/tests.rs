use super::*;
use crate::{AppPaths, db::LibraryStore, launch::LaunchService, settings::SettingsService};
use caskade_installer::manifest::RootManifest;
use std::{fs, path::Path, sync::Arc};
fn service(store: LibraryStore, paths: AppPaths) -> StorageService {
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    StorageService::new(store.clone(), settings, LaunchService::new(store))
        .unwrap()
        .with_paths(paths)
}
#[tokio::test]
async fn roots_preserve_physical_identity_default_labels_templates_and_external_catalogue() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(&temp.path().join("app"));
    let db = LibraryStore::in_memory().await.unwrap();
    let service = service(db.clone(), paths.clone());
    let first = service
        .register_root(temp.path().join("one"), Some("Fast games".into()))
        .await
        .unwrap();
    assert!(first.root.is_default);
    assert!(first.capabilities.writable);
    assert!(first.capabilities.free_bytes.is_some());
    assert!(first.capabilities.total_bytes.is_some());
    assert_eq!(
        RootManifest::read(&first.root.path).unwrap().root_id(),
        first.root.id
    );
    assert!(matches!(
        service.remove_keep_files(&first.root.id).await,
        Err(StorageError::DefaultRoot)
    ));
    let second = service
        .register_root(temp.path().join("two"), None)
        .await
        .unwrap();
    assert!(!second.root.is_default);
    service
        .edit_root(
            &second.root.id,
            Some("Archive".into()),
            "Games - {title}".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        service.folder_template(&second.root.id).await.unwrap(),
        "Games - {title}"
    );
    assert!(
        service
            .edit_root(&second.root.id, None, "../{title}".into())
            .await
            .is_err()
    );
    service.set_default(&second.root.id).await.unwrap();
    assert_eq!(service.resolve(None).await.unwrap().id, second.root.id);
    assert_eq!(
        db.roots()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.is_default)
            .count(),
        1
    );
    service.remove_keep_files(&first.root.id).await.unwrap();
    assert!(first.root.path.is_dir());
    assert!(RootManifest::read(&first.root.path).is_ok());
    let fresh = LibraryStore::in_memory().await.unwrap();
    let restored = super::tests::service(fresh.clone(), paths);
    assert_eq!(restored.restore_roots().await.unwrap(), 1);
    assert_eq!(restored.resolve(None).await.unwrap().id, second.root.id);
    db.close().await;
    fresh.close().await;
}
#[tokio::test]
async fn first_start_registers_the_games_folder_as_default() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let db = LibraryStore::in_memory().await.unwrap();
    let service = service(db.clone(), paths.clone());
    service.initialize().await.unwrap();
    let root = service.resolve(None).await.unwrap();
    assert_eq!(root.path, fs::canonicalize(&paths.games).unwrap());
    assert!(root.is_default);
    // Once any location exists, nothing more is added.
    assert!(service.ensure_default_root().await.unwrap().is_none());
    let fresh = LibraryStore::in_memory().await.unwrap();
    let restored = super::tests::service(fresh.clone(), paths);
    restored.initialize().await.unwrap();
    assert_eq!(fresh.roots().await.unwrap().len(), 1);
    assert_eq!(restored.resolve(None).await.unwrap().id, root.id);
    db.close().await;
    fresh.close().await;
}
#[tokio::test]
async fn offline_remount_and_duplicate_root_identity_are_conservative() {
    let temp = tempfile::tempdir().unwrap();
    let db = LibraryStore::in_memory().await.unwrap();
    let service = service(db.clone(), AppPaths::under(&temp.path().join("app")));
    let one = service
        .register_root(temp.path().join("one"), None)
        .await
        .unwrap();
    let copy = temp.path().join("copied");
    fs::create_dir(&copy).unwrap();
    RootManifest::new(&one.root.id)
        .unwrap()
        .write(&copy)
        .unwrap();
    assert!(service.register_root(copy.clone(), None).await.is_err());
    let parked = temp.path().join("parked");
    fs::rename(&one.root.path, &parked).unwrap();
    assert_eq!(
        service.list_roots().await.unwrap()[0].status,
        RootStatus::Offline
    );
    assert!(matches!(
        service.resolve(None).await,
        Err(StorageError::Offline)
    ));
    fs::rename(&parked, &one.root.path).unwrap();
    assert_eq!(
        service.list_roots().await.unwrap()[0].status,
        RootStatus::Online
    );
    RootManifest::new("replacement-drive")
        .unwrap()
        .write(&one.root.path)
        .unwrap();
    assert_eq!(
        service.list_roots().await.unwrap()[0].status,
        RootStatus::IdentityMismatch
    );
    fs::rename(&one.root.path, &parked).unwrap();
    let adopted = service.register_root(copy, None).await.unwrap();
    assert_eq!(adopted.root.id, one.root.id);
    assert!(adopted.root.is_default);
    db.close().await;
}
struct Limited;
impl RootProbe for Limited {
    fn probe(&self, _: &Path) -> std::io::Result<RootCapabilities> {
        Ok(RootCapabilities {
            writable: false,
            free_bytes: Some(1),
            total_bytes: Some(2),
            warnings: vec!["Read-only fixture".into()],
            ..Default::default()
        })
    }
}
#[tokio::test]
async fn root_probe_failures_never_publish_a_new_identity() {
    let temp = tempfile::tempdir().unwrap();
    let db = LibraryStore::in_memory().await.unwrap();
    let path = temp.path().join("readonly");
    let service = service(db.clone(), AppPaths::under(&temp.path().join("app")))
        .with_probe(Arc::new(Limited));
    assert!(matches!(
        service.register_root(path.clone(), None).await,
        Err(StorageError::ReadOnly)
    ));
    assert!(!path.join(".caskade/root.json").exists());
    assert!(db.roots().await.unwrap().is_empty());
    db.close().await;
}
/// Counts full probes; the default `refresh` keeps the cached result.
struct Counting(std::sync::atomic::AtomicUsize);
impl RootProbe for Counting {
    fn probe(&self, _: &Path) -> std::io::Result<RootCapabilities> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(RootCapabilities {
            writable: true,
            executable: true,
            symlinks: true,
            ..Default::default()
        })
    }
}
#[tokio::test]
async fn listing_reuses_probes_until_a_root_is_edited_or_returns_online() {
    use std::sync::atomic::Ordering::SeqCst;
    let temp = tempfile::tempdir().unwrap();
    let db = LibraryStore::in_memory().await.unwrap();
    let probe = Arc::new(Counting(Default::default()));
    let service =
        service(db.clone(), AppPaths::under(&temp.path().join("app"))).with_probe(probe.clone());
    let root = service
        .register_root(temp.path().join("one"), None)
        .await
        .unwrap()
        .root;
    assert_eq!(probe.0.load(SeqCst), 1);
    for _ in 0..3 {
        assert_eq!(
            service.list_roots().await.unwrap()[0].status,
            RootStatus::Online
        );
    }
    service.resolve(None).await.unwrap();
    assert_eq!(probe.0.load(SeqCst), 1, "listing must not probe again");
    service
        .edit_root(&root.id, Some("Renamed".into()), "{title}".into())
        .await
        .unwrap();
    assert_eq!(probe.0.load(SeqCst), 2, "an edit measures again");
    let parked = temp.path().join("parked");
    fs::rename(&root.path, &parked).unwrap();
    assert_eq!(
        service.list_roots().await.unwrap()[0].status,
        RootStatus::Offline
    );
    assert_eq!(probe.0.load(SeqCst), 2, "an offline root is not probed");
    fs::rename(&parked, &root.path).unwrap();
    assert_eq!(
        service.list_roots().await.unwrap()[0].status,
        RootStatus::Online
    );
    assert_eq!(probe.0.load(SeqCst), 3, "a root back online is probed");
    service.list_roots().await.unwrap();
    assert_eq!(probe.0.load(SeqCst), 3);
    db.close().await;
}
#[cfg(target_os = "linux")]
#[test]
fn filesystem_refresh_keeps_space_live_and_rejects_a_changed_mount() {
    let temp = tempfile::tempdir().unwrap();
    let caps = FileSystemProbe.probe(temp.path()).unwrap();
    assert!(caps.writable);
    let refreshed = FileSystemProbe.refresh(temp.path(), &caps).unwrap();
    assert!(refreshed.free_bytes.is_some());
    assert_eq!(refreshed.filesystem, caps.filesystem);
    let moved = RootCapabilities {
        volume: Some("another-volume".into()),
        ..caps.clone()
    };
    assert!(FileSystemProbe.refresh(temp.path(), &moved).is_none());
    let read_only = RootCapabilities {
        writable: false,
        ..caps
    };
    assert!(
        FileSystemProbe.refresh(temp.path(), &read_only).is_none(),
        "a rw mount the cache calls read-only is measured again"
    );
}
#[cfg(target_os = "linux")]
mod moving {
    use super::*;
    use crate::model::{Collection, Game, GameEntry, InstallLocation, LaunchSpec, PlaySession};
    use caskade_discovery::{InstallState, SourceId};
    use caskade_installer::{
        events::{EventSink, JobEvent},
        manifest::{InstallManifest, RelocationVerdict},
    };
    use std::{collections::BTreeMap, time::Duration};
    use tokio_util::sync::CancellationToken;
    struct Sink;
    impl EventSink for Sink {
        fn emit(&self, _: JobEvent) {}
    }
    struct PanicAtMoved;
    impl EventSink for PanicAtMoved {
        fn emit(&self, event: JobEvent) {
            if event.message == "moved" {
                panic!("simulated app shutdown before the worker can compensate")
            }
        }
    }
    async fn setup(temp: &Path) -> (StorageService, Game, GameEntry, RootSummary, RootSummary) {
        let db = LibraryStore::in_memory().await.unwrap();
        let paths = AppPaths::under(&temp.join("app"));
        let settings = SettingsService::new(db.clone());
        settings.register_core(&paths).unwrap();
        let launch = LaunchService::new(db.clone())
            .with_aliases(None)
            .with_config(crate::launch::LaunchConfig {
                poll_interval: Duration::from_millis(10),
                exit_grace: Duration::from_millis(20),
                heartbeat_interval: Duration::from_millis(10),
                launch_timeout: Duration::from_secs(2),
            });
        let service = StorageService::new(db.clone(), settings, launch)
            .unwrap()
            .with_paths(paths);
        let first = service.register_root(temp.join("one"), None).await.unwrap();
        let second = service.register_root(temp.join("two"), None).await.unwrap();
        let game_dir = first.root.path.join("Fixture");
        fs::create_dir(&game_dir).unwrap();
        fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
            ),
            game_dir.join("game"),
        )
        .unwrap();
        let mut record = InstallManifest::from_json(include_bytes!(
            "../../../caskade-installer/tests/manifests/install.v1.posix.json"
        ))
        .unwrap()
        .into_record();
        record.plan.root_id = first.root.id.clone();
        record.plan.relative_dir = "Fixture".into();
        record.files.clear();
        record.alias = None;
        record.runtime = caskade_installer::RuntimeSpec::Native;
        record.relocation.verdict = RelocationVerdict::Free;
        record.relocation.references.clear();
        record.relocation.last_path = Some(game_dir.to_string_lossy().into_owned());
        InstallManifest::new(record)
            .unwrap()
            .write(&game_dir)
            .unwrap();
        let mut game = Game::new("Fixture");
        game.favorite = true;
        game.notes = "Preserved user note".into();
        db.upsert_game(&game).await.unwrap();
        let mut entry = GameEntry::new(
            &game.id,
            SourceId::new("caskade-storage"),
            "fixture-install-id",
        );
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::Managed {
            root_id: first.root.id.clone(),
            rel_dir: "Fixture".into(),
        });
        entry.launch = Some(LaunchSpec::Executable {
            path: "game".into(),
            args: vec![
                "--token".into(),
                temp.join("launch-token").to_string_lossy().into_owned(),
            ],
            cwd: Some(".".into()),
            env: BTreeMap::new(),
        });
        db.upsert_entry(&entry).await.unwrap();
        (service, game, entry, first, second)
    }
    #[tokio::test]
    async fn listing_rereads_a_changed_manifest_and_leaves_offline_roots_alone() {
        let temp = tempfile::tempdir().unwrap();
        let (service, _, entry, first, _) = setup(temp.path()).await;
        let listed = |roots: Vec<RootSummary>| {
            roots
                .into_iter()
                .find(|r| r.root.id == first.root.id)
                .unwrap()
        };
        let summary = listed(service.list_roots().await.unwrap());
        assert_eq!(summary.games.len(), 1);
        assert_eq!(summary.games[0].entry_id, entry.id);
        assert_eq!(summary.games[0].title, "Fixture");
        assert!(summary.games[0].available);
        assert_eq!(summary.games[0].move_blocked, None);
        let game_dir = first.root.path.join("Fixture");
        let mut record = InstallManifest::read(&game_dir).unwrap().into_record();
        record.relocation.verdict = RelocationVerdict::Pinned;
        InstallManifest::new(record)
            .unwrap()
            .write(&game_dir)
            .unwrap();
        let summary = listed(service.list_roots().await.unwrap());
        assert_eq!(
            summary.games[0].move_blocked.as_deref(),
            Some("This installation is managed by its native package manager.")
        );
        let parked = temp.path().join("parked");
        fs::rename(&first.root.path, &parked).unwrap();
        let summary = listed(service.list_roots().await.unwrap());
        assert_eq!(summary.status, RootStatus::Offline);
        assert_eq!(summary.games[0].title, "Fixture");
        assert!(!summary.games[0].available);
        assert_eq!(
            summary.games[0].move_blocked.as_deref(),
            Some("Storage location is unavailable.")
        );
    }
    #[tokio::test]
    async fn move_preserves_library_identity_sessions_collections_and_launches() {
        let temp = tempfile::tempdir().unwrap();
        let (service, game, entry, first, second) = setup(temp.path()).await;
        let mut session = PlaySession::new(&game.id, &entry.id, 1000);
        session.ended_at_ms = Some(2000);
        session.duration_ms = 1000;
        session.heartbeat_at_ms = 2000;
        service.store.upsert_session(&session).await.unwrap();
        let mut collection = Collection::manual("Favourites");
        collection.game_ids.push(game.id.clone());
        let account = service.store.accounts().await.unwrap()[0].id.clone();
        service
            .store
            .upsert_collection(&account, &collection)
            .await
            .unwrap();
        let held = service.launch.try_game_lease(&game.id).unwrap();
        assert!(matches!(
            service
                .move_entry(
                    &entry.id,
                    &second.root.id,
                    "locked",
                    Arc::new(Sink),
                    CancellationToken::new()
                )
                .await,
            Err(StorageError::Running)
        ));
        drop(held);
        let report = service
            .move_entry(
                &entry.id,
                &second.root.id,
                "move-fixture",
                Arc::new(Sink),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!first.root.path.join("Fixture").exists());
        assert!(report.directory.join("game").exists());
        let moved = service.store.get_entry(&entry.id).await.unwrap().unwrap();
        assert_eq!(moved.id, entry.id);
        assert_eq!(moved.external_id, entry.external_id);
        assert_eq!(
            moved.location,
            Some(InstallLocation::Managed {
                root_id: second.root.id,
                rel_dir: "Fixture".into()
            })
        );
        assert_eq!(service.store.game(&game.id).await.unwrap(), Some(game));
        assert_eq!(
            service.store.get_session(&session.id).await.unwrap(),
            Some(session)
        );
        assert_eq!(
            service
                .store
                .get_collection(&account, &collection.id)
                .await
                .unwrap(),
            Some(collection)
        );
        service
            .launch
            .launch(&entry.id)
            .await
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert!(temp.path().join("launch-token").is_file());
        service.store.close().await;
    }
    #[tokio::test]
    async fn startup_recovery_repairs_a_worker_drop_before_database_update() {
        let temp = tempfile::tempdir().unwrap();
        let (service, _, entry, first, second) = setup(temp.path()).await;
        assert!(
            service
                .move_entry(
                    &entry.id,
                    &second.root.id,
                    "interrupted",
                    Arc::new(PanicAtMoved),
                    CancellationToken::new()
                )
                .await
                .is_err()
        );
        assert!(!first.root.path.join("Fixture").exists());
        assert!(second.root.path.join("Fixture").exists());
        assert_eq!(
            service
                .recover_pending(Arc::new(Sink), CancellationToken::new())
                .await
                .unwrap(),
            vec!["interrupted"]
        );
        assert!(first.root.path.join("Fixture").is_dir());
        assert!(!second.root.path.join("Fixture").exists());
        assert_eq!(
            service.store.get_entry(&entry.id).await.unwrap(),
            Some(entry)
        );
        assert!(
            service
                .recover_pending(Arc::new(Sink), CancellationToken::new())
                .await
                .unwrap()
                .is_empty()
        );
        service.store.close().await;
    }

    #[tokio::test]
    async fn startup_recovery_continues_past_an_intent_it_cannot_recover() {
        let temp = tempfile::tempdir().unwrap();
        let (service, _, entry, first, second) = setup(temp.path()).await;
        assert!(
            service
                .move_entry(
                    &entry.id,
                    &second.root.id,
                    "interrupted",
                    Arc::new(PanicAtMoved),
                    CancellationToken::new()
                )
                .await
                .is_err()
        );
        // Sorted before the good one, so it is met first.
        service
            .store
            .set_setting("storage.move.broken", &serde_json::json!({"damaged": true}))
            .await
            .unwrap();
        assert_eq!(
            service
                .recover_pending(Arc::new(Sink), CancellationToken::new())
                .await
                .unwrap(),
            vec!["interrupted"]
        );
        assert!(first.root.path.join("Fixture").is_dir());
        assert_eq!(
            service.store.get_entry(&entry.id).await.unwrap(),
            Some(entry)
        );
        // The damaged record stays for a later start.
        assert_eq!(
            service
                .store
                .settings_with_prefix("storage.move.")
                .await
                .unwrap()
                .len(),
            1
        );
        service.store.close().await;
    }

    #[tokio::test]
    async fn a_poisoned_cache_lock_does_not_break_later_calls() {
        let temp = tempfile::tempdir().unwrap();
        let (service, ..) = setup(temp.path()).await;
        service.poison_caches();
        let roots = service.list_roots().await.unwrap();
        assert_eq!(roots.len(), 2);
        assert!(roots.iter().all(|root| root.status == RootStatus::Online));
        service.store.close().await;
    }

    #[tokio::test]
    async fn move_never_steals_an_alias_from_an_existing_copy() {
        let temp = tempfile::tempdir().unwrap();
        let (service, _, entry, first, second) = setup(temp.path()).await;
        let source = first.root.path.join("Fixture");
        let other = temp.path().join("other-installation");
        fs::create_dir(&other).unwrap();
        let aliases =
            caskade_installer::runtime::alias::AliasStore::new(service.paths.data.join("g"));
        aliases.link("fixture-alias", &other).unwrap();
        let mut record = InstallManifest::read(&source).unwrap().into_record();
        record.alias = Some("fixture-alias".into());
        InstallManifest::new(record)
            .unwrap()
            .write(&source)
            .unwrap();
        let error = service
            .move_entry(
                &entry.id,
                &second.root.id,
                "alias-conflict",
                Arc::new(Sink),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("another existing installation"));
        assert!(source.is_dir());
        assert!(!second.root.path.join("Fixture").exists());
        assert_eq!(aliases.target("fixture-alias").unwrap(), Some(other));
        service.store.close().await;
    }
}

#[cfg(all(target_os = "linux", feature = "net"))]
#[tokio::test(flavor = "multi_thread")]
async fn install_uses_the_selected_root_and_its_folder_template() {
    use crate::{
        bus::Bus,
        install::{InstallOptions, InstallService, JobStatus},
    };
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let storage = service(store.clone(), AppPaths::under(&temp.path().join("app")));
    let default = storage
        .register_root(temp.path().join("Default"), None)
        .await
        .unwrap();
    let chosen = storage
        .register_root(temp.path().join("Selected"), None)
        .await
        .unwrap();
    storage
        .edit_root(&chosen.root.id, None, "Library {title}".into())
        .await
        .unwrap();
    let input = temp.path().join("Fixture Runner");
    fs::create_dir(&input).unwrap();
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        input.join("FixtureRunner.x86_64"),
    )
    .unwrap();
    let jobs = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            aliases: None,
            ..Default::default()
        },
    )
    .with_storage(storage);
    let job = jobs
        .install(
            input.to_string_lossy().into_owned(),
            Some(chosen.root.id.clone()),
        )
        .await
        .unwrap();
    let finished = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let snapshot = jobs.job(&job.id).unwrap();
            if snapshot.status.is_finished() || snapshot.status == JobStatus::NeedsUser {
                break snapshot;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(finished.status, JobStatus::Completed, "{finished:?}");
    assert!(
        store
            .entries_for_root(&default.root.id)
            .await
            .unwrap()
            .is_empty()
    );
    let installed = store.entries_for_root(&chosen.root.id).await.unwrap();
    assert_eq!(installed.len(), 1);
    let Some(crate::model::InstallLocation::Managed { root_id, rel_dir }) = &installed[0].location
    else {
        panic!("missing managed install")
    };
    assert_eq!(root_id, &chosen.root.id);
    assert_eq!(rel_dir, Path::new("Library Fixture Runner"));
    assert!(
        chosen
            .root
            .path
            .join(rel_dir)
            .join("FixtureRunner.x86_64")
            .is_file()
    );
    store.close().await;
}

#[test]
fn rejected_catalogue_budget_preserves_the_previous_recoverable_roots() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::model::StorageRoot {
        id: "valid-root".into(),
        path: temp.path().join("games"),
        is_default: true,
        label: None,
    };
    super::catalogue::write(temp.path(), std::slice::from_ref(&root)).unwrap();
    assert!(super::catalogue::write(temp.path(), &vec![root.clone(); 129]).is_err());
    assert_eq!(super::catalogue::read(temp.path()).unwrap(), vec![root]);
}
