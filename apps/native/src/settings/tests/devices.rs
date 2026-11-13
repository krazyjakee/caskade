//! Settings → Devices (#105), over a real devices service on loopback
//! (as the flows' share tests run one), as the webview's `devices` tests:
//! this device renamed on Enter, its kind set; the connection switches
//! saved in `core.sync`; nobody paired yet, so Pair a device comes first
//! and What syncs waits behind its button; a pairing link made and shown;
//! the versions of saves to keep.

use std::sync::{Arc, Mutex};

use crate::data::use_data_source;
use crate::frame::tests::{ONE, Window, source};
use crate::input::InputRoot;
use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_ui_data::sync::DeviceSync;
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

static SYNC: Mutex<Option<DeviceSync>> = Mutex::new(None);

fn app() -> Element {
    use_data_source(|| source().clone());
    let sync = SYNC.lock().unwrap().clone().expect("devices");
    use_context_provider(move || sync);
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            crate::settings::devices::DevicesSettings {}
        }
    }
}

/// A device on loopback named Desk, its endpoint started.
fn desk() -> (Devices, AccountService, tempfile::TempDir) {
    caskade_app::runtime::block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let store = LibraryStore::in_memory().await.unwrap();
        let accounts = AccountService::open(store.clone(), Bus::default())
            .await
            .unwrap();
        let settings = SettingsService::new(store.clone());
        settings
            .register_core(&AppPaths::under(&dir.path().join("app")))
            .unwrap();
        let devices = Devices::with_reach(
            store,
            accounts.clone(),
            Arc::new(MemoryKeyStore::default()),
            Reach::loopback(),
        );
        devices.set_settings(settings);
        devices.start();
        let until = Instant::now() + Duration::from_secs(20);
        while !devices.status().await.this_device.listening {
            assert!(Instant::now() < until, "the endpoint starts");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        devices.rename("Desk").await.unwrap();
        (devices, accounts, dir)
    })
}

fn saved(namespace: &str) -> Value {
    caskade_app::runtime::block_on(source().settings.get(namespace))
        .unwrap()
        .value
}

struct Restore(&'static str, Value);

impl Drop for Restore {
    fn drop(&mut self) {
        let value = self.1.clone();
        let _ = caskade_app::runtime::block_on(source().settings.update(self.0, move |_| value));
    }
}

fn press(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

fn row_switch(window: &Window, label: &str) -> blitz_dom::NodeId {
    let row = window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| super::label_of(window, row) == label)
        .unwrap_or_else(|| panic!("no row {label}"));
    window
        .find_all("role", "switch")
        .into_iter()
        .find(|&switch| super::is_inside(window, switch, row))
        .unwrap_or_else(|| panic!("no switch in {label}"))
}

#[test]
fn this_device_is_renamed_and_its_connections_switch() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore("core.sync", saved("core.sync"));
    let (devices, accounts, _dir) = desk();
    *SYNC.lock().unwrap() = Some(DeviceSync::new(devices.clone(), None, None, accounts));
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 2400, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("this device", |window| {
        window
            .find("id", "device-name")
            .and_then(|node| window.attr(node, "value"))
            .as_deref()
            == Some("Desk")
    });
    // Nobody paired: Pair a device first, and What syncs behind a button.
    let texts = window.text(window.get("data-cs", "settings-section"));
    assert!(texts.starts_with("This device"), "{texts}");
    assert!(window.find("id", "sync-choose").is_some());
    // Renamed on Enter, and it says so.
    let name = window.get("id", "device-name");
    window.focus(name);
    window.key_with(
        Key::Character("a".into()),
        dioxus_native::prelude::keyboard_types::Modifiers::CONTROL,
    );
    window.key(Key::Named(NamedKey::Backspace));
    window.type_text("Den");
    window.key(Key::Named(NamedKey::Enter));
    let renamed = devices.clone();
    window.settle_until("renamed", |_| {
        caskade_app::runtime::block_on(renamed.status())
            .this_device
            .name
            == "Den"
    });
    // A connection switch saves core.sync.
    let switch = row_switch(&window, "Play requests from my devices");
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("saved", |_| {
        saved("core.sync")["remotePlay"] == json!(false)
    });
    // What syncs opens, and a choice saves.
    let choose = window.get("id", "sync-choose");
    press(&mut window, choose);
    window.settle_until("the choices", |window| {
        window.find("data-category", "gameScripts").is_some()
    });
    // A pairing link is made and shown.
    let copy = window.get("id", "pairing-link-copy");
    press(&mut window, copy);
    window.settle_until("the link", |window| {
        window
            .find("data-part", "pairing-link")
            .is_some_and(|node| window.text(node).starts_with("caskade://pair"))
    });
}
