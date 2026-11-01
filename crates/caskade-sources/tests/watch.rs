//! Real OS file events over a private authored machine, with async DB imports.
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    model::{InstallState, ListGames},
};
use caskade_discovery::{
    Config, DetectedGame, HostEnv, LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId,
    WatchPath, watch::WatchOptions,
};
use caskade_sources::{DiscoveryService, ImportOptions};
use std::{fs, sync::Arc, time::Duration};

struct FileSource;
impl Source for FileSource {
    fn id(&self) -> SourceId {
        SourceId::new("fixture.watch")
    }
    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn scan(
        &self,
        env: &HostEnv,
        ctx: &ScanCtx,
    ) -> std::result::Result<Vec<DetectedGame>, SourceError> {
        ctx.check()?;
        let path = env
            .resolve_path(std::path::Path::new("/games/title.txt"))
            .unwrap();
        if !path.exists() {
            return Ok(vec![]);
        }
        let title = fs::read_to_string(&path).map_err(|e| SourceError::io(&path, e))?;
        let mut game = DetectedGame::new(self.id(), "stable", title);
        game.install_dir = Some(env.resolve_path(std::path::Path::new("/games")).unwrap());
        Ok(vec![game])
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        vec![WatchPath {
            path: env.resolve_path(std::path::Path::new("/games")).unwrap(),
            recursive: false,
        }]
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_creation_update_removal_and_shutdown_reach_the_library() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("root/games")).unwrap();
    fs::write(
        temp.path().join("machine.toml"),
        "schema_version = 1\nos = \"linux\"\n",
    )
    .unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let service = DiscoveryService::with_sources(
        store.clone(),
        Bus::default(),
        env,
        vec![Arc::new(FileSource)],
    );
    let mut watcher = service
        .watch(
            Config::default(),
            ImportOptions::default(),
            WatchOptions {
                debounce: Duration::from_millis(50),
                max_delay: Duration::from_millis(200),
                poll_interval: Duration::from_secs(30),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let file = temp.path().join("root/games/title.txt");
    fs::write(&file, "Synthetic watched game").unwrap();
    tokio::time::timeout(Duration::from_secs(10), watcher.status.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(
        watcher
            .status
            .borrow_and_update()
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .warnings
            .is_empty()
    );
    let page = store.list_games(&ListGames::default()).await.unwrap();
    assert_eq!(page.total, 1);
    let game_id = page.items[0].id.clone();
    let entry_id = store.entries_for_game(&game_id).await.unwrap()[0]
        .id
        .clone();
    fs::write(&file, "Updated source title").unwrap();
    tokio::time::timeout(Duration::from_secs(10), watcher.status.changed())
        .await
        .unwrap()
        .unwrap();
    watcher.status.borrow_and_update();
    assert_eq!(
        store.entries_for_game(&game_id).await.unwrap()[0].id,
        entry_id
    );
    assert_eq!(store.library_stats().await.unwrap().total, 1);
    fs::remove_file(&file).unwrap();
    tokio::time::timeout(Duration::from_secs(10), watcher.status.changed())
        .await
        .unwrap()
        .unwrap();
    let entry = store.get_entry(&entry_id).await.unwrap().unwrap();
    assert_eq!(entry.state, InstallState::Partial);
    assert_eq!(store.library_stats().await.unwrap().installed, 0);
    watcher.stop().await;
}
