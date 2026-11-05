//! Paired devices (#226): pairing by code and by link, forgetting, the
//! admin check, and the identity, over real endpoints in one process.
//!
//! Code pairing finds the other device over mDNS on this machine's real
//! interfaces with no relay and no internet lookup, as on a local network
//! with no internet; set `CASKADE_TEST_NO_MULTICAST=1` where the platform has
//! no multicast.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use caskade_core::accounts::{
    AccountAccess, AccountRole, AccountService, DEFAULT_AVATAR, NewAccount, Permissions,
};
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{DEVICE_KEY, Devices, DevicesStatus, IdentityStatus, PairingState};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_sync::{Ip, KeyStore, MemoryKeyStore, Purpose, Reach, Relays};
use serde_json::json;

struct Node {
    devices: Devices,
    store: LibraryStore,
    accounts: AccountService,
    keys: Arc<MemoryKeyStore>,
}

impl Node {
    async fn status(&self) -> DevicesStatus {
        self.devices.status().await
    }

    async fn id(&self) -> String {
        self.status().await.this_device.id.unwrap()
    }
}

async fn node_with(
    name: &str,
    reach: Reach,
    keys: Arc<MemoryKeyStore>,
    store: LibraryStore,
) -> Node {
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let devices = Devices::with_reach(store.clone(), accounts.clone(), keys.clone(), reach);
    devices.start();
    let node = Node {
        devices,
        store,
        accounts,
        keys,
    };
    eventually("the endpoint starts", || async {
        node.status().await.this_device.listening
    })
    .await;
    node.devices.rename(name).await.unwrap();
    node
}

async fn node(name: &str) -> Node {
    node_with(
        name,
        Reach::loopback(),
        Arc::new(MemoryKeyStore::default()),
        LibraryStore::in_memory().await.unwrap(),
    )
    .await
}

