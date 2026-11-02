//! Add-ons from the user's other devices (#245): with Plugins and themes
//! syncing, each device offers what another has and it lacks, and nothing
//! installs or turns on by itself.

use caskade_core::bus::Bus;
use caskade_core::db::{LibraryStore, PluginRow};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncEngine;
use caskade_core::sync::offers::{AddonOffer, addon_offers};
use serde_json::json;

struct Device {
    engine: SyncEngine,
    store: LibraryStore,
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
    let engine = SyncEngine::new(store.clone(), name.to_string().repeat(64));
    engine.set_settings(settings.clone());
    Device {
        engine,
        store,
        settings,
        _temp: temp,
    }
}

async fn addons(device: &Device, state: &str) {
    let mut section = device.settings.get("core.sync").await.unwrap().value;
    section["categories"]["addons"] = json!(state);
    device
        .settings
        .set("core.sync", section, None)
        .await
        .unwrap();
}

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

fn plugin(id: &str) -> PluginRow {
    PluginRow {
        id: id.into(),
        enabled: true,
        grants: json!({}),
    }
}

#[tokio::test]
async fn turning_plugins_and_themes_on_offers_the_other_devices_add_ons_and_installs_nothing() {
    let desk = device('a').await;
    let deck = device('b').await;
    // The Desk has two plugins and a theme from the catalogue; the Deck
    // already has one of the plugins.
    for id in ["dev.example.stats", "dev.example.gamemode"] {
        desk.store.upsert_plugin_state(&plugin(id)).await.unwrap();
    }
    desk.store
        .set_setting(
            "addons.records",
            &json!({ "addons": { "caskade.ember": { "version": "1.2.0" } } }),
        )
        .await
        .unwrap();
    deck.store
        .upsert_plugin_state(&plugin("dev.example.stats"))
        .await
        .unwrap();

    // Off (the default): nothing offered.
    sync(&desk, &deck).await;
    assert!(addon_offers(&deck.engine).await.unwrap().is_empty());

    for device in [&desk, &deck] {
        addons(device, "sync").await;
    }
    sync(&desk, &deck).await;
    let desk_id = "a".repeat(64);
    assert_eq!(
        addon_offers(&deck.engine).await.unwrap(),
        [
            AddonOffer {
                device_id: desk_id.clone(),
                addon_id: "caskade.ember".into(),
                version: Some("1.2.0".into()),
            },
            AddonOffer {
                device_id: desk_id.clone(),
                addon_id: "dev.example.gamemode".into(),
                version: None,
            },
        ]
    );
    // The Desk is offered nothing it already has; the Deck's own plugin is
    // one of them.
    assert!(addon_offers(&desk.engine).await.unwrap().is_empty());
    // Nothing installed or turned on by itself.
    let plugins = deck.store.plugin_states().await.unwrap();
    assert_eq!(
        plugins
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["dev.example.stats"]
    );
    assert!(
        deck.store
            .get_setting("addons.records")
            .await
            .unwrap()
            .is_none()
    );

    // Turned off again on the Deck: no offers.
    addons(&deck, "thisDeviceOnly").await;
    assert!(addon_offers(&deck.engine).await.unwrap().is_empty());
}
