//! Emulator configuration between devices (#275), two cores on loopback: a
//! RetroArch controller remap made on the Desk (an AppImage, its config in
//! `~/.config/retroarch/config`) arrives on the Deck (the Flatpak, its config
//! inside the sandbox) once Emulator configuration syncs, in the Deck's own
//! folder; one changed later on the Deck goes back; and nothing travels
//! while the category is off.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::emulation::{
    Catalog, Emulator, EmulatorInstall, EmulatorProfile, OperatingSystem, OsFolders,
};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncService;
use caskade_core::sync::config_files::EmulatorConfig;
use serde_json::json;

const REMAP: &str = "remaps/Snes9x/Chrono Trigger.rmp";

struct Core {
    settings: SettingsService,
    devices: Devices,
    sync: SyncService,
    home: PathBuf,
    _dir: tempfile::TempDir,
}

impl Core {
    async fn id(&self) -> String {
        self.devices.status().await.this_device.id.unwrap()
    }

    async fn emulator_config(&self, on: bool) {
        let mut section = self.settings.get("core.sync").await.unwrap().value;
        section["categories"]["emulatorConfig"] = json!(if on { "sync" } else { "thisDeviceOnly" });
        self.settings.set("core.sync", section, None).await.unwrap();
    }

    /// Syncs with `other` and waits for the exchange to finish.
    async fn sync_with(&self, other: &Core) {
        let id = other.id().await;
        let before = self.last_sync(&id).await;
        self.sync.sync_now(&id);
        within("an exchange", 30, || async {
            self.last_sync(&id)
                .await
                .is_some_and(|at| Some(at) != before)
        })
        .await;
    }

    async fn last_sync(&self, id: &str) -> Option<i64> {
        self.devices
            .status()
            .await
            .devices
            .into_iter()
            .find(|device| device.id == id && !device.syncing)
            .and_then(|device| device.last_sync_ms)
    }
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

fn catalog() -> Arc<Catalog> {
    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../caskade-sources/emulation");
    let read = |name: &str| std::fs::read_to_string(sources.join(name)).unwrap();
    let (platforms, retroarch) = (read("platforms.toml"), read("retroarch.toml"));
    Arc::new(
        Catalog::load([
            ("platforms.toml", platforms.as_str()),
            ("retroarch.toml", retroarch.as_str()),
        ])
        .unwrap(),
    )
}

async fn core(name: &str, install: impl Fn(&Path) -> EmulatorInstall) -> Core {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    let paths = AppPaths::under(&dir.path().join("app"));
    settings.register_core(&paths).unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings.clone());
    devices.start();
    store
        .upsert_emulator(&Emulator {
            id: format!("{}-retroarch", name.to_lowercase()),
            name: "RetroArch".into(),
            definition: Some("retroarch".into()),
            install: install(&home),
            profiles: vec![EmulatorProfile::BuiltIn {
                id: "snes".into(),
                definition_profile: "Snes9x".into(),
                args: Default::default(),
                hooks: Default::default(),
            }],
        })
        .await
        .unwrap();
    let config = EmulatorConfig::with_folders(
        store.clone(),
        catalog(),
        &paths.data,
        OperatingSystem::Linux,
        OsFolders {
            user_home: Some(home.clone()),
            xdg_config: Some(home.join(".config")),
            ..Default::default()
        },
    );
    let sync = SyncService::new(store.clone(), devices.clone());
    sync.set_settings(settings.clone());
    sync.register_blobs(Arc::new(config));
    let core = Core {
        settings,
        devices,
        sync,
        home,
        _dir: dir,
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

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retroarch_remap_reaches_the_other_device_in_its_own_config_folder() {
    // The Desk runs the AppImage; the Deck the Flatpak.
    let desk = core("Desk", |home| EmulatorInstall::AppImage {
        file: home.join("Applications/RetroArch.AppImage"),
    })
    .await;
    let deck = core("Deck", |_| EmulatorInstall::Flatpak {
        app_id: "org.libretro.RetroArch".into(),
    })
    .await;
    let desk_remap = desk.home.join(".config/retroarch/config").join(REMAP);
    let deck_remap = deck
        .home
        .join(".var/app/org.libretro.RetroArch/config/retroarch/config")
        .join(REMAP);
    pair(&desk, &deck).await;
    let (_a, _b) = (desk.sync.start(), deck.sync.start());

    // Off by default: the remap stays on the Desk.
    write(&desk_remap, "input_player1_btn_a = \"8\"\n");
    deck.sync_with(&desk).await;
    deck.sync_with(&desk).await;
    assert!(!deck_remap.exists(), "nothing travels while it's off");

    // Turned on on both: it arrives in the Deck's own folder.
    desk.emulator_config(true).await;
    deck.emulator_config(true).await;
    within("the remap arrives on the Deck", 30, || async {
        deck.sync_with(&desk).await;
        std::fs::read_to_string(&deck_remap).ok().as_deref()
            == Some("input_player1_btn_a = \"8\"\n")
    })
    .await;

    // Changed later on the Deck: the Desk gets the change.
    tokio::time::sleep(Duration::from_millis(20)).await;
    write(&deck_remap, "input_player1_btn_a = \"0\"\n");
    within("the change goes back to the Desk", 30, || async {
        desk.sync_with(&deck).await;
        std::fs::read_to_string(&desk_remap).ok().as_deref()
            == Some("input_player1_btn_a = \"0\"\n")
    })
    .await;
    // And stays: the Desk's older copy doesn't come back.
    deck.sync_with(&desk).await;
    assert_eq!(
        std::fs::read_to_string(&deck_remap).unwrap(),
        "input_player1_btn_a = \"0\"\n"
    );
}