/// Waits up to 20 s for `check`.
async fn eventually<F, Fut>(what: &str, check: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check().await {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn pair_by_link(a: &Node, b: &Node) {
    let link = a.devices.pairing_link().await.unwrap();
    let paired = b.devices.accept_pairing_link(&link.url).await.unwrap();
    assert_eq!(paired.id, a.id().await);
    let b_id = b.id().await;
    eventually("the link's device stores the pairing", || async {
        a.status().await.devices.iter().any(|d| d.id == b_id)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pairing_link_pairs_both_devices_once() {
    let desk = node("Desk").await;
    let laptop = node("Laptop").await;
    let link = desk.devices.pairing_link().await.unwrap();
    assert!(link.url.starts_with("caskade://pair?t=caskadepair"));

    let paired = laptop.devices.accept_pairing_link(&link.url).await.unwrap();
    assert_eq!(paired.name, "Desk");
    let laptop_id = laptop.id().await;
    eventually("the desk lists the laptop", || async {
        desk.status()
            .await
            .devices
            .iter()
            .any(|d| d.id == laptop_id && d.name == "Laptop")
    })
    .await;

    // A paired device connects with its grant.
    let session = laptop
        .devices
        .connect(&desk.id().await, Purpose::Sync)
        .await
        .unwrap();
    session.end().await;

    // Used: refused with a clear message.
    let other = node("Other").await;
    let error = other
        .devices
        .accept_pairing_link(&link.url)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("already used or has expired"), "{error}");
    assert!(other.status().await.devices.is_empty());

    let error = other
        .devices
        .accept_pairing_link("hello")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("isn't a Caskade pairing link"), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forgotten_device_cannot_reconnect_until_it_pairs_again() {
    let desk = node("Desk").await;
    let laptop = node("Laptop").await;
    pair_by_link(&desk, &laptop).await;
    let (desk_id, laptop_id) = (desk.id().await, laptop.id().await);

    desk.devices.forget(&laptop_id).await.unwrap();
    assert!(desk.status().await.devices.is_empty());
    // Refused now, whether or not the laptop has been told yet.
    assert!(
        laptop
            .devices
            .connect(&desk_id, Purpose::Sync)
            .await
            .is_err()
    );
    // Told: it dropped the desk too. Nothing else is deleted.
    eventually("the laptop forgets the desk", || async {
        laptop.status().await.devices.is_empty()
    })
    .await;
    assert!(desk.store.paired_devices().await.unwrap().is_empty());

    pair_by_link(&desk, &laptop).await;
    let session = laptop
        .devices
        .connect(&desk_id, Purpose::Sync)
        .await
        .unwrap();
    session.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_forgotten_while_away_is_refused_and_told_later() {
    let desk = node("Desk").await;
    let laptop = node("Laptop").await;
    pair_by_link(&desk, &laptop).await;
    let (desk_id, laptop_id) = (desk.id().await, laptop.id().await);

    // The laptop is away: its endpoint is down while the desk forgets it.
    laptop.devices.shutdown().await;
    desk.devices.forget(&laptop_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let rows = desk.store.paired_devices().await.unwrap();
    assert_eq!(rows.len(), 1, "kept until the laptop is told");
    assert!(rows[0].forgotten_at_ms.is_some());
    assert!(desk.status().await.devices.is_empty(), "but not listed");

    // Back, it still holds the old grant, and the desk refuses it.
    let back = node_with(
        "Laptop",
        Reach::loopback(),
        laptop.keys.clone(),
        laptop.store.clone(),
    )
    .await;
    assert_eq!(
        back.id().await,
        laptop_id,
        "the same identity after a restart"
    );
    let refused = back.devices.connect(&desk_id, Purpose::Sync).await;
    assert!(refused.is_err());
    assert!(
        refused.unwrap_err().to_string().contains("refused"),
        "the desk refused it"
    );
    // Its knock got it told: both sides have dropped the pairing.
    eventually("the laptop is told", || async {
        back.status().await.devices.is_empty()
            && desk.store.paired_devices().await.unwrap().is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn only_admins_pair_and_forget() {
    let desk = node("Desk").await;
    let admin = desk.accounts.active().await.unwrap().unwrap();
    desk.accounts
        .set_pin(&admin.id, Some("2468".into()))
        .await
        .unwrap();
    let kid = desk
        .accounts
        .create(NewAccount {
            name: "Kid".into(),
            avatar: DEFAULT_AVATAR.into(),
            access: AccountAccess {
                role: AccountRole::Standard,
                permissions: Permissions::all(),
                max_age: None,
                unrated_needs_pin: false,
            },
            pin: None,
        })
        .await
        .unwrap();
    desk.accounts.sign_in(&kid.id, None).await.unwrap();

    let forbidden = |error: caskade_core::Error| {
        assert!(
            matches!(error, caskade_core::Error::Forbidden(_)),
            "{error:?}"
        );
    };
    let some_id = node("Other").await.id().await;
    forbidden(desk.devices.pairing_link().await.unwrap_err());
    forbidden(
        desk.devices
            .accept_pairing_link("caskade://pair?t=x")
            .await
            .unwrap_err(),
    );
    forbidden(desk.devices.pair_nearby(&some_id).await.unwrap_err());
    forbidden(desk.devices.confirm_pairing(&some_id).await.unwrap_err());
    forbidden(desk.devices.cancel_pairing(&some_id).await.unwrap_err());
    forbidden(desk.devices.forget(&some_id).await.unwrap_err());
    forbidden(desk.devices.rename("Mine").await.unwrap_err());
    forbidden(desk.devices.create_identity().await.unwrap_err());
    // Reading the list is fine.
    assert!(desk.status().await.this_device.listening);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_device_key_asks_before_a_new_identity() {
    let store = LibraryStore::in_memory().await.unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    let first = node_with("Desk", Reach::loopback(), keys.clone(), store.clone()).await;
    let id = first.id().await;
    first.devices.shutdown().await;

    // The keychain entry is gone; this device had an identity.
    keys.clear();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let devices = Devices::with_reach(store.clone(), accounts, keys.clone(), Reach::loopback());
    devices.start();
    eventually("the loss is reported", || async {
        devices.status().await.this_device.identity == IdentityStatus::Missing
    })
    .await;
    let status = devices.status().await;
    assert!(!status.this_device.listening, "sync waits");
    assert!(keys.is_empty(), "nothing was created without asking");

    devices.create_identity().await.unwrap();
    eventually("a new identity runs", || async {
        devices.status().await.this_device.listening
    })
    .await;
    let new_id = devices.status().await.this_device.id.unwrap();
    assert_ne!(new_id, id);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_key_or_grant_reaches_the_settings_or_the_status() {
    let desk = node("Desk").await;
    let laptop = node("Laptop").await;
    pair_by_link(&desk, &laptop).await;

    let secret = desk.keys.get().unwrap().unwrap();
    let rows = desk.store.paired_devices().await.unwrap();
    let grants = [&rows[0].grant_in, &rows[0].grant_out];

    let record = desk
        .store
        .get_setting(DEVICE_KEY)
        .await
        .unwrap()
        .unwrap()
        .to_string();
    assert!(!record.contains(&secret));
    let status = serde_json::to_string(&desk.status().await).unwrap();
    assert!(!status.contains(&secret));
    for grant in grants {
        assert!(
            !status.contains(grant.as_str()),
            "a grant reached the status"
        );
        assert!(!format!("{:?}", rows[0]).contains(grant.as_str()));
    }
}

/// Connect only directly (`core.sync.neverRelay`), from the setting to the
/// endpoint. Every bind asks for the real `Reach::network`; the test keeps
/// what it asked for and binds on loopback with its relay choice, n0's relays
/// swapped for a local one, so nothing reaches the internet.
#[tokio::test(flavor = "multi_thread")]
async fn connect_only_directly_rebinds_the_endpoint_without_relays() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    let (local_relay, _relay) = caskade_sync::testing::local_relay().await;
    let asked = Arc::new(Mutex::new(Vec::<Reach>::new()));
    let binds = asked.clone();
    let devices = Devices::with_reach_from(
        store,
        accounts,
        Arc::new(MemoryKeyStore::default()),
        move |network| {
            let relays = match network.relays {
                Relays::N0 => local_relay.clone(),
                _ => Relays::Never,
            };
            binds.lock().unwrap().push(network);
            Reach {
                relays,
                ..Reach::loopback()
            }
        },
    );
    devices.set_settings(settings.clone());
    devices.start();
    let set = async |sync| settings.set("core.sync", sync, None).await.unwrap();
    let last_asked = || {
        asked
            .lock()
            .unwrap()
            .last()
            .map(|reach| reach.relays.clone())
    };
    let binds = || asked.lock().unwrap().len();

    // Off by default: the endpoint has relays, and its relay answers.
    eventually("the relay answers", || async {
        devices.status().await.this_device.internet == Some(true)
    })
    .await;
    assert!(matches!(last_asked(), Some(Relays::N0)));
    assert_eq!(binds(), 1);
    let id = devices.status().await.this_device.id;

    set(json!({ "categories": {}, "neverRelay": true, "port": null })).await;
    eventually("the endpoint rebinds without relays", || async {
        let this_device = devices.status().await.this_device;
        binds() == 2 && this_device.listening && this_device.internet.is_none()
    })
    .await;
    assert!(matches!(last_asked(), Some(Relays::Never)));
    assert_eq!(devices.status().await.this_device.id, id, "the same device");

    // A category choice shares the section but doesn't rebind; the change
    // after it does, and relays come back. Changes apply in order.
    set(json!({
        "categories": { "library": "thisDeviceOnly" },
        "neverRelay": true,
        "port": null,
    }))
    .await;
    set(json!({ "categories": {}, "neverRelay": false, "port": null })).await;
    eventually("the relay answers again", || async {
        devices.status().await.this_device.internet == Some(true)
    })
    .await;
    assert!(matches!(last_asked(), Some(Relays::N0)));
    assert_eq!(binds(), 3, "only changes to the connection rebind");
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_by_code_on_the_local_network_needs_both_confirmations() {
    if std::env::var_os("CASKADE_TEST_NO_MULTICAST").is_some() {
        eprintln!("skipped: CASKADE_TEST_NO_MULTICAST");
        return;
    }
    // A local network with no internet: mDNS only.
    let service = format!("cskd{:08x}", std::process::id() ^ 0x5a5a);
    let lan = || Reach {
        relays: Relays::Never,
        ip: Ip::Loopback { port: None },
        internet_lookup: false,
        lan: Some(service.clone()),
    };
    let new = |name: &'static str| {
        let lan = lan();
        async move {
            node_with(
                name,
                lan,
                Arc::new(MemoryKeyStore::default()),
                LibraryStore::in_memory().await.unwrap(),
            )
            .await
        }
    };
    let desk = new("Desk").await;
    let tv = new("Living room").await;
    let (desk_id, tv_id) = (desk.id().await, tv.id().await);

    eventually("the desk sees the TV nearby", || async {
        desk.status()
            .await
            .nearby
            .iter()
            .any(|d| d.id == tv_id && d.name == "Living room")
    })
    .await;

    let pairing = desk.devices.pair_nearby(&tv_id).await.unwrap();
    assert_eq!(pairing.state, PairingState::Waiting);
    assert!(!pairing.incoming);
    eventually("the TV shows the same code", || async {
        tv.status()
            .await
            .pairing
            .is_some_and(|p| p.code == pairing.code && p.incoming && p.name == "Desk")
    })
    .await;

    // One side's confirmation isn't enough.
    desk.devices.confirm_pairing(&tv_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(desk.status().await.devices.is_empty());
    assert!(tv.status().await.devices.is_empty());

    tv.devices.confirm_pairing(&desk_id).await.unwrap();
    eventually("both are paired", || async {
        desk.status().await.devices.iter().any(|d| d.id == tv_id)
            && tv.status().await.devices.iter().any(|d| d.id == desk_id)
    })
    .await;
    assert!(
        desk.status().await.nearby.iter().all(|d| d.id != tv_id),
        "a paired device isn't listed as nearby"
    );

    // A cancel on either side stores nothing.
    let phone = new("Phone").await;
    let phone_id = phone.id().await;
    eventually("the desk sees the phone", || async {
        desk.status().await.nearby.iter().any(|d| d.id == phone_id)
    })
    .await;
    desk.devices.pair_nearby(&phone_id).await.unwrap();
    eventually("the phone shows the code", || async {
        phone.status().await.pairing.is_some()
    })
    .await;
    phone.devices.cancel_pairing(&desk_id).await.unwrap();
    eventually("the desk sees the cancel", || async {
        desk.status()
            .await
            .pairing
            .is_some_and(|p| p.state == PairingState::Cancelled)
    })
    .await;
    assert!(desk.status().await.devices.iter().all(|d| d.id != phone_id));
    assert!(phone.status().await.devices.is_empty());
}
