#![cfg(target_os = "linux")]
use super::super::*;
use crate::{
    launch::LaunchService,
    model::{Game, GameEntry, InstallLocation},
};
use caskade_discovery::SourceId;
use std::{
    path::Path,
    time::{Duration, Instant},
};

async fn finished(service: &InstallService, id: &str) -> JobSnapshot {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = service.job(id).unwrap();
        if job.status.is_finished() {
            return job;
        }
        assert!(Instant::now() < deadline, "{job:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn setup(root: &Path) -> (LibraryStore, InstallService, LaunchService) {
    std::fs::create_dir_all(root).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("remove-root").unwrap();
    identity.write(root).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let mut record = StorageRoot::from_manifest(root.into(), &identity);
    record.is_default = true;
    store.upsert_root(&record).await.unwrap();
    let launch = LaunchService::new(store.clone()).with_aliases(None);
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    (store, service, launch)
}

/// Installs a one-executable game into the root and returns its entry.
async fn install(service: &InstallService, store: &LibraryStore, dir: &Path) -> GameEntry {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir(dir).unwrap();
    let game = dir.join("game.x86_64");
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &game,
    )
    .unwrap();
    std::fs::set_permissions(&game, std::fs::Permissions::from_mode(0o755)).unwrap();
    let job = service
        .install(dir.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let done = finished(service, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    store
        .entries_for_game(done.game_id.as_deref().unwrap())
        .await
        .unwrap()
        .remove(0)
}

async fn game_with(store: &LibraryStore, source: SourceId) -> (Game, GameEntry) {
    let game = store.upsert_game(&Game::new("Added")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, source, crate::model::new_id());
    entry.location = Some(InstallLocation::External {
        path: "/games/added.rom".into(),
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    (game, entry)
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_an_install_uninstalls_it_and_keeps_hand_off_files() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Games");
    let (store, service, launch) = setup(&root).await;
    let entry = install(&service, &store, &t.path().join("Removed Game")).await;
    let Some(InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
        panic!("{entry:?}")
    };
    let rom = t.path().join("removed.rom");
    std::fs::write(&rom, b"rom").unwrap();
    let mut handoff = GameEntry::new(&entry.game_id, HANDOFF_SOURCE, "rom");
    handoff.location = Some(InstallLocation::External { path: rom.clone() });
    store.upsert_entry(&handoff).await.unwrap();

    let lease = launch.try_game_lease(&entry.game_id).unwrap();
    assert!(
        service
            .remove_game(&entry.game_id, launch.clone())
            .await
            .is_err()
    );
    drop(lease);
    let job = service
        .remove_game(&entry.game_id, launch)
        .await
        .unwrap()
        .expect("files to uninstall");
    let done = finished(&service, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert!(store.game(&entry.game_id).await.unwrap().is_none());
    assert!(
        store
            .entries_for_game(&entry.game_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!root.join(rel_dir).exists());
    assert!(rom.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_install_whose_folder_is_gone_and_a_hand_off_go_at_once() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Games");
    let (store, service, launch) = setup(&root).await;
    let entry = install(&service, &store, &t.path().join("Deleted Game")).await;
    let Some(InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
        panic!("{entry:?}")
    };
    std::fs::remove_dir_all(root.join(rel_dir)).unwrap();
    let removed = service
        .remove_game(&entry.game_id, launch.clone())
        .await
        .unwrap();
    assert!(removed.is_none());
    assert!(store.game(&entry.game_id).await.unwrap().is_none());

    let (handoff, _) = game_with(&store, HANDOFF_SOURCE).await;
    assert!(
        service
            .remove_game(&handoff.id, launch.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert!(store.game(&handoff.id).await.unwrap().is_none());

    // An install that was uninstalled leaves a game without entries.
    let empty = store.upsert_game(&Game::new("Uninstalled")).await.unwrap();
    assert!(
        service
            .remove_game(&empty.id, launch)
            .await
            .unwrap()
            .is_none()
    );
    assert!(store.game(&empty.id).await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn scanned_and_web_games_stay() {
    let t = tempfile::tempdir().unwrap();
    let (store, service, launch) = setup(&t.path().join("Games")).await;
    for source in [SourceId::new("steam"), crate::web::WEB_SOURCE] {
        let (game, mut entry) = game_with(&store, HANDOFF_SOURCE).await;
        entry.id = crate::model::new_id();
        entry.source_id = source;
        entry.external_id = "other".into();
        store.upsert_entry(&entry).await.unwrap();
        assert!(service.remove_game(&game.id, launch.clone()).await.is_err());
        assert_eq!(store.entries_for_game(&game.id).await.unwrap().len(), 2);
    }
    assert!(service.remove_game("missing", launch).await.is_err());
}
