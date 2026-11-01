//! Accounts over typed IPC on the mock runtime: the rules hold with the
//! webview bypassed, collections follow the active account, and every
//! change reaches the webview as `account-changed`.
use super::*;
use crate::commands::{collections::*, test_ipc::invoke};
use caskade_core::{bus::Bus, db::LibraryStore};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    Listener,
    test::{mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;

#[test]
fn accounts_sign_in_change_and_own_their_collections_over_ipc() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let bus = Bus::default();
    let accounts =
        tauri::async_runtime::block_on(AccountService::open(store.clone(), bus.clone())).unwrap();
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            accounts_list,
            account_create,
            account_update,
            account_delete,
            account_sign_in,
            account_sign_out,
            account_set_pin,
            account_verify_admin_pin,
            list_collections,
            get_collection,
            collection_games,
            save_collection,
            delete_collection,
        ])
        .events(tauri_specta::collect_events![crate::events::AccountChanged]);
    let app = mock_builder()
        .manage(store)
        .manage(accounts)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    crate::events::bridge(app.handle().clone(), &bus);
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    {
        let seen = seen.clone();
        app.listen(crate::events::AccountChanged::NAME, move |event| {
            seen.lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
    }
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let mut errors = Vec::new();
    let mut fails = |command: &str, body: Value| {
        let error = invoke(&window, command, body).unwrap_err();
        errors.push(error.to_string());
    };

    // The migrated admin, signed in without a PIN.
    let list = invoke(&window, "accounts_list", json!({})).unwrap();
    let admin = list["accounts"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(list["activeId"], json!(admin));
    assert_eq!(list["accounts"][0]["role"], "admin");
    assert_eq!(list["accounts"][0]["hasPin"], false);

    let kid = json!({"account": {"name": "Kid", "avatar": "avatar:default", "pin": null,
        "access": {"role": "standard", "maxAge": 12, "unratedNeedsPin": true,
            "permissions": {"settings": ["appearance"], "install": false}}}});
    fails("account_create", kid.clone());
    let locked = invoke(
        &window,
        "account_set_pin",
        json!({"id": admin, "pin": "2468"}),
    )
    .unwrap();
    assert_eq!(locked["hasPin"], true);
    let kid = invoke(&window, "account_create", kid).unwrap();
    let kid_id = kid["id"].as_str().unwrap().to_owned();
    assert_eq!(
        kid["permissions"],
        json!({"settings": ["appearance"], "install": false})
    );
    assert_eq!(kid["maxAge"], 12);
    assert_eq!(kid["unratedNeedsPin"], true);
    // Settings → Accounts can't break the rules: the last admin stays an
    // admin, and keeps its PIN while a standard account exists.
    let demote = invoke(
        &window,
        "account_update",
        json!({"id": admin, "update": {"name": "Player", "avatar": "avatar:default",
            "access": {"role": "standard", "maxAge": null, "unratedNeedsPin": false,
                "permissions": {"settings": [], "install": false}}}}),
    )
    .unwrap_err()
    .to_string();
    assert!(demote.contains("last admin"), "{demote}");
    let unlock = invoke(
        &window,
        "account_set_pin",
        json!({"id": admin, "pin": null}),
    )
    .unwrap_err()
    .to_string();
    assert!(unlock.contains("every admin needs a PIN"), "{unlock}");

    let couch = json!({"collection": {"id": null, "name": "Couch", "description": "",
        "kind": {"type": "manual"}}});
    let mine = invoke(&window, "save_collection", couch.clone()).unwrap();
    let mine = mine["id"].as_str().unwrap().to_owned();

    // The kid signs in: the admin's collection is not found.
    assert_eq!(
        invoke(
            &window,
            "account_sign_in",
            json!({"id": kid_id, "pin": null})
        )
        .unwrap(),
        json!({"type": "matched"})
    );
    assert_eq!(
        invoke(&window, "get_collection", json!({"id": mine})).unwrap(),
        Value::Null
    );
    fails(
        "collection_games",
        json!({"id": mine, "offset": 0, "limit": 10}),
    );
    assert_eq!(
        invoke(&window, "delete_collection", json!({"id": mine})).unwrap(),
        json!(false)
    );
    assert_eq!(
        invoke(&window, "list_collections", json!({})).unwrap(),
        json!([])
    );
    // A collection of the same name is its own.
    invoke(&window, "save_collection", couch).unwrap();
    assert_eq!(
        invoke(&window, "list_collections", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // The kid can't change its access or anyone else.
    fails(
        "account_update",
        json!({"id": kid_id, "update": {"name": "Kid", "avatar": "avatar:default",
            "access": {"role": "admin", "maxAge": null, "unratedNeedsPin": false,
                "permissions": {"settings": [], "install": true}}}}),
    );
    fails("account_set_pin", json!({"id": admin, "pin": null}));
    fails("account_delete", json!({"id": admin}));
    let renamed = invoke(
        &window,
        "account_update",
        json!({"id": kid_id, "update": {"name": "Ace", "avatar": "avatar:default",
            "access": null}}),
    )
    .unwrap();
    assert_eq!(renamed["name"], "Ace");

    // The admin override answers only whether a PIN matched.
    assert_eq!(
        invoke(&window, "account_verify_admin_pin", json!({"pin": "0000"})).unwrap(),
        json!({"type": "wrong"})
    );
    assert_eq!(
        invoke(&window, "account_verify_admin_pin", json!({"pin": "2468"})).unwrap(),
        json!({"type": "matched"})
    );

    // Signed out, collections wait for an account.
    invoke(&window, "account_sign_out", json!({})).unwrap();
    fails("list_collections", json!({}));
    assert_eq!(
        invoke(
            &window,
            "account_sign_in",
            json!({"id": admin, "pin": "1111"})
        )
        .unwrap(),
        json!({"type": "wrong"})
    );
    assert_eq!(
        invoke(
            &window,
            "account_sign_in",
            json!({"id": admin, "pin": "2468"})
        )
        .unwrap(),
        json!({"type": "matched"})
    );
    let list = invoke(&window, "list_collections", json!({})).unwrap();
    assert_eq!(list[0]["id"], json!(mine));
    assert_eq!(
        invoke(&window, "account_delete", json!({"id": kid_id})).unwrap(),
        json!(true)
    );

    // Neither PIN nor hash ever reaches the webview.
    let replies = invoke(&window, "accounts_list", json!({}))
        .unwrap()
        .to_string();
    for text in errors.iter().chain([&replies]) {
        assert!(!text.contains("2468") && !text.contains("argon2"), "{text}");
    }

    for _ in 0..100 {
        if seen.lock().unwrap().last() == Some(&json!({"activeId": admin}))
            && seen.lock().unwrap().len() >= 7
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let seen = seen.lock().unwrap().clone();
    assert!(seen.contains(&json!({"activeId": kid_id})), "{seen:?}");
    assert!(seen.contains(&json!({"activeId": null})), "{seen:?}");
    assert_eq!(seen.last(), Some(&json!({"activeId": admin})));
}
