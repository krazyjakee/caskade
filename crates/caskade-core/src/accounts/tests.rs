//! Account rules, PIN hashes and the throttle, over a private database.
use super::*;
use crate::bus::Event;

async fn service(
    store: &LibraryStore,
) -> (AccountService, tokio::sync::broadcast::Receiver<Event>) {
    let bus = Bus::default();
    let events = bus.stream();
    (
        AccountService::open(store.clone(), bus).await.unwrap(),
        events,
    )
}

fn new(name: &str, role: AccountRole, pin: Option<&str>) -> NewAccount {
    NewAccount {
        name: name.into(),
        avatar: DEFAULT_AVATAR.into(),
        access: AccountAccess {
            role,
            permissions: Permissions {
                settings: vec!["appearance".into()],
                install: false,
            },
            max_age: Some(12),
            unrated_needs_pin: true,
        },
        pin: pin.map(Into::into),
    }
}

fn update(account: &Account, access: Option<AccountAccess>) -> AccountUpdate {
    AccountUpdate {
        name: account.name.clone(),
        avatar: account.avatar.clone(),
        access,
    }
}

fn standard() -> AccountAccess {
    AccountAccess {
        role: AccountRole::Standard,
        permissions: Permissions::default(),
        max_age: None,
        unrated_needs_pin: false,
    }
}

#[tokio::test]
async fn a_library_starts_with_one_admin_signed_in_without_a_pin() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (accounts, _) = service(&store).await;
    let list = accounts.list().await.unwrap();
    assert_eq!(list.len(), 1);
    let admin = &list[0];
    assert_eq!(admin.role, AccountRole::Admin);
    assert_eq!(admin.avatar, DEFAULT_AVATAR);
    assert!(!admin.has_pin);
    assert_eq!(admin.permissions, Permissions::all());
    assert_eq!(accounts.active_id(), Some(admin.id.clone()));
}

#[tokio::test]
async fn standard_accounts_need_every_admin_locked_first() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (accounts, mut events) = service(&store).await;
    let admin = accounts.active().await.unwrap().unwrap();
    let error = accounts
        .create(new("Kid", AccountRole::Standard, None))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("PIN"), "{error}");
    assert_eq!(accounts.list().await.unwrap().len(), 1, "rolled back");

    accounts
        .set_pin(&admin.id, Some("2468".into()))
        .await
        .unwrap();
    let kid = accounts
        .create(new("Kid", AccountRole::Standard, None))
        .await
        .unwrap();
    assert_eq!(kid.role, AccountRole::Standard);
    assert_eq!(kid.permissions.settings, ["appearance"]);
    assert!(!kid.permissions.install);
    assert_eq!(kid.max_age, Some(12));
    assert!(kid.unrated_needs_pin);
    let changed = events.try_recv().unwrap();
    assert_eq!(
        changed,
        Event::AccountChanged {
            active_id: Some(admin.id.clone())
        }
    );

    // Admins keep their PINs while a standard account exists, and new
    // admins come with one.
    assert!(accounts.set_pin(&admin.id, None).await.is_err());
    assert!(
        accounts
            .create(new("Parent", AccountRole::Admin, None))
            .await
            .is_err()
    );
    let parent = accounts
        .create(new("Parent", AccountRole::Admin, Some("1357")))
        .await
        .unwrap();
    assert!(parent.has_pin);
    assert_eq!(parent.permissions, Permissions::all(), "admins may do all");

    // Without standard accounts, an admin may drop its PIN again.
    assert!(accounts.delete(&kid.id).await.unwrap());
    accounts.set_pin(&admin.id, None).await.unwrap();
}

#[tokio::test]
async fn the_last_admin_stays() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (accounts, _) = service(&store).await;
    let admin = accounts.active().await.unwrap().unwrap();
    assert!(accounts.delete(&admin.id).await.is_err());
    assert!(
        accounts
            .update(&admin.id, update(&admin, Some(standard())))
            .await
            .is_err()
    );
    let second = accounts
        .create(new("Second", AccountRole::Admin, None))
        .await
        .unwrap();
    // With two, one may go.
    accounts
        .update(&second.id, update(&second, Some(standard())))
        .await
        .unwrap_err(); // a standard account needs every admin locked
    accounts
        .set_pin(&admin.id, Some("0000".into()))
        .await
        .unwrap();
    let demoted = accounts
        .update(&second.id, update(&second, Some(standard())))
        .await
        .unwrap();
    assert_eq!(demoted.role, AccountRole::Standard);
    assert_eq!(demoted.permissions, Permissions::default());
}

