//! Milestone acceptance (#194): app data backed up in one profile, verified,
//! and restored into another, isolated, through the app's startup path
//! (`recovery::stage` then `open_library`, as `caskade --restore` and
//! Restore and restart do); a library that won't open restored from a
//! scheduled backup; and a scheduled backup to a drive that isn't there
//! failing without touching the last good one.

use caskade_core::{
    AppPaths,
    backup::{
        BackupError, BackupSettings, Backups, CancellationToken, Component, ExportOptions,
        Exported, NAMESPACE, ProgressFn, Schedule, restore,
    },
    db::{LibraryStore, PlaytimeImport, RemoteTotal},
    model::{
        CollectionKind, Game, GameEntry, InstallLocation, InstallState, LaunchSpec, PlaySession,
        SaveCollection,
    },
    settings::SettingsService,
};
use caskade_desktop::recovery;
use caskade_discovery::ExternalId;
use std::{path::Path, sync::Arc};

const START: i64 = 1_700_000_000_000;
const DAY: i64 = 24 * 60 * 60 * 1000;

struct Seeded {
    store: LibraryStore,
    account: String,
    games: Vec<String>,
}

/// 25 installed games, each with a tracked session and a store total, a
/// favourite and a note on some, a collection and a setting.
async fn fill(paths: &AppPaths) -> Seeded {
    let store = LibraryStore::open_default(paths).await.unwrap();
    let account = store.accounts().await.unwrap()[0].id.clone();
    let mut games = Vec::new();
    for index in 0..25u32 {
        let mut game = Game::new(format!("Kept {index:02}"));
        game.favorite = index % 3 == 0;
        game.notes = format!("note {index}");
        store.upsert_game(&game).await.unwrap();
        let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), index.to_string());
        entry.ids = vec![ExternalId::Steam {
            app_id: 1000 + index,
        }];
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External {
            path: format!("/games/kept-{index}").into(),
        });
        entry.launch = Some(LaunchSpec::Uri {
            uri: format!("steam://rungameid/{}", 1000 + index),
        });
        store.upsert_entry(&entry).await.unwrap();
        let mut session = PlaySession::new(&game.id, &entry.id, START + i64::from(index));
        session.duration_ms = 60_000 * u64::from(index + 1);
        session.ended_at_ms = Some(START + 100_000);
        session.heartbeat_at_ms = START + 100_000;
        store.upsert_session(&session).await.unwrap();
        store
            .record_remote_playtime(
                "steam",
                "",
                &[RemoteTotal {
                    entry_id: entry.id.clone(),
                    total_ms: Some(3_600_000),
                    last_played_ms: Some(START + 200_000),
                }],
                START + 300_000,
                PlaytimeImport::Always,
            )
            .await
            .unwrap();
        games.push(game.id);
    }
    let collection = store
        .save_collection(
            &account,
            &SaveCollection {
                id: None,
                name: "Couch".into(),
                description: "co-op".into(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    store
        .add_to_collection(&account, &collection.id, &games[..5])
        .await
        .unwrap();
    store
        .set_setting("app.acceptance", &serde_json::json!({"kept": true}))
        .await
        .unwrap();
    Seeded {
        store,
        account,
        games,
    }
}

fn quiet() -> ProgressFn {
    Arc::new(|_| {})
}

async fn export(store: &LibraryStore, paths: &AppPaths, components: &[Component]) -> Exported {
    let backups = Backups::new(store.clone(), paths.clone());
    let exported = backups
        .export(
            ExportOptions {
                destination: backups.default_destination(),
                components: components.iter().copied().collect(),
                set_id: None,
            },
            quiet(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    caskade_core::backup::verify(&exported.path, &CancellationToken::new(), &|_| {}).unwrap();
    exported
}

/// What a person sees of each game: title, favourite, note, play time,
/// last played and releases.
async fn shown(store: &LibraryStore, games: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for id in games {
        let details = store.get_game(id).await.unwrap().unwrap();
        out.push(format!(
            "{} {} {} {} {:?} {:?}",
            details.game.title,
            details.game.favorite,
            details.game.notes,
            details.playtime_ms,
            details.last_played_at_ms,
            details
                .entries
                .iter()
                .map(|e| (&e.id, &e.state, &e.launch))
                .collect::<Vec<_>>()
        ));
    }
    out
}

async fn collections(store: &LibraryStore, account: &str) -> Vec<(String, String, u64, u64)> {
    store
        .collection_summaries(account)
        .await
        .unwrap()
        .into_iter()
        .map(|c| (c.id, c.name, c.games, c.playtime_ms))
        .collect()
}

#[tokio::test]
async fn a_backup_restores_into_an_isolated_profile_through_startup_and_matches() {
    let temp = tempfile::tempdir().unwrap();
    let source = AppPaths::under(&temp.path().join("source"));
    source.create().unwrap();
    let seeded = fill(&source).await;
    let plugin = source.data.join("plugins/fixture.plugin");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.json"), b"{\"id\":\"fixture.plugin\"}").unwrap();
    let exported = export(&seeded.store, &source, &[Component::Plugins]).await;
    let before = shown(&seeded.store, &seeded.games).await;
    let before_collections = collections(&seeded.store, &seeded.account).await;
    // Each game: its session plus the store's hour beyond it.
    let first = seeded
        .store
        .get_game(&seeded.games[0])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.playtime_ms, 3_600_000);
    seeded.store.close().await;

    // Another computer's profile, its own library opened once already.
    let target = AppPaths::under(&temp.path().join("target"));
    target.create().unwrap();
    let fresh = recovery::open_library(&target).await.unwrap();
    fresh.upsert_game(&Game::new("Replaced")).await.unwrap();
    fresh.close().await;
    recovery::stage(&target, &exported.path).await.unwrap();
    // Staged only: nothing changes until the next start.
    assert!(restore::pending(&target).unwrap().is_some());
    let restored = recovery::open_library(&target).await.unwrap();
    assert_eq!(shown(&restored, &seeded.games).await, before);
    assert_eq!(
        collections(&restored, &seeded.account).await,
        before_collections
    );
    assert_eq!(
        restored.get_setting("app.acceptance").await.unwrap(),
        Some(serde_json::json!({"kept": true}))
    );
    assert!(
        target
            .data
            .join("plugins/fixture.plugin/plugin.json")
            .is_file()
    );
    let outcome = restore::take_outcome(&target).unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(restore::pending(&target).unwrap().is_none());

    // Every table holds what the original did: a backup of the restored
    // profile counts the same rows.
    let again = export(&restored, &target, &[]).await;
    assert_eq!(again.manifest.tables, exported.manifest.tables);
    assert!(
        again
            .manifest
            .tables
            .iter()
            .any(|t| t.name == "remote_playtime" && t.rows == 25)
    );
    restored.close().await;
}

#[tokio::test]
async fn a_library_that_wont_open_comes_back_from_a_scheduled_backup_on_a_drive() {
    let temp = tempfile::tempdir().unwrap();
    let source = AppPaths::under(&temp.path().join("source"));
    source.create().unwrap();
    let seeded = fill(&source).await;
    let drive = temp.path().join("usb");
    std::fs::create_dir(&drive).unwrap();
    let settings = SettingsService::new(seeded.store.clone());
    let backups = Backups::new(seeded.store.clone(), source.clone());
    backups.set_settings(settings.clone());
    settings
        .set(
            NAMESPACE,
            serde_json::to_value(BackupSettings {
                destination: drive.to_string_lossy().into_owned(),
                components: Default::default(),
                schedule: Schedule::Daily,
                keep: 1,
            })
            .unwrap(),
            None,
        )
        .await
        .unwrap();
    let first = backups.run_due(START).await.unwrap().unwrap();

    // The drive is unplugged when the next one is due.
    let away = temp.path().join("away");
    std::fs::rename(&drive, &away).unwrap();
    let missing = backups.run_due(START + DAY).await.unwrap().unwrap_err();
    assert!(
        matches!(missing, BackupError::DestinationMissing(_)),
        "{missing:?}"
    );
    let status = backups.status().await;
    assert!(status.last.unwrap().error.is_some());
    // Plugged in again: the failed one left nothing, and retention kept the
    // last good backup until the next verified.
    std::fs::rename(&away, &drive).unwrap();
    assert!(first.path.is_file());
    assert_eq!(names(&drive), [file_name(&first.path)]);
    // Retried with backoff, not at once.
    assert!(backups.run_due(START + DAY + 1).await.is_none());
    let retried = backups.run_due(START + 2 * DAY).await.unwrap().unwrap();
    assert_eq!(
        names(&drive),
        [file_name(&retried.path)],
        "keep 1 pruned the older"
    );
    seeded.store.close().await;

    // The library breaks; the backup on the drive brings it back at start.
    let broken = AppPaths::under(&temp.path().join("broken"));
    broken.create().unwrap();
    std::fs::write(broken.database(), b"not a database").unwrap();
    assert!(recovery::open_library(&broken).await.is_err());
    recovery::stage(&broken, &retried.path).await.unwrap();
    let restored = recovery::open_library(&broken).await.unwrap();
    for id in &seeded.games {
        assert!(restored.get_game(id).await.unwrap().is_some());
    }
    restored.close().await;
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
