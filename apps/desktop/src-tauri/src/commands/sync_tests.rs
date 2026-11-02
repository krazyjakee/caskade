//! Devices over typed IPC on the mock runtime (#226): pairing by link both
//! ways, the `caskade://pair` deep link waiting for an admin, and the admin
//! rule with the webview bypassed.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    db::LibraryStore,
    devices::{MemoryKeyStore, Reach},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    Listener,
    test::{mock_builder, mock_context, noop_assets},
};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tauri::async_runtime::block_on(future)
}

/// A second device, core only.
fn other_device() -> Devices {
    block_on(async {
        let store = LibraryStore::in_memory().await.unwrap();
        let accounts = AccountService::open(store.clone(), Bus::default())
            .await
            .unwrap();
        let devices = Devices::with_reach(
            store,
            accounts,
            Arc::new(MemoryKeyStore::default()),
            Reach::loopback(),
        );
        devices.start();
        devices
    })
}

fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn devices_pair_by_link_and_by_an_opened_link_over_ipc() {
    let store = block_on(LibraryStore::in_memory()).unwrap();
    let accounts = block_on(AccountService::open(store.clone(), Bus::default())).unwrap();
    let devices = Devices::with_reach(
        store,
        accounts.clone(),
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            devices_status,
            devices_rename,
            devices_set_kind,
            devices_pair_nearby,
            devices_confirm_pairing,
            devices_cancel_pairing,
            devices_pairing_link,
            devices_accept_pairing_link,
            devices_opened_link,
            devices_accept_opened_link,
            devices_dismiss_opened_link,
            devices_forget,
            devices_create_identity,
            devices_retry_identity,
        ])
        .events(tauri_specta::collect_events![
            DevicesChanged,
            PairingLinkOpened
        ]);
    let app = mock_builder()
        .manage(accounts.clone())
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let changes = Arc::new(Mutex::new(Vec::<Value>::new()));
    let opened = Arc::new(Mutex::new(Vec::<Value>::new()));
    for (name, seen) in [
        (DevicesChanged::NAME, changes.clone()),
        (PairingLinkOpened::NAME, opened.clone()),
    ] {
        app.listen(name, move |event| {
            seen.lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
    }
    setup(app.handle(), devices);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    until("the endpoint starts", || {
        invoke(&window, "devices_status", json!({})).unwrap()["thisDevice"]["listening"] == true
    });
    invoke(&window, "devices_rename", json!({"name": "Desk"})).unwrap();
    invoke(&window, "devices_set_kind", json!({"kind": "laptop"})).unwrap();
    let status = invoke(&window, "devices_status", json!({})).unwrap();
    assert_eq!(status["thisDevice"]["name"], "Desk");
    assert_eq!(status["thisDevice"]["kind"], "laptop");
    let own_id = status["thisDevice"]["id"].as_str().unwrap().to_owned();

    // A link minted here pairs the other device.
    let tv = other_device();
    until("the other device starts", || {
        block_on(tv.status()).this_device.listening
    });
    let link = invoke(&window, "devices_pairing_link", json!({})).unwrap();
    let url = link["url"].as_str().unwrap().to_owned();
    assert!(url.starts_with("caskade://pair?t="));
    block_on(tv.accept_pairing_link(&url)).unwrap();
    let tv_id = block_on(tv.status()).this_device.id.unwrap();
    until("the webview hears about the pairing", || {
        changes.lock().unwrap().iter().any(|status| {
            status["devices"]
                .as_array()
                .is_some_and(|d| d.iter().any(|d| d["id"] == json!(tv_id)))
        })
    });
    let listed = invoke(&window, "devices_status", json!({})).unwrap();
    let text = listed.to_string();
    assert!(
        !text.contains("grant"),
        "no grant reaches the webview: {text}"
    );

    // A caskade://pair link opened from outside waits for an admin.
    let phone = other_device();
    until("the phone starts", || {
        block_on(phone.status()).this_device.listening
    });
    let phone_link = block_on(phone.pairing_link()).unwrap().url;
    let phone_id = block_on(phone.status()).this_device.id.unwrap();
    let url = tauri::Url::parse(&phone_link).unwrap();
    assert!(is_pair_link(&url));
    assert!(!is_pair_link(
        &tauri::Url::parse("caskade://install?url=x").unwrap()
    ));
    assert!(hold_pair_link(app.handle(), &url));
    let announced = opened.lock().unwrap().clone();
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0]["deviceId"], json!(phone_id));
    assert!(
        !announced[0].to_string().contains("caskadepair"),
        "the link stays here"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        block_on(phone.status()).devices.is_empty(),
        "opening the link didn't pair"
    );
    let waiting = invoke(&window, "devices_opened_link", json!({})).unwrap();
    assert_eq!(waiting["deviceId"], json!(phone_id));
    let paired = invoke(&window, "devices_accept_opened_link", json!({})).unwrap();
    assert_eq!(paired["id"], json!(phone_id));
    assert_eq!(
        invoke(&window, "devices_opened_link", json!({})).unwrap(),
        Value::Null
    );
    until("the phone lists this device", || {
        block_on(phone.status())
            .devices
            .iter()
            .any(|d| d.id == own_id)
    });

    // Forgetting, over IPC.
    invoke(&window, "devices_forget", json!({"deviceId": phone_id})).unwrap();
    until("the phone was told", || {
        block_on(phone.status()).devices.is_empty()
    });

    // Only admins: with a standard account signed in, every change is refused.
    let admin = block_on(accounts.active()).unwrap().unwrap();
    block_on(accounts.set_pin(&admin.id, Some("2468".into()))).unwrap();
    let kid = block_on(accounts.create(caskade_core::accounts::NewAccount {
        name: "Kid".into(),
        avatar: caskade_core::accounts::DEFAULT_AVATAR.into(),
        access: caskade_core::accounts::AccountAccess {
            role: caskade_core::accounts::AccountRole::Standard,
            permissions: caskade_core::accounts::Permissions::all(),
            max_age: None,
            unrated_needs_pin: false,
        },
        pin: None,
    }))
    .unwrap();
    block_on(accounts.sign_in(&kid.id, None)).unwrap();
    for (command, body) in [
        ("devices_pairing_link", json!({})),
        (
            "devices_accept_pairing_link",
            json!({"link": "caskade://pair?t=x"}),
        ),
        ("devices_pair_nearby", json!({"deviceId": tv_id})),
        ("devices_confirm_pairing", json!({"deviceId": tv_id})),
        ("devices_forget", json!({"deviceId": tv_id})),
        ("devices_rename", json!({"name": "Mine"})),
        ("devices_create_identity", json!({})),
    ] {
        let error = invoke(&window, command, body).unwrap_err();
        assert_eq!(error["code"], "forbidden", "{command}: {error}");
    }
    let status = invoke(&window, "devices_status", json!({})).unwrap();
    assert_eq!(
        status["devices"].as_array().unwrap().len(),
        1,
        "still paired with the TV"
    );
}
