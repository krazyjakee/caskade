//! All input is synthetic or from the authored discovery machine fixtures.
use async_trait::async_trait;
use caskade_core::{
    bus::{Bus, Event, Intercept, Interceptor, LibraryImport},
    db::LibraryStore,
    model::*,
};
use caskade_discovery::{
    Cancel, Confidence, DetectedGame, GameGroup, HostEnv, ScanDelta, ScanReport, SourceError,
    SourceReport,
};
use caskade_sources::{DiscoveryService, ImportOptions};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

fn machine(id: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../caskade-discovery/tests/fixtures/machines")
        .join(id)
}
fn service(store: &LibraryStore, bus: &Bus) -> DiscoveryService {
    DiscoveryService::new(
        store.clone(),
        bus.clone(),
        HostEnv::fixture(machine("linux-integration")).unwrap(),
    )
}
fn detected(source: &str, key: &str, title: &str) -> DetectedGame {
    let mut game = DetectedGame::new(SourceId::from(source.to_owned()), key, title);
    game.install_dir = Some(PathBuf::from(format!("/synthetic/{key}")));
    game.launch.push(caskade_discovery::LaunchTarget::Uri {
        uri: format!("fixture://launch/{key}"),
    });
    game
}
fn group(entries: Vec<DetectedGame>) -> GameGroup {
    GameGroup {
        primary: 0,
        entries,
    }
}
fn report(groups: Vec<GameGroup>) -> ScanReport {
    let ids: BTreeSet<_> = groups
        .iter()
        .flat_map(|g| &g.entries)
        .map(|e| e.source.clone())
        .collect();
    let sources = ids
        .into_iter()
        .map(|source| SourceReport {
            count: groups
                .iter()
                .flat_map(|g| &g.entries)
                .filter(|g| g.source == source)
                .count() as u32,
            source,
            duration_ms: 0,
            warnings: vec![],
            error: None,
        })
        .collect();
    ScanReport {
        groups,
        sources,
        launchers: vec![],
    }
}

