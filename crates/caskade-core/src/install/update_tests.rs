use super::*;
use crate::launch::LaunchService;
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

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

/// A portable release of the fixture game: its executable and `files`.
fn release(dir: &Path, files: &[(&str, &[u8])]) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let exe = dir.join("fixture-runner.x86_64");
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &exe,
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (path, bytes) in files {
        std::fs::write(dir.join(path), bytes).unwrap();
    }
    dir.to_string_lossy().into_owned()
}

/// An update reinstalls the game in its folder under the same entry and
/// alias, keeps the user's files, and leaves nothing set aside. A release
/// that won't install puts the old version back.
#[tokio::test(flavor = "multi_thread")]
async fn updates_replace_the_game_in_place_and_keep_the_user_s_files() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-fixture").unwrap();
    identity.write(&root_path).unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let mut root =
        StorageRoot::from_manifest(std::fs::canonicalize(&root_path).unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let aliases = AliasStore::new(temp.path().join("aliases"));
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: Some(aliases.clone()),
        },
    );
    let launch = LaunchService::new(store.clone());

    let v1 = release(
        &temp.path().join("v1/Fixture Runner"),
        &[("data.pak", b"levels v1"), ("readme.txt", b"v1")],
    );
    let job = service.install(v1, None).await.unwrap();
    let job = finished(&service, &job.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    let game_id = job.game_id.unwrap();
    let entry = store.entries_for_game(&game_id).await.unwrap().remove(0);
    let Some(crate::model::InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
        panic!("{:?}", entry.location);
    };
    let game_dir = root.path.join(rel_dir);
    // The player's save, made by the game in its own folder.
    std::fs::create_dir_all(game_dir.join("saves")).unwrap();
    std::fs::write(game_dir.join("saves/slot1.sav"), b"progress").unwrap();

    let v2 = release(
        &temp.path().join("v2/Fixture Runner"),
        &[("data.pak", b"levels v2"), ("changelog.txt", b"new")],
    );
    let job = service
        .update_game(entry.id.clone(), v2, "v2".into(), launch.clone())
        .await
        .unwrap();
    assert_eq!(job.kind, JobKind::Update);
    let job = finished(&service, &job.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");

    let entries = store.entries_for_game(&game_id).await.unwrap();
    assert_eq!(entries.len(), 1);
    let updated = &entries[0];
    assert_eq!(
        (
            &updated.id,
            &updated.external_id,
            &updated.location,
            &updated.alias_id
        ),
        (
            &entry.id,
            &entry.external_id,
            &entry.location,
            &entry.alias_id
        )
    );
    assert_eq!(updated.state, caskade_discovery::InstallState::Installed);
    assert_eq!(
        std::fs::read(game_dir.join("data.pak")).unwrap(),
        b"levels v2"
    );
    assert!(game_dir.join("changelog.txt").is_file());
    assert!(!game_dir.join("readme.txt").exists());
    assert_eq!(
        std::fs::read(game_dir.join("saves/slot1.sav")).unwrap(),
        b"progress"
    );
    assert_eq!(
        std::fs::canonicalize(aliases.path(entry.alias_id.as_ref().unwrap()).unwrap()).unwrap(),
        std::fs::canonicalize(&game_dir).unwrap()
    );
    let aside = root.path.join(".caskade/updates");
    assert_eq!(std::fs::read_dir(&aside).unwrap().count(), 0);

    // Nothing in this "release" is a game: the update fails and v2 stays.
    let broken = temp.path().join("v3/Fixture Runner");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("notes.txt"), b"oops").unwrap();
    let job = service
        .update_game(
            entry.id.clone(),
            broken.to_string_lossy().into_owned(),
            "v3".into(),
            launch,
        )
        .await
        .unwrap();
    let job = finished(&service, &job.id).await;
    assert_ne!(job.status, JobStatus::Completed, "{job:?}");
    assert_eq!(
        std::fs::read(game_dir.join("data.pak")).unwrap(),
        b"levels v2"
    );
    assert_eq!(
        std::fs::read(game_dir.join("saves/slot1.sav")).unwrap(),
        b"progress"
    );
    assert_eq!(std::fs::read_dir(&aside).unwrap().count(), 0);
    let entry = store.get_entry(&entry.id).await.unwrap().unwrap();
    assert_eq!(entry.state, caskade_discovery::InstallState::Installed);
}

/// An update the app stopped in: put back when nothing replaced the game,
/// finished when the new version is installed.
#[tokio::test(flavor = "multi_thread")]
async fn interrupted_updates_are_undone_or_finished_at_startup() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-fixture").unwrap();
    identity.write(&root_path).unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let mut root =
        StorageRoot::from_manifest(std::fs::canonicalize(&root_path).unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let v1 = release(
        &temp.path().join("v1/Fixture Runner"),
        &[("data.pak", b"v1")],
    );
    let job = finished(&service, &service.install(v1, None).await.unwrap().id).await;
    let entry = store
        .entries_for_game(&job.game_id.unwrap())
        .await
        .unwrap()
        .remove(0);
    let Some(crate::model::InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
        panic!();
    };
    let game_dir = root.path.join(rel_dir);
    std::fs::write(game_dir.join("save.dat"), b"save").unwrap();
    let mut updating = entry.clone();
    updating.state = caskade_discovery::InstallState::Updating;
    store.upsert_entry(&updating).await.unwrap();

    // Stopped after setting the game aside: it goes back.
    let aside = root.path.join(".caskade/updates");
    std::fs::create_dir_all(&aside).unwrap();
    std::fs::rename(&game_dir, aside.join("job-1")).unwrap();
    service.recover_updates().await;
    assert_eq!(std::fs::read(game_dir.join("save.dat")).unwrap(), b"save");
    assert!(!aside.join("job-1").exists());
    let entry = store.get_entry(&entry.id).await.unwrap().unwrap();
    assert_eq!(entry.state, caskade_discovery::InstallState::Installed);

    // Stopped after the new version installed: the save comes across.
    copy_dir(&game_dir, &aside.join("job-2"));
    std::fs::remove_file(game_dir.join("save.dat")).unwrap();
    service.recover_updates().await;
    assert_eq!(std::fs::read(game_dir.join("save.dat")).unwrap(), b"save");
    assert!(!aside.join("job-2").exists());
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
