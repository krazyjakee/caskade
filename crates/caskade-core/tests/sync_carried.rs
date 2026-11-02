//! The rest of what policy declares synced (#273), between two devices'
//! engines: appearance and the rating board as fields changed from their
//! defaults.

use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncEngine;
use serde_json::{Value, json};

struct Device {
    engine: SyncEngine,
    settings: SettingsService,
    _temp: tempfile::TempDir,
}

async fn device(name: char) -> Device {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    caskade_core::accounts::AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(temp.path()))
        .unwrap();
    let engine = SyncEngine::new(store, name.to_string().repeat(64));
    engine.set_settings(settings.clone());
    Device {
        engine,
        settings,
        _temp: temp,
    }
}

/// Both ways, until neither has anything new.
async fn sync(a: &Device, b: &Device) {
    for _ in 0..2 {
        for (from, to) in [(a, b), (b, a)] {
            from.engine.capture().await.unwrap();
            loop {
                let cursor = to.engine.seen().await.unwrap();
                let changes = from.engine.changes_for(&cursor, 100).await.unwrap();
                if changes.is_empty() {
                    break;
                }
                to.engine.receive(&changes).await.unwrap();
            }
        }
    }
}

async fn get(device: &Device, namespace: &str, field: &str) -> Value {
    device.settings.get(namespace).await.unwrap().value[field].clone()
}

async fn set(device: &Device, namespace: &str, field: &str, value: Value) {
    let mut section = device.settings.get(namespace).await.unwrap().value;
    section[field] = value;
    device.settings.set(namespace, section, None).await.unwrap();
}

async fn set_category(device: &Device, category: &str, state: &str) {
    let mut section = device.settings.get("core.sync").await.unwrap().value;
    section["categories"][category] = json!(state);
    device
        .settings
        .set("core.sync", section, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn appearance_and_the_rating_board_follow_the_user_and_defaults_never_win() {
    let desk = device('a').await;
    set(&desk, "core.appearance", "density", json!("compact")).await;
    set(&desk, "core.appearance", "accent", json!("#ff8800")).await;
    set(&desk, "core.accounts", "ratingBoard", json!("esrb")).await;
    // A new device, set up after the Desk's choices: its defaults are newer,
    // and still don't replace them.
    let laptop = device('b').await;
    sync(&desk, &laptop).await;
    for device in [&desk, &laptop] {
        assert_eq!(get(device, "core.appearance", "density").await, "compact");
        assert_eq!(get(device, "core.appearance", "accent").await, "#ff8800");
        assert_eq!(get(device, "core.accounts", "ratingBoard").await, "esrb");
        // Fields nobody changed stay as they were.
        assert_eq!(get(device, "core.appearance", "cardSize").await, 176);
    }

    // Set back to its default on the Laptop: back to the default everywhere.
    set(&laptop, "core.appearance", "density", json!("comfortable")).await;
    sync(&desk, &laptop).await;
    assert_eq!(
        get(&desk, "core.appearance", "density").await,
        "comfortable"
    );
    assert_eq!(get(&desk, "core.appearance", "accent").await, "#ff8800");
}

#[tokio::test]
async fn appearance_set_to_this_device_only_stays_here_until_turned_back_on() {
    let desk = device('a').await;
    let laptop = device('b').await;
    sync(&desk, &laptop).await;
    set_category(&laptop, "appearance", "thisDeviceOnly").await;
    set(&desk, "core.appearance", "density", json!("compact")).await;
    set(&laptop, "core.appearance", "cardSize", json!(240)).await;
    sync(&desk, &laptop).await;
    assert_eq!(
        get(&laptop, "core.appearance", "density").await,
        "comfortable"
    );
    assert_eq!(get(&desk, "core.appearance", "cardSize").await, 176);

    // Back on: the Desk's choice, waiting here, applies. (Sending the
    // Laptop's own again is the exchange's replay, `sync_network`.)
    set_category(&laptop, "appearance", "sync").await;
    laptop.engine.apply_pending().await.unwrap();
    assert_eq!(get(&laptop, "core.appearance", "density").await, "compact");
}
