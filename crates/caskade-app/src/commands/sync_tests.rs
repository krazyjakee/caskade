//! Devices through the commands (#226): pairing by link both ways, the
//! `caskade://pair` deep link waiting for an admin, and the admin rule,
//! which the core holds whatever the shell checks.
use super::*;
use crate::{
    error::ErrorCode,
    events::{Emit, Event},
};
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    db::LibraryStore,
    devices::{MemoryKeyStore, Reach},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// What reached the UI.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<Event>>>);

impl Emit for Sink {
    fn emit(&self, event: Event) -> Result<(), String> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

impl Sink {
    /// Every `DevicesChanged` status so far.
    fn statuses(&self) -> Vec<DevicesStatus> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::DevicesChanged(changed) => Some(changed.0.clone()),
                _ => None,
            })
            .collect()
    }

    /// Every opened pairing link announced so far.
    fn opened(&self) -> Vec<PairingLinkTarget> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::PairingLinkOpened(opened) => Some(opened.0.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Waits up to 20 s for `$check`, which may await.
macro_rules! until {
    ($what:expr, $check:expr) => {{
        let deadline = Instant::now() + Duration::from_secs(20);
        while !$check {
            assert!(Instant::now() < deadline, "{}", $what);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }};
}

/// A device on loopback, started as the app starts it.
async fn this_device() -> (LibraryStore, AccountService, Devices, SyncService) {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts.clone(),
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    let sync = SyncService::new(store.clone(), devices.clone());
    (store, accounts, devices, sync)
}

/// A second device, core only.
async fn other_device() -> Devices {
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
}

/// A second device with library sync running, core only.
async fn other_device_syncing() -> (Devices, LibraryStore, SyncService) {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.start();
    let sync = SyncService::new(store.clone(), devices.clone());
    sync.start();
    (devices, store, sync)
}

#[tokio::test(flavor = "multi_thread")]
async fn devices_pair_by_link_and_by_an_opened_link() {
    let (_store, accounts, devices, sync) = this_device().await;
    let sink = Sink::default();
    setup(Events::new(sink.clone()), &devices, &sync);
    let opened = OpenedPairingLink::default();

    until!(
        "the endpoint starts",
        devices_status(&devices)
            .await
            .unwrap()
            .this_device
            .listening
    );
    devices_rename(&devices, "Desk").await.unwrap();
    devices_set_kind(&devices, DeviceKind::Laptop)
        .await
        .unwrap();
    let status = devices_status(&devices).await.unwrap();
    assert_eq!(status.this_device.name, "Desk");
    assert_eq!(status.this_device.kind, DeviceKind::Laptop);
    let own_id = status.this_device.id.clone().unwrap();

    // A link minted here pairs the other device.
    let tv = other_device().await;
    until!(
        "the other device starts",
        tv.status().await.this_device.listening
    );
    let link = devices_pairing_link(&devices).await.unwrap();
    assert!(link.url.starts_with("caskade://pair?t="));
    tv.accept_pairing_link(&link.url).await.unwrap();
    let tv_id = tv.status().await.this_device.id.unwrap();
    until!(
        "the UI hears about the pairing",
        sink.statuses()
            .iter()
            .any(|status| status.devices.iter().any(|d| d.id == tv_id))
    );
    let listed = devices_status(&devices).await.unwrap();
    let text = serde_json::to_string(&listed).unwrap();
    assert!(!text.contains("grant"), "no grant reaches the UI: {text}");

    // A caskade://pair link opened from outside waits for an admin.
    let phone = other_device().await;
    until!(
        "the phone starts",
        phone.status().await.this_device.listening
    );
    let phone_link = phone.pairing_link().await.unwrap().url;
    let phone_id = phone.status().await.this_device.id.unwrap();
    let url = url::Url::parse(&phone_link).unwrap();
    assert!(is_pair_link(&url));
    assert!(!is_pair_link(
        &url::Url::parse("caskade://install?url=x").unwrap()
    ));
    assert!(hold_pair_link(
        &Events::new(sink.clone()),
        Some(&opened),
        &url
    ));
    let announced = sink.opened();
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].device_id, phone_id);
    assert!(
        !serde_json::to_string(&announced[0])
            .unwrap()
            .contains("caskadepair"),
        "the link stays here"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        phone.status().await.devices.is_empty(),
        "opening the link didn't pair"
    );
    let waiting = devices_opened_link(&opened).await.unwrap().unwrap();
    assert_eq!(waiting.device_id, phone_id);
    let paired = devices_accept_opened_link(&devices, &opened).await.unwrap();
    assert_eq!(paired.id, phone_id);
    assert_eq!(devices_opened_link(&opened).await.unwrap(), None);
    until!(
        "the phone lists this device",
        phone.status().await.devices.iter().any(|d| d.id == own_id)
    );

    // Forgetting.
    devices_forget(&devices, &phone_id).await.unwrap();
    until!(
        "the phone was told",
        phone.status().await.devices.is_empty()
    );

    // Only admins: with a standard account signed in, every change is refused.
    let admin = accounts.active().await.unwrap().unwrap();
    accounts
        .set_pin(&admin.id, Some("2468".into()))
        .await
        .unwrap();
    let kid = accounts
        .create(caskade_core::accounts::NewAccount {
            name: "Kid".into(),
            avatar: caskade_core::accounts::DEFAULT_AVATAR.into(),
            access: caskade_core::accounts::AccountAccess {
                role: caskade_core::accounts::AccountRole::Standard,
                permissions: caskade_core::accounts::Permissions::all(),
                max_age: None,
                unrated_needs_pin: false,
            },
            pin: None,
        })
        .await
        .unwrap();
    accounts.sign_in(&kid.id, None).await.unwrap();
    let refusals = [
        (
            "devices_pairing_link",
            devices_pairing_link(&devices).await.map(|_| ()),
        ),
        (
            "devices_accept_pairing_link",
            devices_accept_pairing_link(&devices, "caskade://pair?t=x")
                .await
                .map(|_| ()),
        ),
        (
            "devices_pair_nearby",
            devices_pair_nearby(&devices, &tv_id).await.map(|_| ()),
        ),
        (
            "devices_confirm_pairing",
            devices_confirm_pairing(&devices, &tv_id).await,
        ),
        ("devices_forget", devices_forget(&devices, &tv_id).await),
        ("devices_rename", devices_rename(&devices, "Mine").await),
        (
            "devices_create_identity",
            devices_create_identity(&devices).await,
        ),
    ];
    for (command, reply) in refusals {
        let error = reply.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden, "{command}: {error:?}");
    }
    let status = devices_status(&devices).await.unwrap();
    assert_eq!(status.devices.len(), 1, "still paired with the TV");
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_status_reaches_the_ui() {
    let (store, _accounts, devices, sync) = this_device().await;
    let sink = Sink::default();
    setup(Events::new(sink.clone()), &devices, &sync);
    until!(
        "the endpoint starts",
        devices_status(&devices)
            .await
            .unwrap()
            .this_device
            .listening
    );

    // A paired device with a game this one doesn't have.
    let (tv, tv_store, _tv_sync) = other_device_syncing().await;
    until!("the TV starts", tv.status().await.this_device.listening);
    tv_store
        .upsert_game(&caskade_core::model::Game::new("Celeste"))
        .await
        .unwrap();
    let link = devices_pairing_link(&devices).await.unwrap();
    tv.accept_pairing_link(&link.url).await.unwrap();
    let tv_id = tv.status().await.this_device.id.unwrap();

    devices_sync_now(&sync, &tv_id).await.unwrap();
    // The UI hears the sync run and finish, and the game arrives.
    until!(
        "a finished sync in DevicesChanged",
        sink.statuses().iter().any(|status| status
            .devices
            .iter()
            .any(|d| d.id == tv_id && d.last_sync_ms.is_some() && !d.syncing))
    );
    let saw_syncing = sink
        .statuses()
        .iter()
        .any(|status| status.devices.iter().any(|d| d.id == tv_id && d.syncing));
    assert!(saw_syncing, "the UI saw it syncing");
    until!("the TV's game arrives", has_game(&store, "Celeste").await);
    let status = devices_status(&devices).await.unwrap();
    assert_eq!(status.devices[0].sync_error, None);
}

/// Whether the library syncs a game titled `title`.
async fn has_game(store: &LibraryStore, title: &str) -> bool {
    for id in store.sync_snapshot().await.unwrap().games.iter() {
        if store.game(id).await.unwrap().unwrap().title == title {
            return true;
        }
    }
    false
}
