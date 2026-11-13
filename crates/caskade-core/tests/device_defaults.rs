//! First settings by kind of device (#244): a new handheld or TV starts in
//! console mode with emulators fullscreen and a larger interface; a desktop
//! keeps the defaults, and an existing library or a section already saved
//! is never changed.

use caskade_core::db::LibraryStore;
use caskade_core::device_type::{DeviceKind, detect_windows, settle_new_device};
use caskade_core::model::Game;
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_discovery::registry::{
    MemoryRegistry, RegistryHive, RegistryKey, RegistryValue, RegistryView,
};
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

fn hklm(path: &str) -> RegistryKey {
    RegistryKey::new(RegistryHive::LocalMachine, RegistryView::Native, path)
}

/// A Windows registry with these firmware values.
fn windows(values: &[(&str, &str)]) -> MemoryRegistry {
    let mut registry = MemoryRegistry::new();
    for (name, text) in values {
        registry.insert(
            hklm(r"HARDWARE\DESCRIPTION\System\BIOS"),
            *name,
            RegistryValue::String((*text).into()),
        );
    }
    registry
}

fn with_battery(mut registry: MemoryRegistry, present: bool) -> MemoryRegistry {
    let battery = r"SYSTEM\CurrentControlSet\Enum\ACPI\PNP0C0A\1";
    registry.insert_key(hklm(battery));
    if present {
        registry.insert_key(hklm(&format!(r"{battery}\Control")));
    }
    registry
}

#[tokio::test]
async fn a_new_windows_handheld_starts_in_console_mode_and_a_windows_desktop_doesnt() {
    let ally = windows(&[("SystemProductName", "ROG Ally RC71L_RC71L")]);
    let (store, settings, _temp) = fresh().await;
    settle_new_device(&settings, &store, detect_windows(&ally))
        .await
        .unwrap();
    assert_eq!(get(&settings, "core.console", "active").await, true);
    assert_eq!(
        get(&settings, "core.emulation", "desktopFullscreen").await,
        true
    );
    assert_eq!(get(&settings, "core.appearance", "uiScale").await, 115);

    let desktop = windows(&[("SystemProductName", "MS-7D25")]);
    let (store, settings, _temp) = fresh().await;
    assert!(
        settle_new_device(&settings, &store, detect_windows(&desktop))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(get(&settings, "core.console", "active").await, false);
    assert_eq!(get(&settings, "core.appearance", "uiScale").await, 100);
}

#[test]
fn windows_machines_are_told_apart_by_firmware_names_and_battery() {
    for values in [
        &[("SystemProductName", "Jupiter")][..],
        &[("SystemProductName", "ROG Ally X RC72LA_RC72LA")],
        &[("SystemProductName", "Claw A1M")],
        &[("SystemProductName", "AYANEO 2S")],
        // A Legion Go's product is a code; a newer one's is known only by family.
        &[("SystemProductName", "83E1")],
        &[
            ("SystemProductName", "83N0"),
            ("SystemFamily", "Legion Go 8AHP2"),
        ],
    ] {
        assert_eq!(
            detect_windows(&windows(values)),
            DeviceKind::Handheld,
            "{values:?}"
        );
    }

    // HP names its boards in hex; a matching one is no Legion Go.
    let desktop = windows(&[
        ("SystemProductName", "HP Pavilion Desktop"),
        ("BaseBoardProduct", "83E1"),
    ]);
    assert_eq!(detect_windows(&desktop), DeviceKind::Desktop);
    // Only a battery that is here now counts.
    assert_eq!(
        detect_windows(&with_battery(desktop.clone(), false)),
        DeviceKind::Desktop
    );
    assert_eq!(
        detect_windows(&with_battery(desktop, true)),
        DeviceKind::Laptop
    );
    assert_eq!(
        detect_windows(&with_battery(
            windows(&[("SystemProductName", "83E1")]),
            true
        )),
        DeviceKind::Handheld
    );
    // Nothing readable at all: a desktop.
    assert_eq!(detect_windows(&MemoryRegistry::new()), DeviceKind::Desktop);
}

/// The real registry, on CI's `windows-elevation` job: Windows runs the reader
/// that the tests above give a fake. The runner is a virtual machine.
#[cfg(windows)]
#[test]
fn windows_this_machine_names_itself_and_is_no_handheld() {
    use caskade_core::device_type::{detect, windows_battery, windows_names};
    let registry = caskade_discovery::registry::NativeRegistry::default();
    let names = windows_names(&registry);
    println!("names {names:?}, battery {}", windows_battery(&registry));
    println!("detected {:?}", detect());
    assert!(!names.is_empty(), "the firmware's names didn't read");
    assert_ne!(detect(), DeviceKind::Handheld);
}

#[tokio::test]
async fn a_new_handheld_starts_in_console_mode_and_a_desktop_doesnt() {
    let (store, settings, _temp) = fresh().await;
    let written = settle_new_device(&settings, &store, DeviceKind::Handheld)
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
        settle_new_device(&settings, &store, DeviceKind::Handheld)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(get(&settings, "core.console", "active").await, false);

    let (store, settings, _temp) = fresh().await;
    assert!(
        settle_new_device(&settings, &store, DeviceKind::Desktop)
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
        settle_new_device(&settings, &store, DeviceKind::Tv)
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
        settle_new_device(&settings, &store, DeviceKind::Tv)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(get(&settings, "core.console", "active").await, false);
}
