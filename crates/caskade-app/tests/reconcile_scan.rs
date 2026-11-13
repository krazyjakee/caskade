//! App discovery scans reconcile managed roots before library import (#61).
//! The scenario is authored in a temporary directory; no live launcher state.
use caskade_app::discovery::{DiscoveryController, DiscoverySnapshot};
use caskade_core::{
    AppPaths,
    bus::Bus,
    db::LibraryStore,
    launch::LaunchService,
    model::{InstallLocation, PlaySession, new_id},
    settings::SettingsService,
    storage::StorageService,
};
use caskade_discovery::HostEnv;
use caskade_sources::{
    DiscoveryService,
    storage::{StorageGameStatus, StorageSource},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Fixture {
    _temp: tempfile::TempDir,
    store: LibraryStore,
    controller: DiscoveryController,
    root: PathBuf,
    root_id: String,
    #[cfg(unix)]
    paths: AppPaths,
}
impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(&temp.path().join("app"));
        let store = LibraryStore::in_memory().await.unwrap();
        let bus = Bus::default();
        let settings = SettingsService::new(store.clone());
        settings.register_core(&paths).unwrap();
        caskade_sources::register_settings(&settings).unwrap();
        let storage = StorageService::new(
            store.clone(),
            settings.clone(),
            LaunchService::new(store.clone()),
        )
        .unwrap()
        .with_paths(paths.clone());
        let root = storage
            .register_root(temp.path().join("Games"), None)
            .await
            .unwrap()
            .root;
        let env = HostEnv::fixture(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../caskade-discovery/tests/fixtures/machines/linux-integration"),
        )
        .unwrap();
        let source = StorageSource::new(store.clone(), bus.clone(), storage.reconciler().unwrap())
            .with_remounts(storage, env.clone(), &paths);
        let service = DiscoveryService::with_sources(store.clone(), bus, env, vec![]);
        let controller = DiscoveryController::new(service, settings).with_storage(source);
        Self {
            _temp: temp,
            store,
            controller,
            root: root.path,
            root_id: root.id,
            #[cfg(unix)]
            paths,
        }
    }
    fn game(&self, relative: &str, verdict: &str, alias: Option<&str>) -> (PathBuf, String) {
        let dir = self.root.join(relative);
        fs::create_dir_all(dir.join(".caskade")).unwrap();
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("bin/play"), b"authored game bytes").unwrap();
        fs::write(dir.join("config.ini"), format!("path={}\n", dir.display())).unwrap();
        let id = new_id();
        let mut record: Value = serde_json::from_str(include_str!(
            "../../caskade-installer/tests/manifests/install.v1.posix.json"
        ))
        .unwrap();
        record["ids"] = json!({"caskade-install":id});
        record["title"] = json!("App scan fixture");
        record["plan"]["root_id"] = json!(self.root_id);
        record["plan"]["relative_dir"] = json!(relative);
        record["launch_targets"][0]["path"] = json!("bin/play");
        record["uninstall_steps"] = json!([]);
        record["external_effects"] = json!([]);
        record["alias"] = json!(alias);
        record["files"] = json!(["bin/play", "config.ini"].map(|name| json!({
            "path":name,"size":fs::metadata(dir.join(name)).unwrap().len(),"sha256":null
        })));
        record["relocation"] = json!({"verdict":verdict,"last_path":dir,"references":[]});
        if verdict == "fragile" {
            fs::write(
                dir.join("binary.dat"),
                [b"\0opaque\0".as_slice(), dir.to_str().unwrap().as_bytes()].concat(),
            )
            .unwrap();
        }
        fs::write(
            dir.join(".caskade/install.json"),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
        (dir, id)
    }
    async fn scan(&self) -> DiscoverySnapshot {
        let mut updates = self.controller.subscribe();
        self.controller.scan().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                let snapshot = updates.borrow_and_update().clone();
                if matches!(snapshot.phase.as_str(), "complete" | "failed" | "cancelled") {
                    assert_eq!(snapshot.phase, "complete", "{:?}", snapshot.error);
                    return snapshot;
                }
                updates.changed().await.unwrap();
            }
        })
        .await
        .unwrap()
    }
    async fn entry(&self, id: &str) -> caskade_core::model::GameEntry {
        self.store
            .entry_by_source("caskade-storage", id)
            .await
            .unwrap()
            .unwrap()
    }
}
fn manifest(dir: &Path) -> Value {
    serde_json::from_slice(&fs::read(dir.join(".caskade/install.json")).unwrap()).unwrap()
}