#[tokio::test]
async fn import_preserves_mapping_tracking_runtime_and_user_edits_on_refresh() {
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let service = service(&store, &bus);
    let mut game = detected("lutris", "native:1", "Synthetic Wine Game");
    game.launch = vec![caskade_discovery::LaunchTarget::Exec {
        program: PathBuf::from("/synthetic/game.exe"),
        args: vec!["a literal; argument".into()],
        cwd: Some(PathBuf::from("/synthetic")),
        env: std::collections::BTreeMap::from([("LANG".into(), "en_GB".into())]),
    }];
    game.ids = vec![ExternalId::Gog {
        product_id: 1207658924,
    }];
    game.runtime = GameRuntime::Windows {
        prefix: Some(caskade_discovery::Prefix {
            path: PathBuf::from("/synthetic/prefix"),
            arch: Some(caskade_discovery::PrefixArch::Win64),
            owner: Some(SourceId::new("lutris")),
        }),
        runner: Some(caskade_discovery::RunnerHint {
            kind: caskade_discovery::RunnerKind::Wine,
            name: Some("synthetic-wine".into()),
            path: Some(PathBuf::from("/synthetic/wine")),
        }),
    };
    game.tracking = Some(TrackingHint {
        steam_app_id: Some(620),
        install_dir: game.install_dir.clone(),
        exe_names: vec!["game.exe".into()],
    });
    game.size = Some(1234);
    let scan = report(vec![group(vec![game.clone()])]);
    let first = service
        .import_report(&scan, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    let saved = store
        .get_game(&first.imported[0].game_id)
        .await
        .unwrap()
        .unwrap();
    let entry = &saved.entries[0];
    assert_eq!(entry.source_id, game.source);
    assert_eq!(entry.external_id, game.key);
    assert_eq!(entry.ids, game.ids);
    assert_eq!(entry.runtime, game.runtime);
    assert_eq!(entry.tracking, game.tracking);
    assert_eq!(
        entry.location,
        game.install_dir
            .clone()
            .map(|path| InstallLocation::External { path })
    );
    assert_eq!(
        entry.launch,
        game.launch.first().cloned().map(LaunchSpec::from)
    );
    assert_eq!(entry.size, Some(1234));
    let mut user = saved.game.clone();
    user.title = "User title".into();
    user.favorite = true;
    user.notes = "Preserve me".into();
    store.upsert_game(&user).await.unwrap();
    let mut refreshed = scan.clone();
    refreshed.groups[0].entries[0].size = Some(5678);
    refreshed.groups[0].entries[0].install_dir = Some(PathBuf::from("/synthetic/moved"));
    let next = service
        .import_report(&refreshed, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(next.imported[0].game_id, user.id);
    assert!(!next.imported[0].created);
    let saved = store.get_game(&user.id).await.unwrap().unwrap();
    assert_eq!(saved.game, user);
    assert_eq!(saved.entries[0].id, entry.id);
    assert_eq!(saved.entries[0].size, Some(5678));
}

#[tokio::test]
async fn history_seeds_maximum_total_once_and_can_be_disabled_on_first_import() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());
    let mut steam = detected("steam", "620", "Portal");
    steam.playtime_secs = Some(3600);
    steam.last_played_unix = Some(1_700_000_000);
    let mut lutris = detected("lutris", "native:1", "Portal");
    lutris.playtime_secs = Some(1800);
    lutris.last_played_unix = Some(1_600_000_000);
    let scan = report(vec![group(vec![steam, lutris])]);
    let first = service
        .import_report(&scan, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert!(first.imported[0].seeded_history);
    let id = &first.imported[0].game_id;
    // A launcher's total is reported play time (#169), not a session.
    assert!(store.sessions_for_game(id, 100).await.unwrap().is_empty());
    let seeded = store.playtime_breakdown(id).await.unwrap();
    assert_eq!(seeded.total_ms, 3_600_000);
    assert_eq!(seeded.last_played_ms, Some(1_700_000_000_000));
    assert_eq!(seeded.sources.len(), 1);
    assert_eq!(seeded.sources[0].provider, "steam");
    assert_eq!(seeded.sources[0].account_name, None);
    let mut later = scan.clone();
    later.groups[0].entries[0].playtime_secs = Some(9999);
    service
        .import_report(&later, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(store.playtime_breakdown(id).await.unwrap(), seeded);

    let mut disabled = detected("heroic", "gog:1", "Disabled");
    disabled.playtime_secs = Some(7200);
    let scan = report(vec![group(vec![disabled])]);
    let imported = service
        .import_report(
            &scan,
            &ImportOptions {
                seed_playtime: false,
                ..Default::default()
            },
            &Cancel::new(),
        )
        .await
        .unwrap();
    service
        .import_report(&scan, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    let game = &imported.imported[0].game_id;
    assert!(store.sessions_for_game(game, 10).await.unwrap().is_empty());
    assert!(
        store
            .playtime_breakdown(game)
            .await
            .unwrap()
            .sources
            .is_empty()
    );
}

struct Rewrite;
#[async_trait]
impl Interceptor<LibraryImport> for Rewrite {
    async fn intercept(
        &self,
        mut game: DetectedGame,
    ) -> std::result::Result<Intercept<DetectedGame>, String> {
        if game.key == "drop" {
            return Ok(Intercept::Cancel("synthetic exclusion".into()));
        }
        game.title = format!("Rewritten {}", game.title);
        Ok(Intercept::Continue(game))
    }
}
#[tokio::test]
async fn interceptors_rewrite_or_drop_before_persistence_and_completion_follows_commit() {
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    bus.intercept::<LibraryImport>("rewrite", 0, Arc::new(Rewrite));
    let mut events = bus.stream();
    let service = service(&store, &bus);
    let scan = report(vec![
        group(vec![detected("steam", "drop", "Dropped")]),
        group(vec![detected("steam", "keep", "Kept")]),
    ]);
    let imported = service
        .import_report(&scan, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(imported.skipped.len(), 1);
    assert_eq!(imported.imported.len(), 1);
    assert_eq!(store.library_stats().await.unwrap().total, 1);
    let Event::LibraryGameAdded { game_id } = events.recv().await.unwrap() else {
        panic!("game-added event expected")
    };
    assert_eq!(
        store.game(&game_id).await.unwrap().unwrap().title,
        "Rewritten Kept"
    );
    let Event::LibraryScanCompleted { sources } = events.recv().await.unwrap() else {
        panic!("completion expected")
    };
    assert_eq!(sources[0].source, "steam");
    assert_eq!(sources[0].count, 2);
    assert!(
        store
            .entry_by_source("steam", "drop")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn maybe_results_are_reviewed_and_cancelled_scans_never_import() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());
    let mut game = detected("desktop", "maybe", "Maybe");
    game.confidence = Confidence::Maybe;
    let scan = report(vec![group(vec![game])]);
    let outcome = service
        .import_report(&scan, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(outcome.review.len(), 1);
    assert!(outcome.imported.is_empty());
    let cancelled = Cancel::new();
    cancelled.cancel();
    assert!(
        service
            .import_report(
                &scan,
                &ImportOptions {
                    accept_maybe: true,
                    ..Default::default()
                },
                &cancelled
            )
            .await
            .is_err()
    );
    assert_eq!(store.library_stats().await.unwrap().total, 0);
    let accepted = service
        .import_report(
            &scan,
            &ImportOptions {
                accept_maybe: true,
                ..Default::default()
            },
            &Cancel::new(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.imported.len(), 1);
}

#[tokio::test]
async fn invalid_group_rolls_back_without_orphans_and_later_groups_still_import() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());
    let mut invalid = detected("fixture", "bad", "Bad");
    invalid.install_dir = Some(PathBuf::from("relative/path"));
    let scan = report(vec![
        group(vec![detected("fixture", "valid-member", "Bad"), invalid]),
        group(vec![detected("fixture", "good", "Good")]),
    ]);
    let outcome = service
        .import_report(&scan, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(outcome.warnings.len(), 1);
    assert_eq!(store.library_stats().await.unwrap().total, 1);
    assert!(
        store
            .entry_by_source("fixture", "valid-member")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .entry_by_source("fixture", "good")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn newly_shared_group_merges_previously_separate_games_and_keeps_history() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());
    let a = detected("steam", "1", "Primary");
    let b = detected("lutris", "2", "Alternative");
    let initial = service
        .import_report(
            &report(vec![group(vec![a.clone()]), group(vec![b.clone()])]),
            &ImportOptions::default(),
            &Cancel::new(),
        )
        .await
        .unwrap();
    let first = &initial.imported[0].game_id;
    let second = &initial.imported[1].game_id;
    let entry = store.entries_for_game(second).await.unwrap().remove(0);
    let mut session = PlaySession::new(second, &entry.id, 1000);
    session.ended_at_ms = Some(2000);
    session.duration_ms = 1000;
    store.upsert_session(&session).await.unwrap();
    let outcome = service
        .import_report(
            &report(vec![group(vec![a, b])]),
            &ImportOptions::default(),
            &Cancel::new(),
        )
        .await
        .unwrap();
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    assert_eq!(outcome.imported[0].game_id, *first);
    assert_eq!(store.library_stats().await.unwrap().total, 1);
    assert_eq!(store.entries_for_game(first).await.unwrap().len(), 2);
    assert_eq!(
        store.sessions_for_game(first, 10).await.unwrap()[0].id,
        session.id
    );
    assert!(store.game(second).await.unwrap().is_none());
}

#[tokio::test]
async fn identity_proxies_are_not_entries_and_earlier_proxy_rows_fold_into_the_owner() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());
    let steam = detected("steam", "730", "Counter-Strike 2");
    // A desktop shortcut that runs `steam steam://rungameid/730`.
    let mut shortcut = detected("desktop", "Counter-Strike 2.desktop", "Counter-Strike 2");
    shortcut.install_dir = None;
    shortcut.ids = vec![ExternalId::Steam { app_id: 730 }];
    caskade_discovery::uri::mark_proxy(&mut shortcut);
    // A row saved before proxies were skipped, with a finished session.
    let legacy = service
        .import_report(
            &report(vec![group(vec![shortcut.clone()])]),
            &ImportOptions::default(),
            &Cancel::new(),
        )
        .await
        .unwrap();
    let legacy_game = &legacy.imported[0].game_id;
    let legacy_entry = store.entries_for_game(legacy_game).await.unwrap().remove(0);
    let mut session = PlaySession::new(legacy_game, &legacy_entry.id, 1000);
    session.ended_at_ms = Some(2000);
    session.duration_ms = 1000;
    store.upsert_session(&session).await.unwrap();

    let outcome = service
        .import_report(
            &report(vec![group(vec![steam, shortcut])]),
            &ImportOptions::default(),
            &Cancel::new(),
        )
        .await
        .unwrap();
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    assert_eq!(outcome.imported[0].entries, 1);
    let game = &outcome.imported[0].game_id;
    let entries = store.entries_for_game(game).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source_id.as_str(), "steam");
    assert_eq!(store.library_stats().await.unwrap().total, 1);
    assert!(
        store
            .entry_by_source("desktop", "Counter-Strike 2.desktop")
            .await
            .unwrap()
            .is_none()
    );
    let sessions = store.sessions_for_game(game, 10).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session.id);
    assert_eq!(sessions[0].entry_id, entries[0].id);
}

#[tokio::test]
async fn successful_absence_marks_partial_but_failed_sources_and_history_are_preserved() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());
    let scan = report(vec![
        group(vec![detected("steam", "a", "A")]),
        group(vec![detected("heroic", "b", "B")]),
    ]);
    service
        .import_report(&scan, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    let mut empty = report(vec![]);
    empty.sources = scan.sources.clone();
    empty.sources[1].error = Some(SourceError::TimedOut);
    // reports are sorted: heroic first, steam second; mark heroic missing.
    service
        .import_report(&empty, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(
        store
            .entry_by_source("heroic", "b")
            .await
            .unwrap()
            .unwrap()
            .state,
        InstallState::Partial
    );
    assert_eq!(
        store
            .entry_by_source("steam", "a")
            .await
            .unwrap()
            .unwrap()
            .state,
        InstallState::Installed
    );
    assert_eq!(store.library_stats().await.unwrap().total, 2);
}

#[tokio::test]
async fn delta_updates_partial_membership_and_removes_without_erasing_library_records() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());
    let a = detected("steam", "a", "A");
    let b = detected("desktop", "b", "A");
    let initial = report(vec![group(vec![a.clone(), b.clone()])]);
    service
        .import_report(&initial, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    let changed = ScanDelta {
        updated: vec![group(vec![a.clone()])],
        sources: initial.sources.clone(),
        ..Default::default()
    };
    service
        .apply_delta(&changed, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(
        store
            .entry_by_source("desktop", "b")
            .await
            .unwrap()
            .unwrap()
            .state,
        InstallState::Partial
    );
    assert_eq!(
        store
            .entry_by_source("steam", "a")
            .await
            .unwrap()
            .unwrap()
            .state,
        InstallState::Installed
    );
    let removed = ScanDelta {
        removed: vec![group(vec![a])],
        sources: initial.sources,
        ..Default::default()
    };
    service
        .apply_delta(&removed, &ImportOptions::default(), &Cancel::new())
        .await
        .unwrap();
    assert_eq!(store.library_stats().await.unwrap().installed, 0);
    assert_eq!(store.library_stats().await.unwrap().total, 1);
}

#[tokio::test]
async fn every_builtin_family_has_a_bus_adapter_and_disabled_sources_are_inert() {
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let config = caskade_discovery::Config {
        sources: caskade_discovery::SourceSelection::only([]),
        ..Default::default()
    };
    caskade_sources::register(
        &bus,
        store,
        HostEnv::fixture(machine("linux-integration")).unwrap(),
        &config,
    );
    let providers = bus.library_sources();
    let expected: BTreeSet<_> = caskade_discovery::types::builtin_sources()
        .iter()
        .map(|s| s.id().to_string())
        .collect();
    assert_eq!(
        providers
            .iter()
            .map(|s| s.id().to_string())
            .collect::<BTreeSet<_>>(),
        expected
    );
    for provider in providers {
        assert!(provider.scan().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn seed_preference_round_trips_through_the_shared_settings_service() {
    use caskade_core::settings::SettingsService;
    use caskade_sources::{IMPORT_SETTINGS, import_options, register_settings};
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    register_settings(&settings).unwrap();
    assert!(import_options(&settings).await.unwrap().seed_playtime);
    settings
        .set(
            IMPORT_SETTINGS,
            serde_json::json!({"seedPlaytime":false}),
            None,
        )
        .await
        .unwrap();
    let reopened = SettingsService::new(store);
    register_settings(&reopened).unwrap();
    assert!(!import_options(&reopened).await.unwrap().seed_playtime);
    assert!(
        reopened
            .set(
                IMPORT_SETTINGS,
                serde_json::json!({"seedPlaytime":"invalid"}),
                None
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn later_identity_merge_keeps_one_aggregate_seed_and_every_actual_play() {
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());
    let mut a = detected("steam", "a", "A");
    a.playtime_secs = Some(7200);
    let mut b = detected("heroic", "b", "B");
    b.playtime_secs = Some(3600);
    let initial = service
        .import_report(
            &report(vec![group(vec![a.clone()]), group(vec![b.clone()])]),
            &ImportOptions::default(),
            &Cancel::new(),
        )
        .await
        .unwrap();
    let second = &initial.imported[1].game_id;
    let entry = store.entries_for_game(second).await.unwrap().remove(0);
    let mut actual = PlaySession::new(second, &entry.id, 1000);
    actual.ended_at_ms = Some(2000);
    actual.duration_ms = 1000;
    store.upsert_session(&actual).await.unwrap();
    let merged = service
        .import_report(
            &report(vec![group(vec![a, b])]),
            &ImportOptions::default(),
            &Cancel::new(),
        )
        .await
        .unwrap();
    assert!(merged.warnings.is_empty(), "{:?}", merged.warnings);
    let game = &merged.imported[0].game_id;
    let sessions = store.sessions_for_game(game, 100).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions.iter().any(|s| s.id == actual.id));
    // Both launchers' totals are the game's own play: the larger counts once,
    // with the session Caskade tracked after them.
    let playtime = store.playtime_breakdown(game).await.unwrap();
    assert_eq!(playtime.total_ms, 7_201_000, "{playtime:?}");
}

#[tokio::test]
async fn persistence_failure_rolls_back_the_game_entries_and_first_import_marker() {
    let store = LibraryStore::in_memory().await.unwrap();
    let original = Game::new("Original");
    store.upsert_game(&original).await.unwrap();
    let original_entry =
        GameEntry::from_detected(&original.id, &detected("fixture", "original", "Original"));
    store.upsert_entry(&original_entry).await.unwrap();
    let mut history = PlaySession::new(&original.id, &original_entry.id, 0);
    history.ended_at_ms = Some(0);
    store.upsert_session(&history).await.unwrap();
    let game = Game::new("Rollback");
    let entry = GameEntry::from_detected(&game.id, &detected("fixture", "rollback", "Rollback"));
    // A total no database can hold fails the import, which rolls back.
    let mut duplicate_id = PlaySession::new(&game.id, &entry.id, 0);
    duplicate_id.ended_at_ms = Some(0);
    duplicate_id.duration_ms = u64::MAX;
    assert!(
        store
            .import_group(&game, std::slice::from_ref(&entry), Some(&duplicate_id))
            .await
            .is_err()
    );
    assert!(store.game(&game.id).await.unwrap().is_none());
    assert!(
        store
            .entry_by_source("fixture", "rollback")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_setting(&format!("discovery.imported.{}", game.id))
            .await
            .unwrap()
            .is_none()
    );
    let saved = store.import_group(&game, &[entry], None).await.unwrap();
    assert!(saved.created);
}
