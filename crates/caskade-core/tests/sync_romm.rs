//! RomM servers across devices (#227, #230): a server added on one device
//! appears on another without its password, which that device then asks for;
//! each device keeps its own emulator and profile for a server's platforms.

use crate::engines::exchange;
use std::sync::Arc;

use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::emulation::{Catalog, EmulationService};
use caskade_core::romm::{
    RommAuth, RommPlatformMapping, RommServerConfig, RommService, register_settings,
};
use caskade_core::secrets::MemorySecrets;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncEngine;

const PLATFORMS: &str = r#"
[[platform]]
id = "nintendo_snes"
name = "Super Nintendo Entertainment System"
short = "SNES"
extensions = ["sfc", "smc", "zip"]
ids = { igdb = 19, esde = "snes" }
"#;

struct Device {
    engine: SyncEngine,
    store: LibraryStore,
    romm: RommService,
    _secrets: Arc<MemorySecrets>,
    _temp: tempfile::TempDir,
}

async fn device(name: char) -> Device {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    caskade_core::accounts::AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    register_settings(&settings).unwrap();
    let emulation = EmulationService::new(
        store.clone(),
        Bus::default(),
        Catalog::load([("platforms.toml", PLATFORMS)]).unwrap(),
    );
    let secrets = Arc::new(MemorySecrets::default());
    let romm = RommService::new(
        store.clone(),
        Bus::default(),
        settings.clone(),
        emulation,
        temp.path().join("cache"),
    )
    .with_secrets(secrets.clone());
    let engine = SyncEngine::new(store.clone(), name.to_string().repeat(64));
    engine.set_settings(settings);
    Device {
        engine,
        store,
        romm,
        _secrets: secrets,
        _temp: temp,
    }
}

fn home(emulator: Option<&str>) -> RommServerConfig {
    RommServerConfig {
        auth: RommAuth::Password,
        username: "admin".into(),
        sync_on_startup: false,
        sync_interval: "manual".into(),
        platforms: vec![RommPlatformMapping {
            slug: "snes".into(),
            enabled: true,
            platform: Some("nintendo_snes".into()),
            emulator_id: emulator.map(str::to_owned),
            profile_id: emulator.map(|_| "default".to_owned()),
        }],
        ..RommServerConfig::new("home", "Home RomM", "https://romm.home.example")
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_romm_server_arrives_without_its_password_and_keeps_local_emulators() {
    let desk = device('a').await;
    let tv = device('b').await;
    desk.romm
        .save_server(home(Some("snes9x")), Some("hunter2".into()))
        .await
        .unwrap();
    exchange(&desk.engine, &tv.engine).await;

    // The server is there, its definition whole...
    let overview = tv.romm.overview().await.unwrap();
    let server = overview
        .settings
        .server("home")
        .expect("the server arrived");
    assert_eq!(server.name, "Home RomM");
    let desk_settings = desk.romm.overview().await.unwrap().settings;
    // The policy rebuilds a synced URL without credentials, query or fragment.
    assert_eq!(
        server.url.trim_end_matches('/'),
        desk_settings
            .server("home")
            .unwrap()
            .url
            .trim_end_matches('/')
    );
    assert_eq!(server.username, "admin");
    assert_eq!(
        server.platforms[0].platform.as_deref(),
        Some("nintendo_snes")
    );
    // ...without the password, which this device asks for...
    assert!(
        !overview.servers[0].has_secret,
        "the TV asks for its own password"
    );
    assert_eq!(
        overview.servers[0].state,
        caskade_core::romm::ConnectionState::SignInNeeded,
        "the TV says it needs signing in, not that it can't connect (#73)"
    );
    // ...and without the desk's emulator choice.
    assert_eq!(server.platforms[0].emulator_id, None);
    // Nothing in the log carries the secret.
    let snapshot = format!("{:?}", tv.store.sync_snapshot().await.unwrap().values);
    assert!(!snapshot.contains("hunter2"));
    for device in [&desk, &tv] {
        let changes = device
            .engine
            .changes_for(&Default::default(), 10_000)
            .await
            .unwrap();
        assert!(!format!("{changes:?}").contains("hunter2"));
    }

    // The TV picks its own emulator; the desk renames the server and changes
    // its emulator. The name travels, the emulators stay put.
    let mut mine = server.clone();
    mine.platforms[0].emulator_id = Some("bsnes".into());
    mine.platforms[0].profile_id = Some("default".into());
    tv.romm.save_server(mine, None).await.unwrap();
    let mut renamed = home(Some("mesen"));
    renamed.name = "Attic RomM".into();
    desk.romm.save_server(renamed, None).await.unwrap();
    exchange(&desk.engine, &tv.engine).await;
    let on_tv = tv.romm.overview().await.unwrap().settings;
    let on_tv = on_tv.server("home").unwrap();
    assert_eq!(on_tv.name, "Attic RomM");
    assert_eq!(on_tv.platforms[0].emulator_id.as_deref(), Some("bsnes"));
    let on_desk = desk.romm.overview().await.unwrap().settings;
    assert_eq!(
        on_desk.server("home").unwrap().platforms[0]
            .emulator_id
            .as_deref(),
        Some("mesen")
    );

    // Removed on one device, gone on the other, its ROMs removed there by
    // RomM's own removal.
    let removed = Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let removed = removed.clone();
        tv.engine
            .on_romm_removed(move |id| removed.lock().unwrap().push(id));
    }
    desk.romm.remove_server("home").await.unwrap();
    exchange(&desk.engine, &tv.engine).await;
    assert_eq!(*removed.lock().unwrap(), ["home"]);
    assert!(
        tv.romm
            .overview()
            .await
            .unwrap()
            .settings
            .server("home")
            .is_none()
    );
}
