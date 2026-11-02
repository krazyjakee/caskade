//! First settings by kind of device (#244): a new handheld or TV starts in
//! console mode with emulators fullscreen and a larger interface; a desktop
//! keeps the defaults, and an existing library or a section already saved
//! is never changed.

use caskade_core::db::LibraryStore;
use caskade_core::device_type::{DeviceType, settle_new_device};
use caskade_core::model::Game;
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use serde_json::{Value, json};

async fn fresh() -> (LibraryStore, SettingsService, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(temp.path()))
        .unwrap();
    (store, settings, temp)
}

async fn get(settings: &SettingsService, namespace: &str, field: &str) -> Value {
    settings.get(namespace).await.unwrap().value[field].clone()
}

#[tokio::test]
async fn a_new_handheld_starts_in_console_mode_and_a_desktop_doesnt() {
    let (store, settings, _temp) = fresh().await;
    let written = settle_new_device(&settings, &store, DeviceType::Handheld)
        .await
        .unwrap();
    assert_eq!(
        written,
        ["core.console", "core.emulation", "core.appearance"]
    );
    assert_eq!(get(&settings, "core.console", "active").await, true);
    assert_eq!(get(&settings, "core.console", "start").await, "auto");
    assert_eq!(
        get(&settings, "core.emulation", "desktopFullscreen").await,
        true
    );
    assert_eq!(get(&settings, "core.appearance", "uiScale").await, 115);
    // Once only: the user's next choice stays.
    let mut console = settings.get("core.console").await.unwrap().value;
    console["active"] = json!(false);
    settings.set("core.console", console, None).await.unwrap();
    assert!(
        settle_new_device(&settings, &store, DeviceType::Handheld)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(get(&settings, "core.console", "active").await, false);

    let (store, settings, _temp) = fresh().await;
    assert!(
        settle_new_device(&settings, &store, DeviceType::Desktop)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(get(&settings, "core.console", "active").await, false);
    assert_eq!(get(&settings, "core.appearance", "uiScale").await, 100);
}

#[tokio::test]
async fn an_existing_library_keeps_its_settings() {
    // Games already here: not a new device.
    let (store, settings, _temp) = fresh().await;
    store.upsert_game(&Game::new("Hades")).await.unwrap();
    assert!(
        settle_new_device(&settings, &store, DeviceType::Tv)
            .await
            .unwrap()
            .is_empty()
    );
    // First-run setup saved: not new either.
    let (store, settings, _temp) = fresh().await;
    let mut setup = settings.get("core.onboarding").await.unwrap().value;
    setup["completed"] = json!(true);
    settings.set("core.onboarding", setup, None).await.unwrap();
    assert!(
        settle_new_device(&settings, &store, DeviceType::Tv)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(get(&settings, "core.console", "active").await, false);
}
