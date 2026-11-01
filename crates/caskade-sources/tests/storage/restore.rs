//! Restoring the library from managed roots after the database is lost, and
//! what a missing or legacy install becomes.
use super::*;

#[tokio::test]
async fn deleting_database_then_rescanning_roots_restores_every_managed_install() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("library.sqlite");
    let store = LibraryStore::open(&database).await.unwrap();
    let first = root(&store, &temp.path().join("root-a"), "root-a").await;
    let second = root(&store, &temp.path().join("root-b"), "root-b").await;
    let explicit_id = new_id();
    let a = game(
        &first,
        "Game A",
        "Managed A",
        Some(&explicit_id),
        RelocationVerdict::Free,
    );
    let b = game(
        &first,
        "Nested/Game B",
        "Managed B",
        None,
        RelocationVerdict::Free,
    );
    let c = game(
        &second,
        "Game C",
        "Managed C",
        None,
        RelocationVerdict::Free,
    );
    let report = source(&store)
        .refresh(CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.imported(), 3, "{report:?}");
    let ids: BTreeSet<_> = [a, b, c]
        .iter()
        .map(|p| InstallManifest::read(p).unwrap().record().ids[INSTALL_ID].clone())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&explicit_id));
    assert_eq!(store.library_stats().await.unwrap().installed, 3);
    for root in [&first, &second] {
        for entry in store.entries_for_root(&root.id).await.unwrap() {
            assert!(matches!(
                entry.location,
                Some(InstallLocation::Managed { .. })
            ));
            assert!(ids.contains(&entry.external_id));
        }
    }
    store.close().await;
    fs::remove_file(&database).unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(PathBuf::from(format!("{}{suffix}", database.display())));
    }
    let store = LibraryStore::open(&database).await.unwrap();
    assert_eq!(store.library_stats().await.unwrap().total, 0);
    // The independent root catalogue (#2) restores these physical root IDs;
    // scanning derives every game solely from its on-disk install manifest.
    for previous in [&first, &second] {
        let identity = RootManifest::read(&previous.path).unwrap();
        store
            .upsert_root(&StorageRoot::from_manifest(
                previous.path.clone(),
                &identity,
            ))
            .await
            .unwrap();
    }
    let rebuilt = source(&store)
        .refresh(CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(rebuilt.imported(), 3, "{rebuilt:?}");
    let page = store
        .list_games(&ListGames {
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        page.items
            .into_iter()
            .map(|g| g.title)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Managed A".into(), "Managed B".into(), "Managed C".into()])
    );
    let mut rebuilt_ids = BTreeSet::new();
    for root in [&first, &second] {
        rebuilt_ids.extend(
            store
                .entries_for_root(&root.id)
                .await
                .unwrap()
                .into_iter()
                .map(|e| e.external_id),
        );
    }
    assert_eq!(rebuilt_ids, ids);
}

#[tokio::test]
async fn missing_game_becomes_partial_but_the_game_and_sessions_remain() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = root(&store, &temp.path().join("root"), "root-removed").await;
    let id = new_id();
    let path = game(&root, "Game", "Removed", Some(&id), RelocationVerdict::Free);
    let source = source(&store);
    source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    let entry = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    let mut session = PlaySession::new(&entry.game_id, &entry.id, 0);
    session.ended_at_ms = Some(0);
    store.upsert_session(&session).await.unwrap();
    fs::remove_dir_all(path).unwrap();
    let report = source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.imported(), 0);
    let current = store.get_entry(&entry.id).await.unwrap().unwrap();
    assert_eq!(current.state, caskade_discovery::InstallState::Partial);
    assert_eq!(store.get_session(&session.id).await.unwrap(), Some(session));
    assert_eq!(store.library_stats().await.unwrap().total, 1);
}

#[tokio::test]
async fn legacy_manifest_without_old_path_is_restored_for_explicit_repair() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = root(&store, &temp.path().join("root"), "root-legacy").await;
    let path = game(
        &root,
        "Legacy",
        "Legacy",
        None,
        RelocationVerdict::Unscanned,
    );
    let mut record = InstallManifest::read(&path).unwrap().into_record();
    record.relocation.last_path = None;
    InstallManifest::new(record).unwrap().write(&path).unwrap();
    let report = source(&store)
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.imported(), 1, "{report:?}");
    assert_eq!(report.games[0].status, StorageGameStatus::NeedsUser);
    assert!(report.games[0].question.is_some());
    let id = InstallManifest::read(&path).unwrap().record().ids[INSTALL_ID].clone();
    let entry = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        entry.state,
        caskade_discovery::InstallState::Broken { .. }
    ));
    assert_eq!(
        entry.location,
        Some(InstallLocation::Managed {
            root_id: root.id,
            rel_dir: "Legacy".into()
        })
    );
}

#[tokio::test]
async fn durable_root_catalogue_restores_registry_then_games_after_database_deletion() {
    use caskade_core::{
        AppPaths,
        launch::LaunchService,
        settings::SettingsService,
        storage::{RootCapabilities, RootProbe, StorageService},
    };
    struct Writable;
    impl RootProbe for Writable {
        fn probe(&self, _: &Path) -> std::io::Result<RootCapabilities> {
            Ok(RootCapabilities {
                writable: true,
                executable: true,
                symlinks: true,
                case_sensitive: true,
                free_bytes: Some(1_000_000),
                total_bytes: Some(2_000_000),
                ..Default::default()
            })
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(&temp.path().join("app"));
    let store = LibraryStore::open_default(&paths).await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let registry = StorageService::new(store.clone(), settings, LaunchService::new(store.clone()))
        .unwrap()
        .with_paths(paths.clone())
        .with_probe(Arc::new(Writable));
    let first = registry
        .register_root(temp.path().join("drive-a"), Some("First drive".into()))
        .await
        .unwrap()
        .root;
    let second = registry
        .register_root(temp.path().join("drive-b"), Some("Second drive".into()))
        .await
        .unwrap()
        .root;
    let id_a = new_id();
    let id_b = new_id();
    game(
        &first,
        "One",
        "Catalogue one",
        Some(&id_a),
        RelocationVerdict::Free,
    );
    game(
        &second,
        "Two",
        "Catalogue two",
        Some(&id_b),
        RelocationVerdict::Free,
    );
    assert_eq!(
        source(&store)
            .refresh(CancellationToken::new())
            .await
            .unwrap()
            .imported(),
        2
    );
    assert!(paths.config.join("storage-roots.json").is_file());
    drop(registry);
    store.close().await;
    fs::remove_file(paths.database()).unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(PathBuf::from(format!(
            "{}{suffix}",
            paths.database().display()
        )));
    }
    let store = LibraryStore::open_default(&paths).await.unwrap();
    assert!(store.roots().await.unwrap().is_empty());
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let registry = StorageService::new(store.clone(), settings, LaunchService::new(store.clone()))
        .unwrap()
        .with_paths(paths)
        .with_probe(Arc::new(Writable));
    assert_eq!(registry.restore_roots().await.unwrap(), 2);
    let report = source(&store)
        .refresh(CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.imported(), 2, "{report:?}");
    assert!(
        store
            .entry_by_source("caskade-storage", &id_a)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .entry_by_source("caskade-storage", &id_b)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store
            .roots()
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first.id, second.id])
    );
}
