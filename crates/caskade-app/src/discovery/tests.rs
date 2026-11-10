use super::*;
use caskade_core::{
    AppPaths,
    bus::Bus,
    db::LibraryStore,
    model::ListGames,
    settings::{DiscoverySettings, LauncherSettings},
};
use caskade_discovery::{
    Confidence, DetectedGame, Evidence, HostEnv, OsSet, ScanCtx, Source, SourceError, SourceId,
    WatchPath,
};
use std::path::PathBuf;

struct Desktop;
impl Source for Desktop {
    fn id(&self) -> SourceId {
        SourceId::new("desktop")
    }
    fn hosts(&self) -> OsSet {
        OsSet::LINUX
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![LauncherStatus::not_found(self.id())]
    }
    fn scan(&self, _: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        ctx.check()?;
        let mut game = DetectedGame::new(self.id(), "fixture.desktop", "Review Fixture Game");
        game.confidence = Confidence::Maybe;
        game.evidence = vec![Evidence::Rule {
            rule: "fixture.game".into(),
            score: 25,
        }];
        game.launch.push(caskade_discovery::LaunchTarget::Uri {
            uri: "fixture://game".into(),
        });
        Ok(vec![game])
    }
    fn watch_paths(&self, _: &HostEnv) -> Vec<WatchPath> {
        vec![]
    }
}
async fn controller() -> (DiscoveryController, LibraryStore) {
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(std::path::Path::new("/fixture/app")))
        .unwrap();
    caskade_sources::register_settings(&settings).unwrap();
    let env = HostEnv::fixture(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../caskade-discovery/tests/fixtures/machines/linux-integration"),
    )
    .unwrap();
    let service = DiscoveryService::with_sources(store.clone(), bus, env, vec![Arc::new(Desktop)]);
    (DiscoveryController::new(service, settings), store)
}
async fn completed(controller: &DiscoveryController) -> DiscoverySnapshot {
    let mut updates = controller.subscribe();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let snapshot = updates.borrow_and_update().clone();
            if matches!(snapshot.phase.as_str(), "complete" | "failed" | "cancelled") {
                return snapshot;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn explicit_review_imports_retained_groups_and_never_marks_unrelated_entries_absent() {
    let (controller, store) = controller().await;
    let initial = controller.scan().await.unwrap();
    assert_eq!(initial.phase, "scanning");
    let snapshot = completed(&controller).await;
    assert_eq!(snapshot.phase, "complete", "{:?}", snapshot.error);
    assert_eq!(store.library_stats().await.unwrap().total, 0);
    assert_eq!(snapshot.review.len(), 1);
    assert_eq!(snapshot.sources[0].count, 1);
    assert_eq!(snapshot.review[0].evidence, ["fixture.game: +25"]);
    let id = snapshot.review[0].id.clone();
    let other = DetectedGame::new(SourceId::new("desktop"), "other.desktop", "Unrelated game");
    controller
        .0
        .service
        .import_report(
            &ScanReport {
                groups: vec![GameGroup {
                    primary: 0,
                    entries: vec![other],
                }],
                ..Default::default()
            },
            &caskade_sources::ImportOptions::default(),
            &Cancel::new(),
        )
        .await
        .unwrap();
    let stale = controller
        .review(vec![id.clone()], true, snapshot.revision - 1)
        .await
        .unwrap_err();
    assert_eq!(stale.code, crate::error::ErrorCode::Conflict);
    let unknown = controller
        .review(vec!["client-authored".into()], true, snapshot.revision)
        .await
        .unwrap_err();
    assert_eq!(unknown.code, crate::error::ErrorCode::Conflict);
    assert!(!unknown.summary.contains("client-authored"));
    let accepted = controller
        .review(vec![id], true, snapshot.revision)
        .await
        .unwrap();
    assert!(accepted.review.is_empty());
    assert_eq!(
        store.list_games(&ListGames::default()).await.unwrap().total,
        2
    );
    let entry = store
        .entry_by_source("desktop", "fixture.desktop")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.state, caskade_core::model::InstallState::Installed);

    assert_eq!(
        store
            .entry_by_source("desktop", "other.desktop")
            .await
            .unwrap()
            .unwrap()
            .state,
        caskade_core::model::InstallState::Installed
    );
    controller.scan().await.unwrap();
    let rescanned = completed(&controller).await;
    assert!(
        rescanned.review.is_empty(),
        "accepted games must not be re-prompted"
    );
}

#[tokio::test]
async fn lifecycle_starts_once_and_stops_its_watcher() {
    let (controller, _) = controller().await;
    controller.start();
    controller.start();
    let mut updates = controller.subscribe();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if updates.borrow_and_update().watching {
                break;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), controller.stop())
        .await
        .unwrap();
    assert!(!controller.snapshot().await.watching);
    assert!(!controller.0.started.load(Ordering::Acquire));
}

