//! Library sync between two cores on loopback (#230): changes arrive within
//! seconds, devices apart for a week converge, a dropped exchange leaves
//! both consistent and resumes, sync doesn't hold up the library, and a
//! category set to This device only stops sending and applying until it's
//! turned back on (#227).

use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{ConnectionPurpose, Devices, MemoryKeyStore, Reach};
use caskade_core::model::{
    CollectionKind, ExternalId, Game, GameEdit, GameEntry, PlaySession, SaveCollection, TextEdit,
};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::exchange::{self, Options};
use caskade_core::sync::{SyncEngine, SyncService};
use serde_json::json;

struct Core {
    store: LibraryStore,
    devices: Devices,
    settings: SettingsService,
    account: String,
    _dir: tempfile::TempDir,
}

impl Core {
    async fn id(&self) -> String {
        self.devices.status().await.this_device.id.unwrap()
    }

    /// Its sync service, started.
    fn sync(&self) -> SyncService {
        let service = SyncService::new(self.store.clone(), self.devices.clone());
        service.set_settings(self.settings.clone());
        service.start();
        service
    }
}

async fn core(name: &str) -> Core {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let account = accounts.active_id().unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings.clone());
    devices.start();
    let core = Core {
        store,
        devices,
        settings,
        account,
        _dir: dir,
    };
    within("the endpoint starts", 20, || async {
        core.devices.status().await.this_device.listening
    })
    .await;
    core.devices.rename(name).await.unwrap();
    // The same game, discovered on each.
    let game = core.store.upsert_game(&Game::new("Hades")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), "1145360");
    entry.ids = vec![ExternalId::Steam { app_id: 1_145_360 }];
    core.store.upsert_entry(&entry).await.unwrap();
    core
}

async fn pair(a: &Core, b: &Core) {
    let link = a.devices.pairing_link().await.unwrap();
    b.devices.accept_pairing_link(&link.url).await.unwrap();
    let b_id = b.id().await;
    within("the link pairs both", 20, || async {
        a.devices
            .status()
            .await
            .devices
            .iter()
            .any(|d| d.id == b_id)
    })
    .await;
}

