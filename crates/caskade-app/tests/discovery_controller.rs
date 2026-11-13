//! The discovery commands over an authored fixture and temporary SQLite.
//! No live HostEnv, registry, launcher files or real user database are read.
use caskade_app::{
    commands::discovery as commands,
    discovery::{DiscoveryController, DiscoverySnapshot},
    events::Event,
};
use caskade_core::{AppPaths, bus::Bus, db::LibraryStore, settings::SettingsService};
use caskade_discovery::{
    Confidence, DetectedGame, Evidence, HostEnv, LauncherStatus, OsSet, ScanCtx, Source,
    SourceError, SourceId, WatchPath,
};
use caskade_sources::DiscoveryService;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

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

async fn complete(controller: &DiscoveryController) -> DiscoverySnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = commands::discovery_status(controller).await.unwrap();
        if snapshot.phase == "complete" {
            return snapshot;
        }
        assert_ne!(snapshot.phase, "failed", "{:?}", snapshot.error);
        assert!(Instant::now() < deadline, "{}", snapshot.phase);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn scan_review_accept_ignore_and_stale_rejection_round_trip_through_the_commands() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(temp.path()))
        .unwrap();
    caskade_sources::register_settings(&settings).unwrap();
    let env = HostEnv::fixture(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../caskade-discovery/tests/fixtures/machines/linux-integration"),
    )
    .unwrap();
    let service =
        DiscoveryService::with_sources(store.clone(), Bus::default(), env, vec![Arc::new(Desktop)]);
    let controller = DiscoveryController::new(service, settings);
    let sink = crate::Sink::default();
    controller.bridge(sink.events());

    let launchers = commands::discovery_launchers(&controller).await.unwrap();
    assert_eq!(launchers.os, "linux");
    assert!(!launchers.launchers[0].found);
    let queued = commands::discovery_scan(&controller).await.unwrap();
    assert_eq!(queued.phase, "scanning");
    let snapshot = complete(&controller).await;
    assert_eq!(snapshot.review.len(), 2);
    assert_eq!(store.library_stats().await.unwrap().total, 0);
    let first = snapshot.review[0].id.clone();
    let revision = snapshot.revision;
    assert!(
        commands::discovery_review(&controller, vec!["forged-record".into()], true, revision)
            .await
            .is_err()
    );
    assert!(
        commands::discovery_review(&controller, vec![first.clone()], true, revision - 1)
            .await
            .is_err()
    );
    let accepted = commands::discovery_review(&controller, vec![first], true, revision)
        .await
        .unwrap();
    assert_eq!(store.library_stats().await.unwrap().total, 1);
    assert_eq!(accepted.review.len(), 1);
    let ignored = commands::discovery_review(
        &controller,
        vec![accepted.review[0].id.clone()],
        false,
        accepted.revision,
    )
    .await
    .unwrap();
    assert!(ignored.review.is_empty());
    commands::discovery_scan(&controller).await.unwrap();
    let rescanned = complete(&controller).await;
    assert!(rescanned.review.is_empty());
    assert_eq!(store.library_stats().await.unwrap().total, 1);
    assert!(
        sink.wait_for(Duration::from_secs(2), |event| match event {
            Event::DiscoveryChanged(changed) if changed.0.phase == "complete" => Some(()),
            _ => None,
        })
        .is_some(),
        "completion event missing"
    );
    store.close().await;
}