#[tokio::test]
async fn the_startup_scan_waits_for_first_run_setup() {
    let (controller, _) = controller().await;
    controller.start();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(controller.snapshot().await.phase, "idle");
    let settings = &controller.0.settings;
    let mut onboarding = settings.get("core.onboarding").await.unwrap().value;
    onboarding["completed"] = true.into();
    settings
        .set("core.onboarding", onboarding, None)
        .await
        .unwrap();
    assert_eq!(completed(&controller).await.phase, "complete");
    controller.stop().await;
}

#[tokio::test]
async fn ignore_is_persistent_and_changes_the_next_backend_scan() {
    let (controller, store) = controller().await;
    controller.scan().await.unwrap();
    let snapshot = completed(&controller).await;
    let ignored = controller
        .review(
            vec![snapshot.review[0].id.clone()],
            false,
            snapshot.revision,
        )
        .await
        .unwrap();
    assert!(ignored.review.is_empty());
    let saved = controller.0.settings.get("core.discovery").await.unwrap();
    assert_eq!(saved.value["ignoredEntries"][0]["key"], "fixture.desktop");
    controller.scan().await.unwrap();
    let next = completed(&controller).await;
    assert_eq!(next.found, 0);
    assert!(next.review.is_empty());
    assert_eq!(store.library_stats().await.unwrap().total, 0);
}

#[tokio::test]
async fn os_source_switch_disables_actual_scan_and_launcher_detection_uses_the_fixture() {
    let (controller, _) = controller().await;
    let snapshot = controller.launchers().await.unwrap();
    assert_eq!(snapshot.os, "linux");
    assert!(!snapshot.launchers[0].found);
    let mut preferences = controller.0.settings.get("core.discovery").await.unwrap();
    preferences.value["osChecks"]["linux"]["desktop"] = serde_json::json!(false);
    controller
        .0
        .settings
        .set(
            "core.discovery",
            preferences.value,
            Some(preferences.revision),
        )
        .await
        .unwrap();
    controller.scan().await.unwrap();
    let next = completed(&controller).await;
    assert!(next.sources.is_empty());
    assert!(next.review.is_empty());
}

#[tokio::test]
async fn old_v1_settings_without_additive_fields_still_validate_and_deserialize() {
    let (controller, _) = controller().await;
    let mut old = serde_json::to_value(DiscoverySettings::default()).unwrap();
    old.as_object_mut().unwrap().remove("scanPaths");
    old.as_object_mut().unwrap().remove("ignoredEntries");
    old.as_object_mut().unwrap().remove("acceptedEntries");
    controller
        .0
        .settings
        .set("core.discovery", old, Some(0))
        .await
        .unwrap();
    let (preferences, _) = config::preferences(&controller.0.settings).await.unwrap();
    assert!(preferences.scan_paths.is_empty());
    assert!(preferences.ignored_entries.is_empty());
}