/// Waits up to `seconds` for `check`.
async fn within<F, Fut>(what: &str, seconds: u64, check: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !check().await {
        assert!(Instant::now() < deadline, "{what} within {seconds} s");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn hades(core: &Core) -> String {
    core.store
        .sync_snapshot()
        .await
        .unwrap()
        .games
        .into_iter()
        .next()
        .unwrap()
}

async fn description(core: &Core) -> String {
    core.store
        .game(&hades(core).await)
        .await
        .unwrap()
        .unwrap()
        .description
}

fn describe(text: &str) -> GameEdit {
    GameEdit {
        description: Some(TextEdit::Set {
            value: text.to_owned(),
        }),
        ..GameEdit::default()
    }
}

async fn collection(core: &Core, name: &str) -> String {
    core.store
        .save_collection(
            &core.account,
            &SaveCollection {
                id: None,
                name: name.to_owned(),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap()
        .id
}

async fn has_collection(core: &Core, id: &str) -> bool {
    core.store
        .sync_snapshot()
        .await
        .unwrap()
        .values
        .contains_key(&(
            caskade_core::sync::change::Entity::Collection,
            id.to_owned(),
            String::new(),
        ))
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_a_collection_and_a_session_arrive_within_seconds() {
    let desk = core("Desk").await;
    let tv = core("TV").await;
    pair(&desk, &tv).await;
    let (_desk_sync, _tv_sync) = (desk.sync(), tv.sync());
    // The first sync matches the games.
    let desk_id = desk.id().await;
    within("a first sync", 20, || async {
        tv.devices
            .status()
            .await
            .devices
            .iter()
            .any(|d| d.id == desk_id && d.last_sync_ms.is_some())
    })
    .await;

    let started = Instant::now();
    let game = hades(&desk).await;
    desk.store
        .edit_game(&game, &describe("Escape the Underworld"), None)
        .await
        .unwrap();
    let couch = collection(&desk, "Couch").await;
    let entry = desk.store.sync_game_entry_stores(&game).await.unwrap()[0]
        .0
        .clone();
    let mut session = PlaySession::new(&game, &entry, 1_000_000);
    session.ended_at_ms = Some(1_600_000);
    session.duration_ms = 600_000;
    let session = desk.store.upsert_session(&session).await.unwrap();

    within(
        "the edit, the collection and the session reach the TV",
        15,
        || async {
            let tv_game = hades(&tv).await;
            description(&tv).await == "Escape the Underworld"
                && has_collection(&tv, &couch).await
                && tv
                    .store
                    .sessions_for_game(&tv_game, 10)
                    .await
                    .unwrap()
                    .iter()
                    .any(|s| s.id == session.id)
        },
    )
    .await;
    assert!(started.elapsed() < Duration::from_secs(15));
    // And back the other way.
    let tv_game = hades(&tv).await;
    tv.store.set_game_favorite(&tv_game, true).await.unwrap();
    within("the TV's favourite reaches the desk", 15, || async {
        desk.store.game(&game).await.unwrap().unwrap().favorite
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn devices_apart_for_a_week_converge_the_same_way() {
    let desk = core("Desk").await;
    let laptop = core("Laptop").await;
    pair(&desk, &laptop).await;
    // The laptop is away for a week: nothing syncs there.
    let day = 86_400_000;
    let desk_sync = desk.sync();
    let desk_engine = loop {
        if let Some(engine) = desk_sync.engine() {
            break engine;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let laptop_engine = SyncEngine::new(laptop.store.clone(), laptop.id().await);
    let laptop_engine = Arc::new(laptop_engine);

    // Day 1, the desk; day 6, the laptop: the same field, and others.
    desk_engine.set_wall_clock(move || 1_900_000_000_000 + day);
    laptop_engine.set_wall_clock(move || 1_900_000_000_000 + 6 * day);
    let desk_game = hades(&desk).await;
    desk.store
        .edit_game(&desk_game, &describe("desk, day 1"), None)
        .await
        .unwrap();
    let desk_shelf = collection(&desk, "Desk shelf").await;
    desk_engine.capture().await.unwrap();
    let laptop_game = hades(&laptop).await;
    laptop
        .store
        .edit_game(&laptop_game, &describe("laptop, day 6"), None)
        .await
        .unwrap();
    laptop
        .store
        .set_game_favorite(&laptop_game, true)
        .await
        .unwrap();
    let laptop_shelf = collection(&laptop, "Laptop shelf").await;
    laptop_engine.capture().await.unwrap();

    // They meet.
    let _laptop_sync = laptop.sync();
    within("they converge", 30, || async {
        description(&desk).await == "laptop, day 6"
            && description(&laptop).await == "laptop, day 6"
            && has_collection(&desk, &laptop_shelf).await
            && has_collection(&laptop, &desk_shelf).await
            && desk.store.game(&desk_game).await.unwrap().unwrap().favorite
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_exchange_leaves_both_consistent_and_resumes() {
    let desk = core("Desk").await;
    let tv = core("TV").await;
    pair(&desk, &tv).await;
    let _tv_sync = tv.sync();
    let desk_engine = SyncEngine::new(desk.store.clone(), desk.id().await);
    // Enough changes for several pages.
    let mut collections = Vec::new();
    for index in 0..150 {
        collections.push(collection(&desk, &format!("Shelf {index}")).await);
    }
    desk_engine.capture().await.unwrap();

    // The connection drops after one page.
    let tv_id = tv.id().await;
    let mut session = desk
        .devices
        .connect(&tv_id, ConnectionPurpose::Sync)
        .await
        .unwrap();
    let options = Options {
        fail_after_pages: Some(1),
    };
    let dropped = exchange::exchange(&desk_engine, &tv_id, &mut session, &options).await;
    assert!(dropped.is_err());
    session.close();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // The TV has a prefix of the changes, each whole, and a sound library.
    let arrived = collections.len();
    let mut have = 0;
    for id in &collections {
        if has_collection(&tv, id).await {
            have += 1;
        }
    }
    assert!(have < arrived, "only part arrived: {have}");
    let tv_seen = tv.store.sync_seen().await.unwrap();
    assert!(tv_seen.contains_key(&desk.id().await));
    assert_eq!(
        tv.store.sync_integrity().await.unwrap(),
        Vec::<String>::new()
    );

    // The next connection carries on from there.
    let mut session = desk
        .devices
        .connect(&tv_id, ConnectionPurpose::Sync)
        .await
        .unwrap();
    exchange::exchange(&desk_engine, &tv_id, &mut session, &Options::default())
        .await
        .unwrap();
    session.end().await;
    for id in &collections {
        assert!(has_collection(&tv, id).await);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_doesnt_hold_up_the_library() {
    let desk = core("Desk").await;
    let tv = core("TV").await;
    pair(&desk, &tv).await;
    for index in 0..600 {
        collection(&desk, &format!("Shelf {index}")).await;
    }
    let (_desk_sync, _tv_sync) = (desk.sync(), tv.sync());
    // While thousands of changes go across, the library answers quickly.
    let game = hades(&tv).await;
    let mut slowest = Duration::ZERO;
    for round in 0..40 {
        let started = Instant::now();
        tv.store.game(&game).await.unwrap();
        tv.store
            .set_game_favorite(&game, round % 2 == 0)
            .await
            .unwrap();
        slowest = slowest.max(started.elapsed());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(slowest < Duration::from_secs(1), "slowest {slowest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn this_device_only_stops_sending_and_applying_until_turned_back_on() {
    let desk = core("Desk").await;
    let tv = core("TV").await;
    pair(&desk, &tv).await;
    let (_desk_sync, _tv_sync) = (desk.sync(), tv.sync());
    let desk_id = desk.id().await;
    within("a first sync", 20, || async {
        tv.devices
            .status()
            .await
            .devices
            .iter()
            .any(|d| d.id == desk_id && d.last_sync_ms.is_some())
    })
    .await;

    // The TV keeps its library to itself.
    let library = |state: &str| json!({ "categories": { "library": state }, "neverRelay": false, "port": null });
    tv.settings
        .set("core.sync", library("thisDeviceOnly"), None)
        .await
        .unwrap();
    let desk_game = hades(&desk).await;
    let tv_game = hades(&tv).await;
    desk.store
        .edit_game(&desk_game, &describe("from the desk"), None)
        .await
        .unwrap();
    tv.store.set_game_favorite(&tv_game, true).await.unwrap();
    // Both sync meanwhile (sessions still do), but the library stays put.
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert_eq!(description(&tv).await, "");
    assert!(!desk.store.game(&desk_game).await.unwrap().unwrap().favorite);

    // Back on: what waited applies, and what was held back goes.
    tv.settings
        .set("core.sync", library("sync"), None)
        .await
        .unwrap();
    within(
        "the waiting edit applies and the held-back favourite goes",
        20,
        || async {
            description(&tv).await == "from the desk"
                && desk.store.game(&desk_game).await.unwrap().unwrap().favorite
        },
    )
    .await;
}