#[tokio::test]
async fn explorer_rename_then_app_scan_repairs_paths_and_preserves_library_history() {
    let fixture = Fixture::new().await;
    let (old, id) = fixture.game("Original", "rewritable", None);
    fixture.scan().await;
    let before = fixture.entry(&id).await;
    let mut played = PlaySession::new(&before.game_id, &before.id, 1000);
    fixture.store.start_session(&played).await.unwrap();
    played = fixture
        .store
        .finish_session(&played.id, 2500, 1500)
        .await
        .unwrap();
    let new = fixture.root.join("Renamed Café Game");
    fs::rename(&old, &new).unwrap(); // The app did not perform this move.
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(&new).unwrap().ino()
    };
    let report = fixture.scan().await;
    let game = &report.storage.as_ref().unwrap().roots[0].games[0];
    assert_eq!(game.status, StorageGameStatus::Reconciled, "{game:?}");
    assert!(game.question.is_none());
    let after = fixture.entry(&id).await;
    assert_eq!(after.id, before.id);
    assert_eq!(after.game_id, before.game_id);
    assert_eq!(
        after.location,
        Some(InstallLocation::Managed {
            root_id: fixture.root_id.clone(),
            rel_dir: "Renamed Café Game".into()
        })
    );
    assert_eq!(
        fixture.store.get_session(&played.id).await.unwrap(),
        Some(played)
    );
    assert_eq!(
        fs::read_to_string(new.join("config.ini")).unwrap(),
        format!("path={}\n", new.display())
    );
    assert_eq!(manifest(&new)["relocation"]["last_path"], json!(new));
    assert!(!old.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(&new).unwrap().ino(), inode);
    }
    let second = fixture.scan().await;
    assert_eq!(
        second.storage.unwrap().roots[0].games[0].status,
        StorageGameStatus::Unchanged
    );
    fixture.store.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn app_scan_retargets_configured_alias_after_an_external_rename() {
    let fixture = Fixture::new().await;
    let (old, id) = fixture.game("Alias original", "aliased", Some("app-scan-alias"));
    fs::write(old.join("config.ini"), b"relative=bin/play\n").unwrap();
    let aliases = fixture.paths.data.join("g");
    fs::create_dir_all(&aliases).unwrap();
    std::os::unix::fs::symlink(&old, aliases.join("app-scan-alias")).unwrap();
    fixture.scan().await;
    let new = fixture.root.join("Alias renamed");
    fs::rename(&old, &new).unwrap();
    let snapshot = fixture.scan().await;
    assert_eq!(
        snapshot.storage.unwrap().roots[0].games[0].status,
        StorageGameStatus::Reconciled
    );
    assert_eq!(
        fs::canonicalize(aliases.join("app-scan-alias")).unwrap(),
        new
    );
    assert_eq!(
        fixture.entry(&id).await.location,
        Some(InstallLocation::Managed {
            root_id: fixture.root_id.clone(),
            rel_dir: "Alias renamed".into()
        })
    );
    fixture.store.close().await;
}

#[tokio::test]
async fn fragile_external_move_exposes_typed_reinstall_question_without_rewriting_binary() {
    let fixture = Fixture::new().await;
    let (old, id) = fixture.game("Fragile original", "fragile", None);
    fixture.scan().await;
    let new = fixture.root.join("Fragile renamed");
    fs::rename(&old, &new).unwrap();
    let binary = fs::read(new.join("binary.dat")).unwrap();
    let snapshot = fixture.scan().await;
    let game = &snapshot.storage.as_ref().unwrap().roots[0].games[0];
    assert_eq!(game.status, StorageGameStatus::NeedsUser, "{game:?}");
    let question = serde_json::to_value(&game.question).unwrap();
    assert_eq!(question["type"], "reinstall_after_move", "{question}");
    assert!(!snapshot.warnings.is_empty());
    assert_eq!(fs::read(new.join("binary.dat")).unwrap(), binary);
    assert!(matches!(
        fixture.entry(&id).await.state,
        caskade_discovery::InstallState::Broken { .. }
    ));
    fixture.store.close().await;
}

#[tokio::test]
async fn offline_root_is_reported_and_does_not_remove_its_library_entry() {
    let fixture = Fixture::new().await;
    let (_old, id) = fixture.game("Game", "free", None);
    fixture.scan().await;
    let before = fixture.entry(&id).await;
    fs::rename(&fixture.root, fixture.root.with_extension("offline")).unwrap();
    let snapshot = fixture.scan().await;
    assert!(snapshot.storage.as_ref().unwrap().roots[0].error.is_some());
    assert!(!snapshot.warnings.is_empty());
    assert_eq!(fixture.entry(&id).await, before);
    fixture.store.close().await;
}
