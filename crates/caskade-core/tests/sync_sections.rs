//! Settings sections kept on one device (#244): a section made This device
//! only there neither sends nor takes in its changes, while the rest of its
//! category keeps syncing, and the UI scale never travels. Turned back on,
//! what waited applies and the section is sent again in full.

use crate::engines::exchange;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncEngine;
use caskade_core::sync::policy::Category;
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

async fn get(device: &Device, namespace: &str, field: &str) -> Value {
    device.settings.get(namespace).await.unwrap().value[field].clone()
}

async fn set(device: &Device, namespace: &str, field: &str, value: Value) {
    let mut section = device.settings.get(namespace).await.unwrap().value;
    section[field] = value;
    device.settings.set(namespace, section, None).await.unwrap();
}

/// Makes `namespace` This device only (or Sync) on `device`.
async fn keep_here(device: &Device, namespace: &str, here: bool) {
    let mut section = device.settings.get("core.sync").await.unwrap().value;
    section["sections"][namespace] = json!(if here { "thisDeviceOnly" } else { "sync" });
    device
        .settings
        .set("core.sync", section, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_section_kept_on_this_device_doesnt_change_on_the_other() {
    let desk = device('a').await;
    let deck = device('b').await;
    exchange(&desk.engine, &deck.engine).await;
    keep_here(&deck, "core.appearance", true).await;

    set(&desk, "core.appearance", "density", json!("compact")).await;
    set(&deck, "core.appearance", "cardSize", json!(240)).await;
    exchange(&desk.engine, &deck.engine).await;
    // Neither side's appearance reached the other.
    assert_eq!(
        get(&deck, "core.appearance", "density").await,
        "comfortable"
    );
    assert_eq!(get(&desk, "core.appearance", "cardSize").await, 176);

    // The rest still syncs: the rating board (another section), both ways.
    set(&desk, "core.accounts", "ratingBoard", json!("esrb")).await;
    exchange(&desk.engine, &deck.engine).await;
    assert_eq!(get(&deck, "core.accounts", "ratingBoard").await, "esrb");

    // Kept on the Desk instead: the Deck's rating board stays there.
    keep_here(&desk, "core.accounts", true).await;
    set(&deck, "core.accounts", "ratingBoard", json!("pegi")).await;
    exchange(&desk.engine, &deck.engine).await;
    assert_eq!(get(&desk, "core.accounts", "ratingBoard").await, "esrb");
}

#[tokio::test]
async fn turned_back_on_a_section_takes_what_waited_and_is_sent_again() {
    let desk = device('a').await;
    let deck = device('b').await;
    exchange(&desk.engine, &deck.engine).await;
    keep_here(&deck, "core.appearance", true).await;
    // The engine has seen the categories as they start.
    deck.engine.capture().await.unwrap();
    assert!(deck.engine.turned_on().await.is_empty());
    set(&desk, "core.appearance", "density", json!("compact")).await;
    exchange(&desk.engine, &deck.engine).await;
    assert_eq!(
        get(&deck, "core.appearance", "density").await,
        "comfortable"
    );

    keep_here(&deck, "core.appearance", false).await;
    // Its category counts as turned on again, so the exchange replays it.
    assert_eq!(deck.engine.turned_on().await, vec![Category::Appearance]);
    deck.engine.apply_pending().await.unwrap();
    assert_eq!(get(&deck, "core.appearance", "density").await, "compact");
}

#[tokio::test]
async fn the_ui_scale_suits_each_screen_and_never_travels() {
    let desk = device('a').await;
    let tv = device('b').await;
    set(&tv, "core.appearance", "uiScale", json!(125)).await;
    set(&tv, "core.appearance", "density", json!("compact")).await;
    exchange(&desk.engine, &tv.engine).await;
    assert_eq!(get(&desk, "core.appearance", "uiScale").await, 100);
    assert_eq!(get(&desk, "core.appearance", "density").await, "compact");
}