#[tokio::test]
async fn only_an_admin_or_the_account_itself_changes_it() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (accounts, _) = service(&store).await;
    let admin = accounts.active().await.unwrap().unwrap();
    accounts
        .set_pin(&admin.id, Some("9876".into()))
        .await
        .unwrap();
    let kid = accounts
        .create(new("Kid", AccountRole::Standard, None))
        .await
        .unwrap();
    let other = accounts
        .create(new("Other", AccountRole::Standard, None))
        .await
        .unwrap();
    assert_eq!(
        accounts.sign_in(&kid.id, None).await.unwrap(),
        PinCheck::Matched
    );
    assert_eq!(accounts.active_id(), Some(kid.id.clone()));

    // Its own name, avatar and PIN.
    let renamed = accounts
        .update(
            &kid.id,
            AccountUpdate {
                name: "  Ace ".into(),
                avatar: DEFAULT_AVATAR.into(),
                access: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.name, "Ace");
    assert!(
        accounts
            .set_pin(&kid.id, Some("1111".into()))
            .await
            .unwrap()
            .has_pin
    );
    // Not its access, nor anyone else.
    let promote = AccountAccess {
        role: AccountRole::Admin,
        ..standard()
    };
    assert!(
        accounts
            .update(&kid.id, update(&renamed, Some(promote)))
            .await
            .is_err()
    );
    assert!(
        accounts
            .update(&other.id, update(&other, None))
            .await
            .is_err()
    );
    assert!(
        accounts
            .set_pin(&other.id, Some("2222".into()))
            .await
            .is_err()
    );
    assert!(accounts.delete(&other.id).await.is_err());
    assert!(
        accounts
            .create(new("New", AccountRole::Standard, None))
            .await
            .is_err()
    );
    assert!(accounts.set_pin(&admin.id, None).await.is_err());

    // Signed out, nobody changes anything.
    accounts.sign_out();
    assert_eq!(accounts.active_id(), None);
    assert!(accounts.require_active().is_err());
    assert!(
        accounts
            .update(&kid.id, update(&renamed, None))
            .await
            .is_err()
    );

    // Bad input is refused.
    accounts
        .sign_in(&admin.id, Some("9876".into()))
        .await
        .unwrap();
    let mut bad = new("Bad", AccountRole::Standard, None);
    bad.avatar = "avatar:nope".into();
    assert!(accounts.create(bad).await.is_err());
    let mut bad = new("Bad", AccountRole::Standard, None);
    bad.access.permissions.settings = vec!["accounts".into()];
    assert!(accounts.create(bad).await.is_err());
    assert!(
        accounts
            .create(new(" ", AccountRole::Standard, None))
            .await
            .is_err()
    );
    for pin in ["123", "123456789", "12a4", "١٢٣٤"] {
        let error = accounts
            .set_pin(&kid.id, Some(pin.into()))
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains(pin), "the error names no PIN: {error}");
    }
}

#[tokio::test]
async fn pins_are_salted_argon2id_hashes() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (accounts, _) = service(&store).await;
    let admin = accounts.active().await.unwrap().unwrap();
    let second = accounts
        .create(new("Second", AccountRole::Admin, Some("4321")))
        .await
        .unwrap();
    accounts
        .set_pin(&admin.id, Some("4321".into()))
        .await
        .unwrap();
    let rows = store.account_rows().await.unwrap();
    let hashes: Vec<_> = rows
        .iter()
        .map(|row| row.pin_hash.clone().unwrap())
        .collect();
    for hash in &hashes {
        assert!(hash.starts_with("$argon2id$"), "{hash}");
        assert!(!hash.contains("4321"));
    }
    assert_ne!(hashes[0], hashes[1], "a salt per account");
    // The webview's view of an account has no hash.
    let json = serde_json::to_string(&accounts.list().await.unwrap()).unwrap();
    assert!(!json.contains("argon2"));

    accounts.sign_out();
    assert_eq!(
        accounts
            .sign_in(&second.id, Some("1234".into()))
            .await
            .unwrap(),
        PinCheck::Wrong
    );
    assert_eq!(accounts.active_id(), None);
    assert_eq!(
        accounts.sign_in(&second.id, None).await.unwrap(),
        PinCheck::Wrong
    );
    assert_eq!(
        accounts
            .sign_in(&second.id, Some("4321".into()))
            .await
            .unwrap(),
        PinCheck::Matched
    );
    assert_eq!(accounts.active_id(), Some(second.id.clone()));
    assert_eq!(
        accounts.verify_admin_pin("4321".into()).await.unwrap(),
        PinCheck::Matched
    );
    assert_eq!(
        accounts.verify_admin_pin("0000".into()).await.unwrap(),
        PinCheck::Wrong
    );
}

