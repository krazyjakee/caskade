//! Accounts over typed IPC on the mock runtime: the rules hold with the
//! webview bypassed, collections follow the active account, and every
//! change reaches the webview as `account-changed`.
use super::*;
use crate::commands::{
    collections::*,
    test_ipc::{assert_report, invoke},
};
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

/// `reply` without the ids, times and log references the app generates: random
/// digits that spell a four-digit PIN by chance, about one run in 800.
fn without_generated(reply: &Value) -> Value {
    match reply {
        Value::Object(fields) => fields
            .iter()
            .filter(|(name, _)| {
                !matches!(name.as_str(), "id" | "activeId" | "reference") && !name.ends_with("Ms")
            })
            .map(|(name, value)| (name.clone(), without_generated(value)))
            .collect(),
        Value::Array(items) => items.iter().map(without_generated).collect(),
        other => other.clone(),
    }
}

/// Whether `reply` carries the tests' PIN or a hash of one.
fn leaks_pin(reply: &Value) -> bool {
    let text = without_generated(reply).to_string();
    text.contains("2468") || text.contains("argon2")
}

#[test]
fn only_what_the_app_generates_may_spell_the_pin() {
    let by_chance = [
        // CI run 42069074708's two attempts, then an error report's reference.
        json!({"accounts": [{"id": "08526c24-aadd-1725-1d92-3863999dd210",
            "createdAtMs": 1793898246833_u64}]}),
        json!({"accounts": [{"id": "af694205-112f-86e2-5f55-9ced332468ea"}],
            "activeId": "af694205-112f-86e2-5f55-9ced332468ea"}),
        json!({"code": "forbidden", "summary": "Not allowed.", "reference": "a2468b0c"}),
    ];
    for reply in by_chance {
        assert!(!leaks_pin(&reply), "{reply}");
    }
    let leaks = [
        json!({"accounts": [{"id": "a", "pin": "2468"}]}),
        json!({"accounts": [{"id": "a", "pinHash": "$argon2id$v=19$m=1,t=1,p=1$c2FsdA$aGFzaA"}]}),
        json!({"code": "invalid", "summary": "2468 is wrong.", "reference": "0"}),
        json!({"code": "invalid", "summary": "Wrong.", "detail": "pin=2468", "reference": "0"}),
    ];
    for reply in leaks {
        assert!(leaks_pin(&reply), "{reply}");
    }
}

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
        errors.push(invoke(&window, command, body).unwrap_err());
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
    assert!(unlock.contains("Every admin needs a PIN"), "{unlock}");
    let gone = invoke(
        &window,
        "account_set_pin",
        json!({"id": "account-1", "pin": null}),
    )
    .unwrap_err();
    assert_report(&gone, "invalid", &["account-1"]);

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
    let list = invoke(&window, "accounts_list", json!({})).unwrap();
    // The ids it skips are still the one read before there was a PIN.
    assert_eq!(list["activeId"], json!(admin));
    assert_eq!(list["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(list["accounts"][0]["id"], json!(admin));
    for reply in errors.iter().chain([&list]) {
        assert!(!leaks_pin(reply), "{reply}");
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
