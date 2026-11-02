//! Save versions on one device (#239): a version after each session, none
//! when nothing changed, restoring an older one keeps the one it replaces,
//! and pruning keeps the newest, every tip and every head.

use caskade_core::{
    db::LibraryStore,
    ludusavi::Ludusavi,
    model::{Game, GameEntry, InstallLocation, InstallState, SourceId},
    paths::AppPaths,
    saves::{
        LocationKind, LocationSpec, SavePreferences, Saves,
        versions::{SaveCheck, SaveVersions, VersionReason},
    },
    settings::SettingsService,
};
use caskade_discovery::HostEnv;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn write(path: impl AsRef<Path>, value: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value).unwrap();
}

struct Device {
    versions: SaveVersions,
    settings: SettingsService,
    entry: GameEntry,
    folder: PathBuf,
    _dir: tempfile::TempDir,
}

/// A game whose saves the user pointed at `~/Saves/Fixture`.
async fn device() -> Device {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("root/home/player")).unwrap();
    write(
        dir.path().join("machine.toml"),
        "os = 'linux'\n[env]\nHOME = '/home/player'\nUSER = 'player'\n",
    );
    let env = HostEnv::fixture(dir.path()).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    let game = store.upsert_game(&Game::new("Fixture")).await.unwrap();
    let base = env.home().join("games/fixture");
    fs::create_dir_all(&base).unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("manual"), "fixture");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External { path: base });
    let entry = store.upsert_entry(&entry).await.unwrap();
    let folder = env.home().join("Saves/Fixture");
    fs::create_dir_all(&folder).unwrap();
    store
        .set_save_preferences(
            &entry.id,
            &SavePreferences {
                manifest_title: None,
                locations: Some(vec![LocationSpec {
                    kind: LocationKind::Folder,
                    path: folder.to_string_lossy().into_owned(),
                }]),
            },
            0,
        )
        .await
        .unwrap();
    let saves = Saves::with_environment(store, Ludusavi::new(dir.path().join("cache")), env);
    let versions = SaveVersions::new(saves, dir.path().join("blobs"));
    versions.set_settings(settings.clone());
    versions.set_account(|| Some("player".into()));
    Device {
        versions,
        settings,
        entry,
        folder,
        _dir: dir,
    }
}

#[tokio::test]
async fn a_version_follows_each_changed_session_and_restoring_keeps_the_one_replaced() {
    let device = device().await;
    let (versions, entry) = (&device.versions, &device.entry.id);
    let slot = device.folder.join("slot1.sav");
    assert_eq!(
        versions.before_play(entry).await.unwrap().check,
        SaveCheck::NoSaves
    );

    write(&slot, "chapter one");
    let first = versions
        .capture(entry, Some("session-1"), VersionReason::Session)
        .await
        .unwrap()
        .expect("a first version");
    assert!(first.parents.is_empty());
    assert_eq!((first.file_count, first.size_bytes), (1, 11));
    assert_eq!(first.session_id.as_deref(), Some("session-1"));
    // Nothing changed: no version.
    assert!(
        versions
            .capture(entry, None, VersionReason::Session)
            .await
            .unwrap()
            .is_none()
    );

    write(&slot, "chapter two");
    let second = versions
        .capture(entry, None, VersionReason::Session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.parents, vec![first.id.clone()]);

    // Back to chapter one: the save in place is chapter one, and chapter
    // two is still a version.
    let restored = versions
        .use_version(entry, &first.id, VersionReason::Restored)
        .await
        .unwrap();
    assert_eq!(fs::read_to_string(&slot).unwrap(), "chapter one");
    assert_eq!(restored.content_hash, first.content_hash);
    assert!(restored.parents.contains(&second.id));
    let all = versions.versions(&device.entry.game_id).await.unwrap();
    assert_eq!(
        all.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
        [&restored.id, &second.id, &first.id]
    );
    // And playing starts from it.
    assert_eq!(
        versions.before_play(entry).await.unwrap().check,
        SaveCheck::UpToDate
    );

    // A save changed outside Caskade is kept before a launch.
    write(device.folder.join("nested/slot2.sav"), "outside");
    versions.before_play(entry).await.unwrap();
    let newest = versions.versions(&device.entry.game_id).await.unwrap();
    assert_eq!(newest[0].reason, VersionReason::Found);
    assert_eq!(newest[0].file_count, 2);
}

#[tokio::test]
async fn pruning_keeps_the_newest_and_drops_files_nothing_names() {
    let device = device().await;
    device
        .settings
        .set(
            "core.saves",
            json!({ "manifestUrl": "https://example.com/manifest.yaml", "keep": 3 }),
            None,
        )
        .await
        .unwrap();
    let (versions, entry) = (&device.versions, &device.entry.id);
    let mut made = Vec::new();
    for index in 0..6 {
        write(device.folder.join("slot.sav"), &format!("save {index}"));
        made.push(
            versions
                .capture(entry, None, VersionReason::Session)
                .await
                .unwrap()
                .unwrap(),
        );
    }
    let kept: Vec<String> = versions
        .versions(&device.entry.game_id)
        .await
        .unwrap()
        .into_iter()
        .map(|version| version.id)
        .collect();
    assert_eq!(
        kept,
        made.iter()
            .rev()
            .take(3)
            .map(|v| v.id.clone())
            .collect::<Vec<_>>()
    );
    // The oldest's file list is gone with it; the newest's is kept.
    assert!(!versions.blobs().has(&made[0].content_hash));
    assert!(versions.blobs().has(&made[5].content_hash));
    assert!(versions.complete(&made[5]));
}

#[tokio::test]
async fn the_end_of_a_session_keeps_a_version_and_a_session_that_never_started_doesnt() {
    use caskade_core::bus::Bus;
    use caskade_core::launch::{LaunchAfter, LaunchEvent, SessionStatus};

    let device = device().await;
    let bus = Bus::default();
    device.versions.attach(&bus);
    let event = |session: &str, status| LaunchEvent {
        command: None,
        game_id: device.entry.game_id.clone(),
        entry_id: device.entry.id.clone(),
        session_id: session.into(),
        started_at_ms: 1,
        ended_at_ms: Some(2),
        duration_ms: 1,
        status,
    };
    write(device.folder.join("slot1.sav"), "after a run");
    // A launch that failed to start changed nothing.
    bus.run::<LaunchAfter>(event("never", SessionStatus::NotStarted))
        .await;
    bus.run::<LaunchAfter>(event("played", SessionStatus::Exited))
        .await;
    // Kept in the background, so the session's end isn't held up.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let kept = loop {
        let kept = device
            .versions
            .versions(&device.entry.game_id)
            .await
            .unwrap();
        if !kept.is_empty() || std::time::Instant::now() > deadline {
            break kept;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].session_id.as_deref(), Some("played"));
    assert_eq!(kept[0].reason, VersionReason::Session);
    assert_eq!(kept[0].account_id, "player");
}