#[tokio::test]
async fn wrong_pins_lock_an_account_across_a_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let store = LibraryStore::open(&path).await.unwrap();
    let (accounts, _) = service(&store).await;
    let admin = accounts.active().await.unwrap().unwrap();
    accounts
        .set_pin(&admin.id, Some("5555".into()))
        .await
        .unwrap();
    accounts.sign_out();
    for _ in 0..pin::TRIES {
        assert_eq!(
            accounts
                .sign_in(&admin.id, Some("0000".into()))
                .await
                .unwrap(),
            PinCheck::Wrong
        );
    }
    // Locked: even the right PIN waits.
    let locked = accounts
        .sign_in(&admin.id, Some("5555".into()))
        .await
        .unwrap();
    let PinCheck::Locked { until_ms } = locked else {
        panic!("{locked:?}");
    };
    assert!(until_ms > crate::now_ms() + 20_000);
    store.close().await;

    let store = LibraryStore::open(&path).await.unwrap();
    let (accounts, _) = service(&store).await;
    assert_eq!(accounts.active_id(), None, "a PIN keeps the picker");
    assert_eq!(
        accounts
            .sign_in(&admin.id, Some("5555".into()))
            .await
            .unwrap(),
        PinCheck::Locked { until_ms }
    );
    assert_eq!(
        accounts.list().await.unwrap()[0].locked_until_ms,
        Some(until_ms)
    );
    // The admin override is locked with it.
    assert_eq!(
        accounts.verify_admin_pin("5555".into()).await.unwrap(),
        PinCheck::Locked { until_ms }
    );
    store.close().await;
}

#[test]
fn waits_double_from_30_seconds_to_15_minutes() {
    let waits: Vec<_> = (1..=7).map(pin::wait_ms).collect();
    assert_eq!(
        waits,
        [30_000, 60_000, 120_000, 240_000, 480_000, 900_000, 900_000]
    );
    assert_eq!(pin::wait_ms(u32::MAX), 900_000);
    let mut throttle = Throttle::default();
    for _ in 0..pin::TRIES - 1 {
        throttle = throttle.wrong(0);
        assert!(!throttle.locked(0));
    }
    throttle = throttle.wrong(0);
    assert!(throttle.locked(29_999));
    assert!(!throttle.locked(30_000));
    for _ in 0..pin::TRIES {
        throttle = throttle.wrong(30_000);
    }
    assert_eq!(throttle.locked_until_ms, 90_000);
}

#[tokio::test]
async fn deleting_an_account_deletes_its_collections() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (accounts, _) = service(&store).await;
    let admin = accounts.active().await.unwrap().unwrap();
    let second = accounts
        .create(new("Second", AccountRole::Admin, None))
        .await
        .unwrap();
    let game = crate::model::Game::new("Shared");
    store.upsert_game(&game).await.unwrap();
    let mut shelf = crate::model::Collection::manual("Shelf");
    shelf.game_ids = vec![game.id.clone()];
    store.upsert_collection(&second.id, &shelf).await.unwrap();
    store
        .upsert_collection(&admin.id, &crate::model::Collection::manual("Mine"))
        .await
        .unwrap();
    assert!(accounts.delete(&second.id).await.unwrap());
    assert!(store.collections(&second.id).await.unwrap().is_empty());
    assert_eq!(store.collections(&admin.id).await.unwrap().len(), 1);
    assert!(
        store
            .game_collection_ids(&second.id, &game.id)
            .await
            .unwrap()
            .is_empty()
    );
    // The game stays in the library.
    assert!(store.game(&game.id).await.unwrap().is_some());
}

#[test]
fn avatars_are_the_shipped_set() {
    // The UI ships at least 24 (#203); ids are unique and stay `avatar:<name>`.
    assert!(AVATARS.len() >= 25);
    assert_eq!(AVATARS[0], DEFAULT_AVATAR);
    let unique: std::collections::BTreeSet<_> = AVATARS.iter().collect();
    assert_eq!(unique.len(), AVATARS.len());
    assert!(AVATARS.iter().all(|id| {
        id.strip_prefix("avatar:")
            .is_some_and(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase()))
    }));
    assert!(avatar("avatar:rocket").is_ok());
}
