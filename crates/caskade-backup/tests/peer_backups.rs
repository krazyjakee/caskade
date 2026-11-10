//! Backups kept on another of the user's devices (#243), between cores on
//! loopback: a backup made while the holder is away reaches it when it's
//! back, and a fresh install paired with the holder restores it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_backup::peers::PeerBackups;
use caskade_backup::{Backups, CancellationToken, ExportOptions, RestoreOptions, restore};
use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::model::Game;
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncService;
use serde_json::json;

struct Core {
    paths: AppPaths,
    store: LibraryStore,
    settings: SettingsService,
    devices: Devices,
    sync: SyncService,
    peers: PeerBackups,
    backups: Backups,
    destination: PathBuf,
    _temp: tempfile::TempDir,
}

impl Core {
    async fn id(&self) -> String {
        self.devices.status().await.this_device.id.unwrap()
    }

    async fn back_up(&self) -> PathBuf {
        self.backups
            .export(
                ExportOptions {
                    destination: self.destination.clone(),
                    components: Default::default(),
                    set_id: None,
                },
                Arc::new(|_| {}),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .path
    }

    async fn keep_backups_on(&self, device: &str) {
        let mut section = self.settings.get("core.backups").await.unwrap().value;
        section["keepOnDevice"] = json!(device);
        self.settings
            .set("core.backups", section, None)
            .await
            .unwrap();
    }
}

async fn core(name: &str) -> Core {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    paths.create().unwrap();
    let store = LibraryStore::open_default(&paths).await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    caskade_backup::register_settings(&settings).unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings.clone());
    devices.start();
    let destination = temp.path().join("my-backups");
    std::fs::create_dir_all(&destination).unwrap();
    let peers = PeerBackups::new(
        settings.clone(),
        paths.data.join("peer-backups"),
        &destination,
    );
    peers.set_devices(devices.clone());
    let sync = SyncService::new(store.clone(), devices.clone());
    sync.set_settings(settings.clone());
    sync.register_blobs(Arc::new(peers.clone()));
    let backups = Backups::new(store.clone(), paths.clone());
    let core = Core {
        paths,
        store,
        settings,
        devices,
        sync,
        peers,
        backups,
        destination,
        _temp: temp,
    };
    within("the endpoint starts", 20, || async {
        core.devices.status().await.this_device.listening
    })
    .await;
    core.devices.rename(name).await.unwrap();
    core
}

async fn pair(a: &Core, b: &Core) {
    let link = a.devices.pairing_link().await.unwrap();
    b.devices.accept_pairing_link(&link.url).await.unwrap();
    let b_id = b.id().await;
    within("the link pairs both", 20, || async {
        a.devices
            .status()
            .await
            .devices
            .iter()
            .any(|d| d.id == b_id)
    })
    .await;
}

async fn within<F, Fut>(what: &str, seconds: u64, check: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !check().await {
        assert!(Instant::now() < deadline, "{what} within {seconds} s");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_made_while_the_holder_is_away_reaches_it_and_restores_on_a_fresh_install() {
    let desktop = core("Desktop").await;
    let nas = core("NAS").await;
    pair(&desktop, &nas).await;
    let nas_id = nas.id().await;
    let desktop_id = desktop.id().await;
    desktop.keep_backups_on(&nas_id).await;
    desktop
        .store
        .upsert_game(&Game::new("Outer Wilds"))
        .await
        .unwrap();

    // Made while nothing syncs: the NAS hasn't got it.
    let made = desktop.back_up().await;
    assert!(nas.peers.holding().is_empty());

    // The NAS is back: the next sync carries it.
    let (_a, _b) = (desktop.sync.start(), nas.sync.start());
    desktop.sync.sync_now(&nas_id);
    within("the NAS keeps the desktop's backup", 30, || async {
        nas.peers
            .holding()
            .iter()
            .any(|held| held.origin == desktop_id)
    })
    .await;
    let held = nas.peers.holding();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].name, made.file_name().unwrap().to_string_lossy());
    assert_eq!(held[0].origin_name, "Desktop");
    let (hash, size) = caskade_core::saves::versions::hash_file(&made).unwrap();
    assert_eq!((held[0].hash.as_str(), held[0].size), (hash.as_str(), size));

    // The desktop's drive is gone: a fresh install pairs with the NAS, sees
    // what it holds, fetches the backup and restores it.
    let fresh = core("New desktop").await;
    pair(&nas, &fresh).await;
    let _c = fresh.sync.start();
    fresh.sync.sync_now(&nas_id);
    within("the fresh install hears what the NAS holds", 30, || async {
        fresh
            .peers
            .held()
            .get(&nas_id)
            .is_some_and(|held| held.iter().any(|backup| backup.hash == hash))
    })
    .await;
    // Asked for from here: the exchange this starts asks for it, even when
    // one already running began before it.
    let fetching = fresh.peers.fetch(&hash, Duration::from_secs(60));
    fresh.sync.sync_now(&nas_id);
    let archive = fetching.await.unwrap();
    assert_eq!(
        caskade_core::saves::versions::hash_file(&archive)
            .unwrap()
            .0,
        hash
    );

    fresh
        .backups
        .stage_restore(
            RestoreOptions {
                archive,
                components: Default::default(),
            },
            Arc::new(|_| {}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    fresh.devices.shutdown().await;
    fresh.store.close().await;
    restore::apply_pending(&fresh.paths).unwrap().unwrap();
    let restored = LibraryStore::open_default(&fresh.paths).await.unwrap();
    restore::finish(&fresh.paths).unwrap();
    let titles: Vec<String> = restored
        .list_games(&Default::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|game| game.title)
        .collect();
    assert!(titles.contains(&"Outer Wilds".to_owned()), "{titles:?}");
}
