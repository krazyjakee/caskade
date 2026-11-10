//! Native-command contract over an authored fixture and temporary SQLite.
//! No live HostEnv, registry, launcher files or real user database are read.
use crate::ipc::invoke;
use caskade_core::{AppPaths, bus::Bus, db::LibraryStore, settings::SettingsService};
use caskade_desktop::{discovery::DiscoveryController, events::DiscoveryChanged};
use caskade_discovery::{
    Confidence, DetectedGame, Evidence, HostEnv, LauncherStatus, OsSet, ScanCtx, Source,
    SourceError, SourceId, WatchPath,
};
use caskade_sources::DiscoveryService;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    Listener,
    test::{mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;
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
        Ok(["first", "second"]
            .into_iter()
            .map(|key| {
                let mut game = DetectedGame::new(self.id(), key, format!("Original fixture {key}"));
                game.confidence = Confidence::Maybe;
                game.evidence.push(Evidence::Rule {
                    rule: "fixture.game".into(),
                    score: 25,
                });
                game
            })
            .collect())
    }
    fn watch_paths(&self, _: &HostEnv) -> Vec<WatchPath> {
        vec![]
    }
}
fn complete(view: &tauri::WebviewWindow<tauri::test::MockRuntime>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = invoke(view, "discovery_status", json!({})).unwrap();
        if snapshot["phase"] == "complete" {
            return snapshot;
        }
        assert_ne!(snapshot["phase"], "failed", "{snapshot}");
        assert!(Instant::now() < deadline, "{snapshot}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn scan_review_accept_ignore_and_stale_rejection_round_trip_through_typed_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let (store, controller) = tauri::async_runtime::block_on(async {
        let store = LibraryStore::in_memory().await.unwrap();
        let bus = Bus::default();
        let settings = SettingsService::new(store.clone());
        settings
            .register_core(&AppPaths::under(temp.path()))
            .unwrap();
        caskade_sources::register_settings(&settings).unwrap();
        let env =
            HostEnv::fixture(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
                "../../../crates/caskade-discovery/tests/fixtures/machines/linux-integration",
            ))
            .unwrap();
        let service =
            DiscoveryService::with_sources(store.clone(), bus, env, vec![Arc::new(Desktop)]);
        (store, DiscoveryController::new(service, settings))
    });
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .dangerously_cast_bigints_to_number()
        .commands(tauri_specta::collect_commands![
            caskade_desktop::commands::discovery::discovery_status,
            caskade_desktop::commands::discovery::discovery_scan,
            caskade_desktop::commands::discovery::discovery_review,
            caskade_desktop::commands::discovery::discovery_launchers,
            caskade_desktop::commands::discovery::discovery_cancel
        ])
        .events(tauri_specta::collect_events![DiscoveryChanged]);
    let app = mock_builder()
        .manage(controller.clone())
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let view = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = events.clone();
    app.listen(DiscoveryChanged::NAME, move |event| {
        seen.lock()
            .unwrap()
            .push(serde_json::from_str(event.payload()).unwrap());
    });
    controller.bridge(caskade_desktop::events::sink(app.handle().clone()));
    let launchers = invoke(&view, "discovery_launchers", json!({})).unwrap();
    assert_eq!(launchers["os"], "linux");
    assert_eq!(launchers["launchers"][0]["found"], false);
    let queued = invoke(&view, "discovery_scan", json!({})).unwrap();
    assert_eq!(queued["phase"], "scanning");
    let snapshot = complete(&view);
    assert_eq!(snapshot["review"].as_array().unwrap().len(), 2);
    assert_eq!(
        tauri::async_runtime::block_on(store.library_stats())
            .unwrap()
            .total,
        0
    );
    let first = snapshot["review"][0]["id"].clone();
    let revision = snapshot["revision"].as_u64().unwrap();
    assert!(
        invoke(
            &view,
            "discovery_review",
            json!({"ids":["forged-record"],"accept":true,"revision":revision})
        )
        .is_err()
    );
    assert!(
        invoke(
            &view,
            "discovery_review",
            json!({"ids":[first],"accept":true,"revision":revision-1})
        )
        .is_err()
    );
    let accepted = invoke(
        &view,
        "discovery_review",
        json!({"ids":[first],"accept":true,"revision":revision}),
    )
    .unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(store.library_stats())
            .unwrap()
            .total,
        1
    );
    assert_eq!(accepted["review"].as_array().unwrap().len(), 1);
    let ignored = invoke(
        &view,
        "discovery_review",
        json!({"ids":[accepted["review"][0]["id"]],"accept":false,"revision":accepted["revision"]}),
    )
    .unwrap();
    assert_eq!(ignored["review"], json!([]));
    invoke(&view, "discovery_scan", json!({})).unwrap();
    let rescanned = complete(&view);
    assert_eq!(rescanned["review"], json!([]));
    assert_eq!(
        tauri::async_runtime::block_on(store.library_stats())
            .unwrap()
            .total,
        1
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while events
        .lock()
        .unwrap()
        .iter()
        .all(|event| event["phase"] != "complete")
    {
        assert!(Instant::now() < deadline, "completion event missing");
        std::thread::sleep(Duration::from_millis(10));
    }
    tauri::async_runtime::block_on(store.close());
}
