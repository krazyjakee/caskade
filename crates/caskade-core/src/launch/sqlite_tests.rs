#![cfg(target_os = "linux")]
use super::{
    test_support::{Events, fast_config},
    *,
};
use crate::{
    db::LibraryStore,
    model::{Game, GameEntry, InstallLocation, LaunchSpec, StorageRoot},
};
use caskade_discovery::{InstallState, SourceId};
use caskade_installer::runtime::alias::AliasStore;
use std::{collections::BTreeMap, fs, sync::Arc, time::Duration};

#[tokio::test]
async fn managed_fixture_launch_uses_alias_and_persists_relative_entry_and_session() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("storage");
    let game_dir = root.join("Café Game");
    fs::create_dir_all(&game_dir).unwrap();
    crate::test_programs::copy_program(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        game_dir.join("game"),
    );
    let token = temp.path().join("token");
    let store = LibraryStore::in_memory().await.unwrap();
    caskade_installer::manifest::RootManifest::new("fixture-root")
        .unwrap()
        .write(&root)
        .unwrap();
    let game = Game::new("Fixture");
    store.upsert_game(&game).await.unwrap();
    store
        .upsert_root(&StorageRoot {
            id: "fixture-root".into(),
            path: root,
            is_default: true,
            label: None,
        })
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("caskade-storage"), "fixture");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::Managed {
        root_id: "fixture-root".into(),
        rel_dir: "Café Game".into(),
    });
    entry.alias_id = Some("fixture-alias".into());
    entry.launch = Some(LaunchSpec::Executable {
        path: "game".into(),
        args: vec!["--token".into(), token.to_str().unwrap().into()],
        cwd: Some(".".into()),
        env: BTreeMap::new(),
    });
    store.upsert_entry(&entry).await.unwrap();
    let aliases = AliasStore::new(temp.path().join("aliases"));
    let events = Arc::new(Events::default());
    let service = LaunchService::new(store.clone())
        .with_aliases(Some(aliases.clone()))
        .with_hooks(events)
        .with_config(fast_config());
    let session = service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(session.ended_at_ms.is_some());
    assert!(token.is_file());
    assert_eq!(
        aliases.target("fixture-alias").unwrap(),
        Some(fs::canonicalize(&game_dir).unwrap())
    );
    let saved = store.sessions_for_game(&game.id, 10).await.unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].duration_ms, session.duration_ms);
    assert_eq!(store.get_entry(&entry.id).await.unwrap().unwrap(), entry);
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
}

#[tokio::test]
async fn sqlite_open_session_survives_service_restart_and_preserves_uniqueness() {
    let temp = tempfile::tempdir().unwrap();
    let game_dir = temp.path().join("game");
    fs::create_dir(&game_dir).unwrap();
    crate::test_programs::copy_program("/bin/sleep", game_dir.join("game"));
    let db = temp.path().join("library.db");
    let store = LibraryStore::open(&db).await.unwrap();
    let game = Game::new("Sleep fixture");
    store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "sleep");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: game_dir.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: game_dir.join("game"),
        args: vec!["1.0".into()],
        cwd: Some(game_dir),
        env: BTreeMap::new(),
    });
    store.upsert_entry(&entry).await.unwrap();
    let first = LaunchService::new(store.clone())
        .with_config(fast_config())
        .with_aliases(None);
    let handle = first.launch(&entry.id).await.unwrap();
    let id = handle.ticket.session_id.clone();
    tokio::time::sleep(Duration::from_millis(100)).await;
    first.shutdown();
    assert!(matches!(handle.wait().await, Err(LaunchError::Cancelled)));
    store.close().await;
    drop(first);
    drop(store);
    let store = LibraryStore::open(&db).await.unwrap();
    let second = LaunchService::new(store.clone())
        .with_config(fast_config())
        .with_aliases(None);
    let mut recovered = second.recover().await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].ticket.session_id, id);
    assert!(matches!(
        second.launch(&entry.id).await,
        Err(LaunchError::AlreadyRunning)
    ));
    let completed = recovered.remove(0).wait().await.unwrap();
    assert!(completed.duration_ms >= 700, "{}", completed.duration_ms);
    assert_eq!(
        store.sessions_for_game(&game.id, 10).await.unwrap().len(),
        1
    );
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
}

struct CancelWine;
#[async_trait::async_trait]
impl LaunchHooks for CancelWine {
    async fn before(&self, _: &LaunchTicket, _: &mut LaunchCommand) -> Result<bool> {
        Ok(false)
    }
}
#[tokio::test]
async fn cancellation_precedes_managed_wine_initialization_and_session_reservation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let directory = root.join("game");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("game.exe"), b"MZ").unwrap();
    caskade_installer::manifest::RootManifest::new("root")
        .unwrap()
        .write(&root)
        .unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let game = Game::new("Cancelled Wine game");
    store.upsert_game(&game).await.unwrap();
    store
        .upsert_root(&StorageRoot {
            id: "root".into(),
            path: root,
            is_default: true,
            label: None,
        })
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "cancel-wine");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::Managed {
        root_id: "root".into(),
        rel_dir: "game".into(),
    });
    entry.runtime = caskade_discovery::GameRuntime::Windows {
        prefix: Some(caskade_discovery::Prefix {
            path: ".caskade/prefix".into(),
            arch: None,
            owner: None,
        }),
        runner: Some(caskade_discovery::RunnerHint {
            kind: caskade_discovery::RunnerKind::Wine,
            name: None,
            path: None,
        }),
    };
    entry.launch = Some(LaunchSpec::Executable {
        path: "game.exe".into(),
        args: vec![],
        cwd: Some(".".into()),
        env: BTreeMap::new(),
    });
    store.upsert_entry(&entry).await.unwrap();
    assert!(matches!(
        LaunchService::new(store.clone())
            .with_hooks(Arc::new(CancelWine))
            .launch(&entry.id)
            .await,
        Err(LaunchError::Cancelled)
    ));
    assert!(!directory.join(".caskade/prefix").exists());
    assert!(
        store
            .sessions_for_game(&game.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
}