#[test]
fn every_exposed_deep_and_source_preference_changes_the_scan_configuration() {
    let mut preferences = DiscoverySettings::default();
    let mut launchers = LauncherSettings::default();
    launchers.enabled.insert("steam".into(), false);
    preferences.deep_scans.custom_folders = true;
    preferences.scan_paths = vec![
        std::env::temp_dir()
            .join("fixture-games")
            .to_string_lossy()
            .into_owned(),
    ];
    preferences.deep_scans.external_drives = true;
    preferences.deep_scans.portable_executables = true;
    preferences.deep_scans.wine_prefixes = true;
    preferences.deep_scans.full_disk = true;
    let config = config::scan_config(
        &preferences,
        &launchers,
        caskade_discovery::OperatingSystem::Linux,
    )
    .unwrap();
    assert_eq!(config.deep.folders.len(), 1);
    assert!(
        config.deep.external_drives
            && config.deep.portable
            && config.deep.wine_prefixes
            && config.deep.full_disk
    );
    let serialized = serde_json::to_value(config.sources).unwrap();
    assert!(serialized.to_string().contains("steam.shortcuts"));
    preferences.scan_paths = vec!["relative/untrusted".into()];
    let relative = config::scan_config(
        &preferences,
        &launchers,
        caskade_discovery::OperatingSystem::Linux,
    )
    .unwrap_err();
    assert_eq!(relative.code, crate::error::ErrorCode::Invalid);
    for (value, seconds) in [
        ("1h", 3600),
        ("6h", 21600),
        ("12h", 43200),
        ("24h", 86400),
        ("7d", 604800),
    ] {
        preferences.refresh_interval = value.into();
        assert_eq!(config::refresh_interval(&preferences).as_secs(), seconds);
    }
}

#[tokio::test]
async fn a_settings_change_cancels_only_a_scan_it_makes_out_of_date() {
    let (controller, _store) = controller().await;
    let settings = controller.0.settings.clone();
    let (preferences, launchers) = config::preferences(&settings).await.unwrap();
    let running = Cancel::new();
    {
        let mut state = controller.0.state.lock().await;
        state.cancel = Some(running.clone());
        state.scanning_with = Some((
            config::scan_config(&preferences, &launchers, controller.0.service.env().os()).unwrap(),
            caskade_sources::import_options(&settings).await.unwrap(),
        ));
    }
    // First-run setup finishing changes nothing the scan uses.
    settings
        .set(
            "core.onboarding",
            serde_json::json!({ "completed": true }),
            None,
        )
        .await
        .ok();
    controller.cancel_if_outdated().await;
    assert!(!running.is_cancelled());
    // Discovery turned off: the scan is of an old configuration.
    let mut discovery = settings.get("core.discovery").await.unwrap().value;
    discovery["enabled"] = false.into();
    settings
        .set("core.discovery", discovery, None)
        .await
        .unwrap();
    controller.cancel_if_outdated().await;
    assert!(running.is_cancelled());
}

#[test]
fn wrapped_failures_are_classified_and_name_no_type() {
    use crate::error::ErrorCode;
    let settings = failure(caskade_sources::Error::Settings(
        caskade_core::settings::SettingsError::Unknown("core.discovery".into()),
    ));
    assert_eq!(settings.code, ErrorCode::Unavailable);
    assert!(!settings.summary.contains("core.discovery"), "{settings:?}");
    let denied = failure(caskade_sources::Error::Core(caskade_core::Error::Io(
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    )));
    assert_eq!(denied.code, ErrorCode::AccessDenied);
    let usage = failure(caskade_sources::Error::Discovery(
        caskade_discovery::Error::Usage("bad source key".into()),
    ));
    assert_eq!(usage.code, ErrorCode::Internal);
    assert!(!usage.summary.contains("source key"), "{usage:?}");
    assert_eq!(usage.detail.as_deref(), Some("bad source key"));
    assert_eq!(
        failure(caskade_sources::Error::Cancelled).code,
        ErrorCode::Cancelled
    );
}

#[test]
fn check_names_follow_the_language() {
    let mut checks = config::os_checks(caskade_discovery::OperatingSystem::Linux);
    assert_eq!(checks[0].label, "Desktop entries");
    let _sv = caskade_core::i18n::scope_catalog(
        "sv",
        r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Desktop entries"
msgstr "Skrivbordsposter"
"#,
    );
    config::relabel(&mut checks);
    assert_eq!(checks[0].label, "Skrivbordsposter");
}
