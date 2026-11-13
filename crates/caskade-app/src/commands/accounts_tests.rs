//! Accounts called as a shell calls them: the rules hold with the UI
//! bypassed, collections follow the active account, and every change
//! reaches the UI as `account.changed`.
use super::*;
use crate::commands::collections::*;
use crate::error::ErrorCode;
use crate::events::{Emit, Event, Events};
use caskade_core::{
    accounts::{AccountRole, Permissions},
    bus::Bus,
    db::LibraryStore,
    model::SaveCollection,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// The active ids `account.changed` carried, in order.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<Option<String>>>>);

impl Emit for Sink {
    fn emit(&self, event: Event) -> Result<(), String> {
        if let Event::AccountChanged(changed) = event {
            self.0.lock().unwrap().push(changed.active_id);
        }
        Ok(())
    }
}

/// Asserts that `error` is a report of `code` whose summary names none of
/// `internal` (ids, namespaces, type names).
fn assert_report(error: &CommandError, code: ErrorCode, internal: &[&str]) {
    assert_eq!(error.code, code, "{error:?}");
    for name in internal {
        assert!(
            !error.summary.contains(name),
            "{} names {name}",
            error.summary
        );
    }
}

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

fn new_account(value: Value) -> NewAccount {
    serde_json::from_value(value).unwrap()
}

fn update(value: Value) -> AccountUpdate {
    serde_json::from_value(value).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn accounts_sign_in_change_and_own_their_collections() {
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let accounts = AccountService::open(store.clone(), bus.clone())
        .await
        .unwrap();
    let sink = Sink::default();
    crate::events::bridge(Events::new(sink.clone()), &bus);
    let mut errors: Vec<CommandError> = Vec::new();

    // The migrated admin, signed in without a PIN.
    let list = accounts_list(&accounts).await.unwrap();
    let admin = list.accounts[0].id.clone();
    assert_eq!(list.active_id.as_deref(), Some(admin.as_str()));
    assert_eq!(list.accounts[0].role, AccountRole::Admin);
    assert!(!list.accounts[0].has_pin);

    let kid = new_account(
        json!({"name": "Kid", "avatar": "avatar:default", "pin": null,
        "access": {"role": "standard", "maxAge": 12, "unratedNeedsPin": true,
            "permissions": {"settings": ["appearance"], "install": false}}}),
    );
    errors.push(account_create(&accounts, kid.clone()).await.unwrap_err());
    let locked = account_set_pin(&accounts, &admin, Some("2468".into()))
        .await
        .unwrap();
    assert!(locked.has_pin);
    let kid = account_create(&accounts, kid).await.unwrap();
    let kid_id = kid.id.clone();
    assert_eq!(
        kid.permissions,
        Permissions {
            settings: vec!["appearance".into()],
            install: false,
        }
    );
    assert_eq!(kid.max_age, Some(12));
    assert!(kid.unrated_needs_pin);
    // Settings → Accounts can't break the rules: the last admin stays an
    // admin, and keeps its PIN while a standard account exists.
    let demote = account_update(
        &accounts,
        &admin,
        update(json!({"name": "Player", "avatar": "avatar:default",
            "access": {"role": "standard", "maxAge": null, "unratedNeedsPin": false,
                "permissions": {"settings": [], "install": false}}})),
    )
    .await
    .unwrap_err();
    assert!(demote.summary.contains("last admin"), "{demote:?}");
    let unlock = account_set_pin(&accounts, &admin, None).await.unwrap_err();
    assert!(
        unlock.summary.contains("Every admin needs a PIN"),
        "{unlock:?}"
    );
    let gone = account_set_pin(&accounts, "account-1", None)
        .await
        .unwrap_err();
    assert_report(&gone, ErrorCode::Invalid, &["account-1"]);

    let couch: SaveCollection = serde_json::from_value(
        json!({"id": null, "name": "Couch", "description": "", "kind": {"type": "manual"}}),
    )
    .unwrap();
    let mine = save_collection(&store, &accounts, &couch).await.unwrap().id;

    // The kid signs in: the admin's collection is not found.
    assert_eq!(
        account_sign_in(&accounts, &kid_id, None).await.unwrap(),
        PinCheck::Matched
    );
    assert_eq!(
        get_collection(&store, &accounts, &mine).await.unwrap(),
        None
    );
    errors.push(
        collection_games(&store, &accounts, &mine, 0, 10)
            .await
            .unwrap_err(),
    );
    assert!(!delete_collection(&store, &accounts, &mine).await.unwrap());
    assert!(
        list_collections(&store, &accounts)
            .await
            .unwrap()
            .is_empty()
    );
    // A collection of the same name is its own.
    save_collection(&store, &accounts, &couch).await.unwrap();
    assert_eq!(list_collections(&store, &accounts).await.unwrap().len(), 1);

    // The kid can't change its access or anyone else.
    errors.push(
        account_update(
            &accounts,
            &kid_id,
            update(json!({"name": "Kid", "avatar": "avatar:default",
                "access": {"role": "admin", "maxAge": null, "unratedNeedsPin": false,
                    "permissions": {"settings": [], "install": true}}})),
        )
        .await
        .unwrap_err(),
    );
    errors.push(account_set_pin(&accounts, &admin, None).await.unwrap_err());
    errors.push(account_delete(&accounts, &admin).await.unwrap_err());
    let renamed = account_update(
        &accounts,
        &kid_id,
        update(json!({"name": "Ace", "avatar": "avatar:default", "access": null})),
    )
    .await
    .unwrap();
    assert_eq!(renamed.name, "Ace");

    // The admin override answers only whether a PIN matched.
    assert_eq!(
        account_verify_admin_pin(&accounts, "0000".into())
            .await
            .unwrap(),
        PinCheck::Wrong
    );
    assert_eq!(
        account_verify_admin_pin(&accounts, "2468".into())
            .await
            .unwrap(),
        PinCheck::Matched
    );

    // Signed out, collections wait for an account.
    account_sign_out(&accounts).await.unwrap();
    errors.push(list_collections(&store, &accounts).await.unwrap_err());
    assert_eq!(
        account_sign_in(&accounts, &admin, Some("1111".into()))
            .await
            .unwrap(),
        PinCheck::Wrong
    );
    assert_eq!(
        account_sign_in(&accounts, &admin, Some("2468".into()))
            .await
            .unwrap(),
        PinCheck::Matched
    );
    let list = list_collections(&store, &accounts).await.unwrap();
    assert_eq!(list[0].id, mine);
    assert!(account_delete(&accounts, &kid_id).await.unwrap());

    // Neither PIN nor hash ever reaches the UI.
    let list = accounts_list(&accounts).await.unwrap();
    // The ids it skips are still the one read before there was a PIN.
    assert_eq!(list.active_id.as_deref(), Some(admin.as_str()));
    assert_eq!(list.accounts.len(), 1);
    assert_eq!(list.accounts[0].id, admin);
    let replies = errors
        .iter()
        .map(|error| serde_json::to_value(error).unwrap())
        .chain([serde_json::to_value(&list).unwrap()]);
    for reply in replies {
        assert!(!leaks_pin(&reply), "{reply}");
    }

    let last = Some(admin.clone());
    for _ in 0..100 {
        {
            let seen = sink.0.lock().unwrap();
            if seen.last() == Some(&last) && seen.len() >= 7 {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let seen = sink.0.lock().unwrap().clone();
    assert!(seen.contains(&Some(kid_id)), "{seen:?}");
    assert!(seen.contains(&None), "{seen:?}");
    assert_eq!(seen.last(), Some(&last));
}
