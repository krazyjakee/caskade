//! Two or more cores in one process (#250), each with its own library,
//! settings, device key and storage, connected on loopback (or through a
//! local relay, `core_with`): what the devices, sync, copy, send and
//! play-there tests start from. Nothing here reaches the internet.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::AccountService;
use caskade_core::artwork::ArtworkCache;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::install::{InstallOptions, InstallService};
use caskade_core::model::{ExternalId, Game, GameEntry, StorageRoot};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncService;
use caskade_core::transfer::Transfers;
use caskade_installer::manifest::RootManifest;
use caskade_installer::pipeline::input;

/// One device: its library, its device service started, copies and an
/// install service with a storage root of its own.
pub struct Core {
    pub store: LibraryStore,
    pub artwork: ArtworkCache,
    pub devices: Devices,
    pub settings: SettingsService,
    /// The signed-in account (the default admin).
    pub account: String,
    pub transfers: Transfers,
    pub install: InstallService,
    /// The storage root installs land in.
    pub games: PathBuf,
    pub dir: tempfile::TempDir,
}

impl Core {
    /// This device's id.
    pub async fn id(&self) -> String {
        self.devices.status().await.this_device.id.unwrap()
    }

    /// Its library sync, started.
    pub fn sync(&self) -> SyncService {
        let service = SyncService::new(self.store.clone(), self.devices.clone());
        service.set_settings(self.settings.clone());
        service.set_artwork_cache(self.artwork.clone());
        service.start();
        service
    }
}

/// A device on loopback named `name`, with Hades (Steam) discovered.
pub async fn core(name: &str) -> Core {
    core_with(name, Reach::loopback()).await
}

/// A device named `name` reached as `reach` says (loopback, a local relay),
/// with Hades (Steam) discovered.
pub async fn core_with(name: &str, reach: Reach) -> Core {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let account = accounts.active_id().unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(&dir.path().join("app")))
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        reach,
    );
    devices.set_settings(settings.clone());
    // Served with the endpoint: set up before it starts.
    let transfers = Transfers::new(store.clone(), devices.clone(), &dir.path().join("cache"))
        .await
        .unwrap();
    devices.start();
    let games = dir.path().join("Games");
    std::fs::create_dir_all(&games).unwrap();
    let identity = RootManifest::new(format!("{}-root", name.to_lowercase())).unwrap();
    identity.write(&games).unwrap();
    let games = games.canonicalize().unwrap();
    let mut root = StorageRoot::from_manifest(games.clone(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let install = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    )
    .with_settings(settings.clone())
    .with_host(input::host(Some("linux")).unwrap());
    install.set_peer_fetcher(Arc::new(transfers.clone()));
    let core = Core {
        artwork: ArtworkCache::new(dir.path().join("artwork")).unwrap(),
        store,
        devices,
        settings,
        account,
        transfers,
        install,
        games,
        dir,
    };
    within("the endpoint starts", 20, || async {
        core.devices.status().await.this_device.listening
    })
    .await;
    core.devices.rename(name).await.unwrap();
    // The same game, discovered on each.
    let game = core.store.upsert_game(&Game::new("Hades")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), "1145360");
    entry.ids = vec![ExternalId::Steam { app_id: 1_145_360 }];
    core.store.upsert_entry(&entry).await.unwrap();
    core
}

/// Pairs `a` and `b` with a pairing link `a` makes.
pub async fn pair(a: &Core, b: &Core) {
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

/// Waits up to `seconds` for `check`.
pub async fn within<F, Fut>(what: &str, seconds: u64, check: F)
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
