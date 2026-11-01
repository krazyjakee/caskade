//! Migration and repository integration tests over private SQLite databases.
use super::*;
use crate::model::*;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

/// The admin every library starts with.
pub(super) async fn admin(store: &LibraryStore) -> String {
    store.accounts().await.unwrap()[0].id.clone()
}

async fn game_entry(store: &LibraryStore, title: &str) -> (Game, GameEntry) {
    let game = Game::new(title);
    store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), new_id());
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: PathBuf::from("/fixture/game"),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: PathBuf::from("/fixture/game/play"),
        args: vec!["--test".into(), "a;literal".into()],
        cwd: Some(PathBuf::from("/fixture/game")),
        env: BTreeMap::from([("FIXTURE".into(), "literal value".into())]),
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    (game, entry)
}

#[tokio::test]
async fn launch_variants_and_discovery_identity_round_trip_losslessly() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (game, mut entry) = game_entry(&store, "Launch variants").await;
    entry.ids = vec![
        ExternalId::Steam { app_id: 620 },
        ExternalId::Gog { product_id: 12345 },
    ];
    let specs = [
        LaunchSpec::Uri {
            uri: "steam://rungameid/620".into(),
        },
        LaunchSpec::Flatpak {
            app_id: "org.fixture.Game".into(),
            args: vec!["two words".into()],
        },
        LaunchSpec::Emulator {
            platform_id: "fixture".into(),
            target: crate::emulation::EmulatorTarget::Profile {
                emulator_id: "fixture-emulator".into(),
                profile_id: "fixture-profile".into(),
            },
            roms: vec![crate::emulation::GameRom::absolute("/fixture/rom.bin")],
            args: crate::emulation::ArgsOverride::Append {
                args: vec!["--literal".into()],
            },
        },
        LaunchSpec::Plugin {
            plugin_id: "fixture".into(),
            runner_id: "runner".into(),
            payload: serde_json::json!({"nested":[1,true,"literal"]}),
        },
    ];
    for spec in specs {
        entry.launch = Some(spec);
        store.upsert_entry(&entry).await.unwrap();
        assert_eq!(store.get_entry(&entry.id).await.unwrap().unwrap(), entry);
    }
    assert_eq!(
        store
            .get_game(&game.id)
            .await
            .unwrap()
            .unwrap()
            .entries
            .len(),
        1
    );
}

#[tokio::test]
async fn concurrent_reservations_and_compare_and_set_have_one_winner() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::open(&temp.path().join("race.sqlite"))
        .await
        .unwrap();
    let (game, entry) = game_entry(&store, "Race").await;
    let a = PlaySession::new(&game.id, &entry.id, 1000);
    let b = PlaySession::new(&game.id, &entry.id, 1001);
    let (a, b) = tokio::join!(store.start_session(&a), store.start_session(&b));
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let one = serde_json::json!(1);
    let two = serde_json::json!(2);
    let (a, b) = tokio::join!(
        store.compare_and_set_setting("race", None, &one),
        store.compare_and_set_setting("race", None, &two)
    );
    assert_ne!(a.unwrap(), b.unwrap());
    let old = store.get_setting("race").await.unwrap().unwrap();
    let (a, b) = tokio::join!(
        store.compare_and_set_setting("race", Some(&old), &serde_json::Value::Bool(true)),
        store.compare_and_set_setting("race", Some(&old), &serde_json::Value::Bool(false))
    );
    assert_ne!(a.unwrap(), b.unwrap());
}

#[tokio::test]
async fn managed_runtime_paths_are_relative_even_without_a_prefix() {
    use caskade_discovery::{Prefix, PrefixArch, RunnerHint, RunnerKind};
    let store = LibraryStore::in_memory().await.unwrap();
    let (_, mut entry) = game_entry(&store, "Managed runtime").await;
    store
        .upsert_root(&StorageRoot {
            id: "runtime-root".into(),
            path: PathBuf::from("/fixture/root"),
            is_default: false,
            label: None,
        })
        .await
        .unwrap();
    entry.location = Some(InstallLocation::Managed {
        root_id: "runtime-root".into(),
        rel_dir: PathBuf::from("Game"),
    });
    entry.launch = None;
    entry.runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: PathBuf::from(".caskade/prefix"),
            arch: Some(PrefixArch::Win64),
            owner: Some(SourceId::new("managed")),
        }),
        runner: Some(RunnerHint {
            kind: RunnerKind::Proton,
            name: Some("fixture-proton".into()),
            path: None,
        }),
    };
    assert_eq!(store.upsert_entry(&entry).await.unwrap(), entry);
    for prefix in [
        None,
        Some(Prefix {
            path: PathBuf::from("/absolute/prefix"),
            arch: None,
            owner: None,
        }),
    ] {
        let mut bad = entry.clone();
        bad.runtime = GameRuntime::Windows {
            prefix,
            runner: Some(RunnerHint {
                kind: RunnerKind::Wine,
                name: None,
                path: Some(PathBuf::from("/absolute/wine")),
            }),
        };
        assert!(store.upsert_entry(&bad).await.is_err());
    }
    let mut bad = entry.clone();
    bad.runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: PathBuf::from("/absolute/prefix"),
            arch: None,
            owner: None,
        }),
        runner: None,
    };
    assert!(store.upsert_entry(&bad).await.is_err());
    bad = entry.clone();
    bad.tracking = Some(TrackingHint {
        install_dir: Some(PathBuf::from("/absolute/tracking")),
        ..Default::default()
    });
    assert!(store.upsert_entry(&bad).await.is_err());
}

#[tokio::test]
async fn managed_location_updates_keep_source_identity_and_session_references() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (game, mut entry) = game_entry(&store, "Managed move").await;
    for id in ["old-root", "new-root"] {
        store
            .upsert_root(&StorageRoot {
                id: id.into(),
                path: PathBuf::from(format!("/fixture/{id}")),
                is_default: false,
                label: None,
            })
            .await
            .unwrap();
    }
    entry.location = Some(InstallLocation::Managed {
        root_id: "old-root".into(),
        rel_dir: "Game".into(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: "play.exe".into(),
        args: vec![],
        cwd: Some(".".into()),
        env: BTreeMap::new(),
    });
    store.upsert_entry(&entry).await.unwrap();
    let mut session = PlaySession::new(&game.id, &entry.id, 1000);
    session.ended_at_ms = Some(2000);
    session.duration_ms = 1000;
    store.upsert_session(&session).await.unwrap();
    let moved = store
        .update_managed_location(&entry.id, "new-root", std::path::Path::new("Renamed\\Game"))
        .await
        .unwrap();
    let mut expected = entry.clone();
    expected.location = Some(InstallLocation::Managed {
        root_id: "new-root".into(),
        rel_dir: "Renamed/Game".into(),
    });
    assert_eq!(moved, expected);
    assert!(store.entries_for_root("old-root").await.unwrap().is_empty());
    assert_eq!(
        store.entries_for_root("new-root").await.unwrap().as_slice(),
        std::slice::from_ref(&expected)
    );
    assert_eq!(store.get_session(&session.id).await.unwrap(), Some(session));
    for (root, relative) in [
        ("missing", "Game"),
        ("old-root", "/absolute"),
        ("old-root", "../escape"),
    ] {
        assert!(
            store
                .update_managed_location(&entry.id, root, std::path::Path::new(relative))
                .await
                .is_err()
        );
        assert_eq!(
            store.get_entry(&entry.id).await.unwrap(),
            Some(expected.clone())
        );
    }
    let (_, external) = game_entry(&store, "External").await;
    assert!(
        store
            .update_managed_location(&external.id, "new-root", std::path::Path::new("Game"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn migrations_up_down_and_up_preserve_schema_contract() {
    let store = LibraryStore::in_memory().await.unwrap();
    let tables:i64=sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('games','game_entries','storage_roots','play_sessions','artwork','collections','collection_members','settings','game_search')").fetch_one(&store.pool).await.unwrap();
    assert_eq!(tables, 9);
    MIGRATOR.undo(&store.pool, 0).await.unwrap();
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='games'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(tables, 0);
    MIGRATOR.run(&store.pool).await.unwrap();
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(enabled, 1);
}

#[tokio::test]
async fn file_database_reopens_and_every_game_field_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::open_default(&paths).await.unwrap();
    let mut game = Game::new("Café & Space");
    game.sort_title = "Space, Café".into();
    game.description = "Description\nwith lines".into();
    game.aliases = vec!["Old Name".into()];
    game.genres = vec!["Adventure".into()];
    game.tags = vec!["co-op".into()];
    game.release_date = Some("2024-02-29".into());
    game.platforms = vec!["pc".into()];
    game.favorite = true;
    game.rating = Some(89);
    game.notes = "User notes".into();
    assert_eq!(store.upsert_game(&game).await.unwrap(), game);
    store.close().await;
    let store = LibraryStore::open_default(&paths).await.unwrap();
    assert_eq!(store.game(&game.id).await.unwrap(), Some(game.clone()));
    assert_eq!(store.get_game(&game.id).await.unwrap().unwrap().game, game);
    assert!(store.delete_game(&game.id).await.unwrap());
    assert!(!store.delete_game(&game.id).await.unwrap());
}

#[tokio::test]
async fn managed_install_uses_manifest_identity_and_never_stores_absolute_game_paths() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (game, mut entry) = game_entry(&store, "Managed").await;
    let manifest = caskade_installer::manifest::RootManifest::new("fixture-root-id").unwrap();
    let mut root = StorageRoot::from_manifest(PathBuf::from("/volumes/old-root"), &manifest);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    entry.location = Some(InstallLocation::Managed {
        root_id: manifest.root_id().into(),
        rel_dir: PathBuf::from("Game\\Sub"),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: PathBuf::from("bin\\play.exe"),
        args: vec![],
        cwd: Some(PathBuf::from(".")),
        env: BTreeMap::new(),
    });
    entry.alias_id = Some("fixture-alias".into());
    entry.tracking = Some(TrackingHint {
        install_dir: Some(PathBuf::from(".")),
        ..Default::default()
    });
    let saved = store.upsert_entry(&entry).await.unwrap();
    let json: String = sqlx::query_scalar("SELECT record_json FROM game_entries WHERE id=?")
        .bind(&entry.id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert!(!json.contains("/volumes/"));
    assert!(!json.contains("/fixture/"));
    assert!(json.contains("bin/play.exe"));
    assert_eq!(
        saved.location,
        Some(InstallLocation::Managed {
            root_id: root.id.clone(),
            rel_dir: PathBuf::from("Game/Sub")
        })
    );
    root.path = PathBuf::from("/volumes/new-root");
    store.upsert_root(&root).await.unwrap();
    assert_eq!(store.get_entry(&entry.id).await.unwrap(), Some(saved));
    assert_eq!(
        store.get_root(&root.id).await.unwrap().unwrap().path,
        root.path
    );
    assert!(store.delete_root(&root.id).await.is_err());
    for unsafe_path in [
        "/absolute",
        "C:/absolute",
        r"C:\absolute",
        r"\\server\share",
        r"\\?\C:\absolute",
        "../escape",
        "nested/../escape",
    ] {
        let mut bad = entry.clone();
        bad.location = Some(InstallLocation::Managed {
            root_id: root.id.clone(),
            rel_dir: PathBuf::from(unsafe_path),
        });
        assert!(store.upsert_entry(&bad).await.is_err(), "{unsafe_path}");
        let mut bad = entry.clone();
        bad.launch = Some(LaunchSpec::Executable {
            path: PathBuf::from(unsafe_path),
            args: vec![],
            cwd: None,
            env: BTreeMap::new(),
        });
        assert!(
            store.upsert_entry(&bad).await.is_err(),
            "launch {unsafe_path}"
        );
    }
    store.delete_game(&game.id).await.unwrap();
    assert!(store.delete_root(&root.id).await.unwrap());
}

#[tokio::test]
async fn roots_enforce_one_default_and_entries_keep_source_identity() {
    let store = LibraryStore::in_memory().await.unwrap();
    for n in 1..=2 {
        store
            .upsert_root(&StorageRoot {
                id: format!("root-{n}"),
                path: PathBuf::from(format!("/root-{n}")),
                is_default: true,
                label: Some(format!("Disk {n}")),
            })
            .await
            .unwrap();
    }
    let roots = store.roots().await.unwrap();
    assert_eq!(roots.iter().filter(|r| r.is_default).count(), 1);
    assert_eq!(roots[0].id, "root-2");
    let (g, entry) = game_entry(&store, "First").await;
    let mut same = entry.clone();
    same.id = new_id();
    same.size = Some(1234);
    let updated = store.upsert_entry(&same).await.unwrap();
    assert_eq!(updated.id, entry.id);
    assert_eq!(updated.size, Some(1234));
    assert_eq!(store.entries_for_game(&g.id).await.unwrap().len(), 1);
    let (other, _) = game_entry(&store, "Second").await;
    same.game_id = other.id;
    assert!(store.upsert_entry(&same).await.is_err());
    assert_eq!(
        store
            .entry_by_source(entry.source_id.as_str(), &entry.external_id)
            .await
            .unwrap()
            .unwrap()
            .game_id,
        g.id
    );
    assert!(store.delete_entry(&entry.id).await.unwrap());
}

#[tokio::test]
async fn sessions_reserve_activate_heartbeat_finish_reopen_and_reject_wrong_entry_game() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (g, entry) = game_entry(&store, "Played").await;
    let mut session = PlaySession::new(&g.id, &entry.id, 1000);
    session.tracking = Some(serde_json::json!({"phase":"pending"}));
    assert_eq!(store.start_session(&session).await.unwrap(), session);
    assert!(
        store
            .start_session(&PlaySession::new(&g.id, &entry.id, 1001))
            .await
            .is_err()
    );
    let active = store
        .activate_session(
            &session.id,
            2000,
            serde_json::json!({"processes":[{"pid":42,"startedAt":123}]}),
        )
        .await
        .unwrap();
    assert_eq!(active.started_at_ms, 2000);
    assert_eq!(active.duration_ms, 0);
    store
        .heartbeat_session(
            &session.id,
            5000,
            3000,
            Some(serde_json::json!({"pid":42,"startedAt":123})),
        )
        .await
        .unwrap();
    let heartbeat = store
        .heartbeat_session(&session.id, 5100, 2000, None)
        .await
        .unwrap();
    assert_eq!(heartbeat.duration_ms, 3000);
    assert_eq!(
        heartbeat.tracking,
        Some(serde_json::json!({"pid":42,"startedAt":123}))
    );
    assert!(
        store
            .activate_session(&session.id, 6000, serde_json::Value::Null)
            .await
            .is_err()
    );
    assert_eq!(store.in_flight_sessions().await.unwrap().len(), 1);
    let finished = store.finish_session(&session.id, 7000, 5000).await.unwrap();
    assert_eq!(finished.duration_ms, 5000);
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
    assert_eq!(
        store
            .finish_session(&session.id, 99_000, 99_000)
            .await
            .unwrap(),
        finished
    );
    let list = store.sessions_for_game(&g.id, 10).await.unwrap();
    assert_eq!(list.as_slice(), std::slice::from_ref(&finished));
    assert_eq!(store.recent_sessions(10).await.unwrap(), list);
    let mut imported = finished.clone();
    imported.id = new_id();
    imported.started_at_ms = 1;
    assert_eq!(store.upsert_session(&imported).await.unwrap(), imported);
    assert!(store.delete_session(&imported.id).await.unwrap());
    let (other, _) = game_entry(&store, "Other").await;
    assert!(
        store
            .start_session(&PlaySession::new(&other.id, &entry.id, 8000))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn summaries_carry_the_cover_and_the_icon() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (g, _) = game_entry(&store, "Icons").await;
    let page = store.list_games(&ListGames::default()).await.unwrap();
    assert_eq!(
        (page.items[0].cover.as_ref(), page.items[0].icon.as_ref()),
        (None, None)
    );
    let mut icon = Artwork::new(
        &g.id,
        ArtworkKind::Icon,
        "b".repeat(64),
        "installer",
        "originals/bb.png",
        64,
        64,
    );
    icon.thumbnails.push(ArtworkThumbnail {
        size: 96,
        path: "thumbs/bb-96.png".into(),
        width: 64,
        height: 64,
    });
    store.upsert_artwork(&icon).await.unwrap();
    let cover = Artwork::new(
        &g.id,
        ArtworkKind::Cover,
        "c".repeat(64),
        "fixture",
        "originals/cc.png",
        600,
        900,
    );
    store.upsert_artwork(&cover).await.unwrap();
    let page = store.list_games(&ListGames::default()).await.unwrap();
    assert_eq!(page.items[0].icon.as_ref(), Some(&icon));
    assert_eq!(page.items[0].cover.as_ref(), Some(&cover));
}

#[tokio::test]
async fn artwork_collections_and_settings_round_trip_without_absolute_cache_paths() {
    let store = LibraryStore::in_memory().await.unwrap();
    let account = admin(&store).await;
    let (g, _) = game_entry(&store, "Artwork").await;
    let mut artwork = Artwork::new(
        &g.id,
        ArtworkKind::Cover,
        "a".repeat(64),
        "fixture",
        "originals/aa.png",
        800,
        1200,
    );
    artwork.thumbnails.push(ArtworkThumbnail {
        size: 320,
        path: "thumbs/aa-320.png".into(),
        width: 213,
        height: 320,
    });
    for kind in [
        ArtworkKind::Cover,
        ArtworkKind::Hero,
        ArtworkKind::Logo,
        ArtworkKind::Icon,
        ArtworkKind::Screenshot,
    ] {
        let mut a = artwork.clone();
        a.id = new_id();
        a.kind = kind;
        store.upsert_artwork(&a).await.unwrap();
        assert!(store.artwork_for_game(&g.id).await.unwrap().contains(&a));
        assert!(store.delete_artwork(&a.id).await.unwrap());
    }
    let mut bad = artwork.clone();
    bad.cached_path = "/outside/cache.png".into();
    assert!(store.upsert_artwork(&bad).await.is_err());
    bad = artwork.clone();
    bad.thumbnails[0].path = "../escape.png".into();
    assert!(store.upsert_artwork(&bad).await.is_err());
    let mut manual = Collection::manual("Shelf");
    manual.game_ids = vec![g.id.clone()];
    assert_eq!(
        store.upsert_collection(&account, &manual).await.unwrap(),
        manual
    );
    let smart = Collection {
        name: "Favorites".into(),
        description: "Saved filter".into(),
        pinned: true,
        kind: CollectionKind::Smart {
            query: ListGames {
                filter: GameFilter::Favorites,
                ..Default::default()
            },
        },
        ..Collection::manual("")
    };
    assert_eq!(
        store.upsert_collection(&account, &smart).await.unwrap(),
        smart
    );
    assert_eq!(store.collections(&account).await.unwrap().len(), 2);
    assert_eq!(
        store
            .collection_games(&account, &manual.id, 0, 10)
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        store
            .collection_games(&account, &smart.id, 0, 10)
            .await
            .unwrap()
            .total,
        0
    );
    assert!(store.delete_collection(&account, &manual.id).await.unwrap());
    assert!(store.delete_collection(&account, &smart.id).await.unwrap());
    let value = serde_json::json!({"version":1,"value":{"enabled":true}});
    store.set_setting("app.ui", &value).await.unwrap();
    assert_eq!(
        store.get_setting("app.ui").await.unwrap(),
        Some(value.clone())
    );
    assert!(
        !store
            .compare_and_set_setting("app.ui", None, &serde_json::Value::Null)
            .await
            .unwrap()
    );
    assert!(
        !store
            .compare_and_set_setting("app.ui", Some(&serde_json::Value::Null), &value)
            .await
            .unwrap()
    );
    assert!(
        store
            .compare_and_set_setting("app.ui", Some(&value), &serde_json::Value::Null)
            .await
            .unwrap()
    );
    assert_eq!(
        store.get_setting("app.ui").await.unwrap(),
        Some(serde_json::Value::Null)
    );
    store.set_setting("app._literal", &value).await.unwrap();
    store.set_setting("app.Xliteral", &value).await.unwrap();
    assert_eq!(store.settings_with_prefix("app._").await.unwrap().len(), 1);
    assert!(store.delete_setting("app.ui").await.unwrap());
}

#[tokio::test]
async fn merge_transfers_sources_sessions_artwork_and_memberships_atomically() {
    let store = LibraryStore::in_memory().await.unwrap();
    let account = admin(&store).await;
    let (mut keep, ke) = game_entry(&store, "Canonical").await;
    let (mut donor, de) = game_entry(&store, "Alias title").await;
    keep.notes = "First notes".into();
    donor.notes = "Other notes".into();
    donor.tags = vec!["co-op".into()];
    donor.favorite = true;
    store.upsert_game(&keep).await.unwrap();
    store.upsert_game(&donor).await.unwrap();
    let session = PlaySession::new(&donor.id, &de.id, 1000);
    store.start_session(&session).await.unwrap();
    store.finish_session(&session.id, 5000, 4000).await.unwrap();
    let artwork = Artwork::new(
        &donor.id,
        ArtworkKind::Cover,
        "b".repeat(64),
        "fixture",
        "bb/cover.png",
        100,
        200,
    );
    store.upsert_artwork(&artwork).await.unwrap();
    let mut collection = Collection::manual("Both");
    collection.game_ids = vec![keep.id.clone(), donor.id.clone()];
    store
        .upsert_collection(&account, &collection)
        .await
        .unwrap();
    let merged = store.merge_games(&keep.id, &donor.id).await.unwrap();
    assert!(merged.favorite);
    assert!(merged.aliases.contains(&donor.title));
    assert!(merged.tags.contains(&"co-op".into()));
    assert!(merged.notes.contains("Other notes"));
    assert!(store.game(&donor.id).await.unwrap().is_none());
    let entries = store.entries_for_game(&keep.id).await.unwrap();
    assert_eq!(entries.len(), 2);
    assert!(
        entries.iter().any(|e| e.id == ke.id
            && e.source_id == ke.source_id
            && e.external_id == ke.external_id)
    );
    assert!(
        entries.iter().any(|e| e.id == de.id
            && e.source_id == de.source_id
            && e.external_id == de.external_id)
    );
    let details = store.get_game(&keep.id).await.unwrap().unwrap();
    assert_eq!(details.playtime_ms, 4000);
    assert_eq!(details.artwork[0].game_id, keep.id);
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .unwrap()
            .game_id,
        keep.id
    );
    assert_eq!(
        store
            .get_collection(&account, &collection.id)
            .await
            .unwrap()
            .unwrap()
            .game_ids,
        [keep.id.clone()]
    );
    assert_eq!(
        store
            .list_games(&ListGames {
                query: Some("Alias".into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .items[0]
            .id,
        keep.id
    );
    let (other, oe) = game_entry(&store, "Other").await;
    let a = PlaySession::new(&keep.id, &ke.id, 9000);
    let b = PlaySession::new(&other.id, &oe.id, 9000);
    store.start_session(&a).await.unwrap();
    store.start_session(&b).await.unwrap();
    assert!(store.merge_games(&keep.id, &other.id).await.is_err());
    assert!(store.game(&other.id).await.unwrap().is_some());
    assert_eq!(
        store.get_entry(&oe.id).await.unwrap().unwrap().game_id,
        other.id
    );
}

#[tokio::test]
async fn search_triggers_cover_title_sort_aliases_tags_updates_deletes_and_literal_queries() {
    let store = LibraryStore::in_memory().await.unwrap();
    let mut game = Game::new("Café Adventure");
    game.sort_title = "Sort synonym".into();
    game.aliases = vec!["Old nickname".into()];
    game.tags = vec!["cooperation".into()];
    store.upsert_game(&game).await.unwrap();
    for query in ["cafe", "adven", "synonym", "nickname", "cooper"] {
        let page = store
            .list_games(&ListGames {
                query: Some(query.into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.total, 1, "{query}");
    }
    game.title = "New title".into();
    game.aliases.clear();
    game.tags.clear();
    store.upsert_game(&game).await.unwrap();
    for query in ["adven", "nickname", "cooper"] {
        assert_eq!(
            store
                .list_games(&ListGames {
                    query: Some(query.into()),
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            0,
            "{query}"
        );
    }
    sqlx::query(
        "UPDATE games SET title=?,record_json=json_set(record_json,'$.title',?) WHERE id=?",
    )
    .bind("Direct trigger")
    .bind("Direct trigger")
    .bind(&game.id)
    .execute(&store.pool)
    .await
    .unwrap();
    assert_eq!(
        store
            .list_games(&ListGames {
                query: Some("trigger".into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .total,
        1
    );
    for hostile in [
        "\" OR *",
        "\"; DROP TABLE games; --",
        "* NOT title:whatever",
    ] {
        assert_eq!(
            store
                .list_games(&ListGames {
                    query: Some(hostile.into()),
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            0
        );
    }
    let literal = Game::new("100% Genuine_Thing");
    store.upsert_game(&literal).await.unwrap();
    for query in ["%", "_"] {
        assert_eq!(
            store
                .list_games(&ListGames {
                    query: Some(query.into()),
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            1
        );
    }
    store.delete_game(&game.id).await.unwrap();
    assert_eq!(
        store
            .list_games(&ListGames {
                query: Some("trigger".into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .total,
        0
    );
    sqlx::query("INSERT INTO game_search(game_search,rank) VALUES('integrity-check',1)")
        .execute(&store.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn list_filters_stats_sort_and_pagination_use_persisted_state() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (a, ae) = game_entry(&store, "Alpha").await;
    let (mut b, mut be) = game_entry(&store, "Beta").await;
    b.favorite = true;
    store.upsert_game(&b).await.unwrap();
    be.launch = Some(LaunchSpec::Emulator {
        platform_id: "fixture-platform".into(),
        target: crate::emulation::EmulatorTarget::AskEmulator,
        roms: vec![crate::emulation::GameRom::absolute(
            "/fixture/game/game.rom",
        )],
        args: crate::emulation::ArgsOverride::Default,
    });
    store.upsert_entry(&be).await.unwrap();
    let mut hidden = Game::new("Hidden");
    hidden.hidden = true;
    store.upsert_game(&hidden).await.unwrap();
    let mut owned = Game::new("Owned");
    owned.sort_title = "00 Owned".into();
    store.upsert_game(&owned).await.unwrap();
    for (entry, start, duration) in [(&ae, 1000, 4000), (&be, 6000, 2000)] {
        let record = PlaySession::new(&entry.game_id, &entry.id, start);
        store.start_session(&record).await.unwrap();
        store
            .finish_session(&record.id, start + duration, duration as u64)
            .await
            .unwrap();
    }
    for (filter, count) in [
        (GameFilter::All, 3),
        (GameFilter::Installed, 2),
        (GameFilter::Favorites, 1),
        (GameFilter::Recent, 2),
        (GameFilter::Emulated, 1),
    ] {
        assert_eq!(
            store
                .list_games(&ListGames {
                    filter,
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            count
        );
    }
    assert_eq!(
        store
            .list_games(&ListGames {
                sort: GameSort::Recent,
                ..Default::default()
            })
            .await
            .unwrap()
            .items[0]
            .id,
        b.id
    );
    assert_eq!(
        store
            .list_games(&ListGames {
                sort: GameSort::Playtime,
                ..Default::default()
            })
            .await
            .unwrap()
            .items[0]
            .id,
        a.id
    );
    let page = store
        .list_games(&ListGames {
            offset: 1,
            limit: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, a.id);
    assert!(
        store
            .list_games(&ListGames {
                limit: 1001,
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert_eq!(
        store.library_stats().await.unwrap(),
        LibraryStats {
            total: 3,
            installed: 2,
            favorites: 1,
            recent: 2,
            emulated: 1,
            web: 0,
            owned: 0,
            playtime_ms: 6000
        }
    );
}

#[tokio::test]
async fn installed_entries_without_a_folder_count_as_installed() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (game, mut entry) = game_entry(&store, "Launcher only").await;
    entry.location = None;
    store.upsert_entry(&entry).await.unwrap();
    let page = store
        .list_games(&ListGames {
            filter: GameFilter::Installed,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, game.id);
    assert!(page.items[0].installed);
    assert_eq!(store.library_stats().await.unwrap().installed, 1);
}

#[tokio::test]
async fn web_games_show_in_the_web_tab_only() {
    let store = LibraryStore::in_memory().await.unwrap();
    let account = admin(&store).await;
    let web_entry = |game: &Game, url: &str| {
        let mut entry = GameEntry::new(&game.id, SourceId::new("web"), url);
        entry.state = InstallState::Installed;
        entry.launch = Some(LaunchSpec::Web { url: url.into() });
        entry
    };
    game_entry(&store, "Native").await;
    let mut web = Game::new("Web only");
    web.platforms = vec!["browser".into()];
    store.upsert_game(&web).await.unwrap();
    store
        .upsert_entry(&web_entry(&web, "https://a.example/"))
        .await
        .unwrap();
    // A game with a release of another kind shows in both.
    let (both, _) = game_entry(&store, "Both").await;
    store
        .upsert_entry(&web_entry(&both, "https://b.example/"))
        .await
        .unwrap();
    store.set_game_favorite(&web.id, true).await.unwrap();

    let titles = |filter| {
        let store = &store;
        async move {
            let page = store
                .list_games(&ListGames {
                    filter,
                    ..Default::default()
                })
                .await
                .unwrap();
            assert_eq!(page.total as usize, page.items.len());
            page.items
                .into_iter()
                .map(|game| game.title)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(titles(GameFilter::All).await, ["Both", "Native"]);
    assert_eq!(titles(GameFilter::Installed).await, ["Both", "Native"]);
    assert!(titles(GameFilter::Favorites).await.is_empty());
    assert_eq!(titles(GameFilter::Web).await, ["Both", "Web only"]);
    let stats = store.library_stats().await.unwrap();
    assert_eq!(
        (stats.total, stats.installed, stats.favorites, stats.web),
        (2, 2, 0, 2)
    );
    assert!(
        store
            .library_platforms(GameFilter::All)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .library_platforms(GameFilter::Web)
            .await
            .unwrap()
            .len(),
        1
    );

    // A manual collection keeps what was put in it, and Settings lists a
    // hidden web game.
    let shelf = store
        .save_collection(
            &account,
            &SaveCollection {
                id: None,
                name: "Shelf".into(),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    store
        .add_to_collection(&account, &shelf.id, &[web.id.clone()])
        .await
        .unwrap();
    assert_eq!(
        store
            .collection_games(&account, &shelf.id, 0, 10)
            .await
            .unwrap()
            .total,
        1
    );
    store.set_game_hidden(&web.id, true).await.unwrap();
    assert_eq!(store.hidden_games(0, 10).await.unwrap().total, 1);
}

#[tokio::test]
async fn sorts_by_added_and_release_date() {
    let store = LibraryStore::in_memory().await.unwrap();
    let mut old = Game::new("Old");
    old.release_date = Some("1998-11-21".into());
    store.upsert_game(&old).await.unwrap();
    let mut undated = Game::new("Undated");
    undated.release_date = None;
    store.upsert_game(&undated).await.unwrap();
    let mut new = Game::new("New");
    new.release_date = Some("2024-02-29".into());
    store.upsert_game(&new).await.unwrap();
    let entry = |game: &Game| GameEntry::new(&game.id, SourceId::new("fixture"), new_id());
    for game in [&undated, &new] {
        store.upsert_entry(&entry(game)).await.unwrap();
    }
    // A new release of the oldest game makes it the last added.
    store.upsert_entry(&entry(&old)).await.unwrap();
    // The newest game, uninstalled: nothing of it is left to have been added.
    let mut gone = Game::new("Uninstalled");
    gone.release_date = None;
    store.upsert_game(&gone).await.unwrap();
    let release = store.upsert_entry(&entry(&gone)).await.unwrap();
    store.delete_entry(&release.id).await.unwrap();
    let titles = |sort| {
        let store = &store;
        async move {
            store
                .list_games(&ListGames {
                    sort,
                    ..Default::default()
                })
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|game| game.title)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        titles(GameSort::Added).await,
        ["Old", "New", "Undated", "Uninstalled"]
    );
    assert_eq!(
        titles(GameSort::Released).await,
        ["New", "Old", "Undated", "Uninstalled"]
    );
    let page = store
        .list_games(&ListGames {
            sort: GameSort::Released,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items[0].release_date.as_deref(), Some("2024-02-29"));
}

#[tokio::test]
async fn thousand_game_search_stays_below_fifty_ms() {
    let store = LibraryStore::in_memory().await.unwrap();
    let games: Vec<_> = (0..1203)
        .map(|i| {
            let mut g = Game::new(format!("Fixture Game {i:04}"));
            g.aliases = vec![format!("Nickname {i:04}")];
            g.tags = vec![format!("genre{}", i % 7)];
            g
        })
        .collect();
    store.upsert_games(&games).await.unwrap();
    let mut timings = Vec::new();
    for query in ["1037", "Nickname 1037", "Fixture", "genre3", "001"]
        .into_iter()
        .cycle()
        .take(25)
    {
        let start = Instant::now();
        let result = store
            .list_games(&ListGames {
                query: Some(query.into()),
                ..Default::default()
            })
            .await
            .unwrap();
        timings.push(start.elapsed());
        assert!(result.total > 0);
    }
    timings.sort();
    let p95 = timings[timings.len() * 95 / 100];
    eprintln!(
        "1203-game FTS5 search median={:?} p95={p95:?} max={:?}",
        timings[timings.len() / 2],
        timings.last().unwrap()
    );
    assert!(p95 < Duration::from_millis(50), "p95={p95:?}");
}

#[tokio::test]
async fn rejected_writes_roll_back_bulk_import_and_manual_collection_update() {
    let store = LibraryStore::in_memory().await.unwrap();
    let account = admin(&store).await;
    let first = Game::new("Valid");
    let mut invalid = Game::new("Bad");
    invalid.rating = Some(101);
    assert!(store.upsert_games(&[first.clone(), invalid]).await.is_err());
    assert!(store.game(&first.id).await.unwrap().is_none());
    let (g, _) = game_entry(&store, "Existing").await;
    let mut collection = Collection::manual("Stable");
    collection.game_ids = vec![g.id.clone()];
    store
        .upsert_collection(&account, &collection)
        .await
        .unwrap();
    let mut bad = collection.clone();
    bad.game_ids.push("missing-game".into());
    assert!(store.upsert_collection(&account, &bad).await.is_err());
    assert_eq!(
        store
            .get_collection(&account, &collection.id)
            .await
            .unwrap(),
        Some(collection)
    );
}

#[tokio::test]
async fn collections_are_saved_filled_pinned_and_summarized() {
    let store = LibraryStore::in_memory().await.unwrap();
    let account = admin(&store).await;
    let (alpha, _) = game_entry(&store, "Alpha").await;
    let (beta, _) = game_entry(&store, "Beta").await;
    let gamma = Game::new("Gamma");
    store.upsert_game(&gamma).await.unwrap();
    let mut session = PlaySession::new(
        &beta.id,
        &store.entries_for_game(&beta.id).await.unwrap()[0].id,
        1000,
    );
    session.ended_at_ms = Some(4000);
    session.duration_ms = 3000;
    session.heartbeat_at_ms = 4000;
    store.upsert_session(&session).await.unwrap();

    let shelf = store
        .save_collection(
            &account,
            &SaveCollection {
                id: None,
                name: "Shelf".into(),
                description: "By hand".into(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    assert!(shelf.updated_at_ms > 0);
    let ids = |games: &[&Game]| games.iter().map(|g| g.id.clone()).collect::<Vec<_>>();
    store
        .add_to_collection(&account, &shelf.id, &ids(&[&gamma, &alpha]))
        .await
        .unwrap();
    // Already there: keeps its place.
    store
        .add_to_collection(&account, &shelf.id, &ids(&[&alpha, &beta]))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_collection(&account, &shelf.id)
            .await
            .unwrap()
            .unwrap()
            .game_ids,
        ids(&[&gamma, &alpha, &beta])
    );
    store
        .remove_from_collection(&account, &shelf.id, &ids(&[&alpha]))
        .await
        .unwrap();
    assert_eq!(
        store.game_collection_ids(&account, &beta.id).await.unwrap(),
        vec![shelf.id.clone()]
    );
    assert!(
        store
            .game_collection_ids(&account, &alpha.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .add_to_collection(&account, &shelf.id, &["missing-game".into()])
            .await
            .is_err()
    );

    let installed = store
        .save_collection(
            &account,
            &SaveCollection {
                id: None,
                name: "installed".into(),
                description: String::new(),
                kind: CollectionKind::Smart {
                    query: ListGames {
                        filter: GameFilter::Installed,
                        ..Default::default()
                    },
                },
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .add_to_collection(&account, &installed.id, &ids(&[&gamma]))
            .await
            .is_err()
    );
    store
        .set_collection_pinned(&account, &installed.id, true)
        .await
        .unwrap();

    let summaries = store.collection_summaries(&account).await.unwrap();
    let names: Vec<_> = summaries.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["installed", "Shelf"], "pinned first");
    assert_eq!((summaries[0].games, summaries[0].playtime_ms), (2, 3000));
    assert_eq!((summaries[1].games, summaries[1].playtime_ms), (2, 3000));
    assert_eq!(summaries[1].description, "By hand");
    assert!(summaries[0].pinned);

    // Hidden games leave the counts.
    store.set_game_hidden(&beta.id, true).await.unwrap();
    let summaries = store.collection_summaries(&account).await.unwrap();
    assert_eq!((summaries[1].games, summaries[1].playtime_ms), (1, 0));

    // Renaming keeps the games; turning smart drops them.
    let renamed = store
        .save_collection(
            &account,
            &SaveCollection {
                id: Some(shelf.id.clone()),
                name: "Renamed".into(),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.game_ids, ids(&[&gamma, &beta]));
    let smart = store
        .save_collection(
            &account,
            &SaveCollection {
                id: Some(shelf.id.clone()),
                name: "Renamed".into(),
                description: String::new(),
                kind: CollectionKind::Smart {
                    query: ListGames::default(),
                },
            },
        )
        .await
        .unwrap();
    assert!(smart.game_ids.is_empty());
    assert!(
        store
            .save_collection(
                &account,
                &SaveCollection {
                    id: Some("missing".into()),
                    name: "Nope".into(),
                    description: String::new(),
                    kind: CollectionKind::Manual,
                }
            )
            .await
            .is_err()
    );
    assert!(
        store
            .save_collection(
                &account,
                &SaveCollection {
                    id: None,
                    name: String::new(),
                    description: String::new(),
                    kind: CollectionKind::Manual,
                }
            )
            .await
            .is_err()
    );
}
