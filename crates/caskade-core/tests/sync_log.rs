//! The sync change log (#228): changes from three devices, exchanged in
//! random orders, converge; deletions stay deleted; a clock set back can't
//! make older edits win; compaction keeps the log to what's unseen or live.
//!
//! Devices exchange in process here (`changes_for` → `receive`); #230 does
//! the same over the network.

use std::collections::BTreeMap;

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::model::{
    CollectionKind, ExternalId, Game, GameEdit, GameEntry, ListEdit, SaveCollection, TextEdit,
};
use caskade_core::sync::SyncEngine;
use caskade_core::sync::change::{Entity, key_parts};
use serde_json::Value;

struct Device {
    engine: SyncEngine,
    store: LibraryStore,
    account: String,
}

async fn device(name: char) -> Device {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let account = accounts.active_id().unwrap();
    Device {
        engine: SyncEngine::new(store.clone(), name.to_string().repeat(64)),
        store,
        account,
    }
}

/// `from` sends `to` everything `to` hasn't seen, in pages of `page`.
async fn send(from: &Device, to: &Device, page: usize) {
    loop {
        let cursor = to.engine.seen().await.unwrap();
        let changes = from.engine.changes_for(&cursor, page).await.unwrap();
        if changes.is_empty() {
            break;
        }
        to.engine.receive(&changes).await.unwrap();
    }
}

/// Every device sends every other everything, until nothing changes.
async fn settle(devices: &[&Device]) {
    for _ in 0..4 {
        for device in devices {
            device.engine.capture().await.unwrap();
        }
        for from in devices {
            for to in devices {
                if from.engine.device() != to.engine.device() {
                    send(from, to, 50).await;
                }
            }
        }
    }
}

/// A device's synced data with game ids replaced by the games' automatic
/// titles (unique in these tests), and its own default admin (each
/// device's stays its own, #273) by `household`, so devices compare.
async fn view(device: &Device) -> BTreeMap<String, Value> {
    let snapshot = device.store.sync_snapshot().await.unwrap();
    let mut titles = BTreeMap::new();
    let mut household = None;
    for ((entity, key, field), value) in &snapshot.values {
        if *entity == Entity::Game && field == "identity" {
            titles.insert(key.clone(), value["title"].as_str().unwrap().to_owned());
        }
        if *entity == Entity::Account && caskade_core::db::is_stand_in_marker(value) {
            household = Some(key.clone());
        }
    }
    let household = |text: &str| match &household {
        Some(id) => text.replace(id.as_str(), "household"),
        None => text.to_owned(),
    };
    let name = |id: &str| titles.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let mut out = BTreeMap::new();
    for ((entity, key, field), value) in snapshot.values {
        if entity == Entity::Game && field == "identity" {
            continue;
        }
        let mut parts: Vec<String> = key_parts(&key).into_iter().map(str::to_owned).collect();
        for &part in entity.game_parts() {
            parts[part] = name(&parts[part]);
        }
        let mut value = value;
        if entity == Entity::Session {
            let game = name(value["game"].as_str().unwrap());
            value["game"] = Value::String(game);
        }
        if let Some(account) = value.as_str().map(household) {
            value = Value::String(account);
        }
        out.insert(
            household(&format!("{}:{}:{field}", entity.as_str(), parts.join("|"))),
            value,
        );
    }
    out
}

/// A tiny deterministic generator, so every seed replays.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const TITLES: [&str; 4] = ["Alpha", "Beta", "Gamma", "Delta"];
const WORDS: [&str; 4] = ["red", "green", "blue", "gold"];

async fn games(device: &Device) -> Vec<String> {
    let snapshot = device.store.sync_snapshot().await.unwrap();
    let mut games: Vec<String> = snapshot.games.into_iter().collect();
    games.sort();
    games
}

async fn collections(device: &Device) -> Vec<String> {
    let snapshot = device.store.sync_snapshot().await.unwrap();
    let mut ids: Vec<String> = snapshot
        .values
        .keys()
        .filter(|(entity, _, field)| *entity == Entity::Collection && field.is_empty())
        .map(|(_, key, _)| key.clone())
        .collect();
    ids.sort();
    ids
}

/// One random user action on `device`.
async fn act(device: &Device, rng: &mut Rng) {
    let games = games(device).await;
    let game = games[rng.below(games.len())].clone();
    let word = WORDS[rng.below(WORDS.len())].to_owned();
    match rng.below(9) {
        0 => {
            let game_now = device.store.game(&game).await.unwrap().unwrap();
            device
                .store
                .set_game_favorite(&game, !game_now.favorite)
                .await
                .unwrap();
        }
        1 => {
            let game_now = device.store.game(&game).await.unwrap().unwrap();
            device
                .store
                .set_game_hidden(&game, !game_now.hidden)
                .await
                .unwrap();
        }
        2 | 3 => {
            let title = if rng.below(3) == 0 {
                TextEdit::Reset
            } else {
                TextEdit::Set {
                    value: format!("{word} game"),
                }
            };
            let edit = GameEdit {
                description: Some(title),
                ..GameEdit::default()
            };
            device.store.edit_game(&game, &edit, None).await.unwrap();
        }
        4 => {
            let tags = match rng.below(3) {
                0 => ListEdit::Add { values: vec![word] },
                1 => ListEdit::Remove { values: vec![word] },
                _ => ListEdit::Reset,
            };
            let edit = GameEdit {
                tags: Some(tags),
                ..GameEdit::default()
            };
            device.store.edit_game(&game, &edit, None).await.unwrap();
        }
        5 => {
            device
                .store
                .save_collection(
                    &device.account,
                    &SaveCollection {
                        id: None,
                        name: word,
                        description: String::new(),
                        kind: CollectionKind::Manual,
                    },
                )
                .await
                .unwrap();
        }
        6 => {
            let collections = collections(device).await;
            if !collections.is_empty() {
                let id = &collections[rng.below(collections.len())];
                sqlx_delete_collection(device, id).await;
            }
        }
        7 => {
            let collections = collections(device).await;
            if !collections.is_empty() {
                let id = &collections[rng.below(collections.len())];
                rename_collection(device, id, &word).await;
            }
        }
        _ => {
            let collections = collections(device).await;
            if !collections.is_empty() {
                let id = &collections[rng.below(collections.len())];
                add_member(device, id, &game, rng.below(2) == 0).await;
            }
        }
    }
}

/// The owner of a collection, whichever device's account it is.
async fn owner(device: &Device, id: &str) -> String {
    let snapshot = device.store.sync_snapshot().await.unwrap();
    snapshot
        .values
        .get(&(Entity::Collection, id.to_owned(), "account".to_owned()))
        .and_then(Value::as_str)
        .unwrap_or(&device.account)
        .to_owned()
}

async fn sqlx_delete_collection(device: &Device, id: &str) {
    let owner = owner(device, id).await;
    device.store.delete_collection(&owner, id).await.unwrap();
}

async fn rename_collection(device: &Device, id: &str, name: &str) {
    let owner = owner(device, id).await;
    device
        .store
        .save_collection(
            &owner,
            &SaveCollection {
                id: Some(id.to_owned()),
                name: name.to_owned(),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
}

async fn add_member(device: &Device, id: &str, game: &str, add: bool) {
    let owner = owner(device, id).await;
    let games = [game.to_owned()];
    if add {
        device
            .store
            .add_to_collection(&owner, id, &games)
            .await
            .unwrap();
    } else {
        device
            .store
            .remove_from_collection(&owner, id, &games)
            .await
            .unwrap();
    }
}

/// Three devices that each discovered the same games (the same Steam ids).
async fn three() -> (Device, Device, Device) {
    let a = device('a').await;
    let b = device('b').await;
    let c = device('c').await;
    for device in [&a, &b, &c] {
        for (index, title) in TITLES.iter().enumerate() {
            discovered(device, title, 100 + index as u64).await;
        }
    }
    (a, b, c)
}

async fn discovered(device: &Device, title: &str, app: u64) -> String {
    let game = Game::new(title);
    let game = device.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), app.to_string());
    entry.ids = vec![ExternalId::Steam { app_id: app as u32 }];
    device.store.upsert_entry(&entry).await.unwrap();
    game.id
}

#[tokio::test(flavor = "multi_thread")]
async fn random_interleavings_from_three_devices_converge() {
    for seed in 1..=12u64 {
        let (a, b, c) = three().await;
        let devices = [&a, &b, &c];
        settle(&devices).await;
        let mut rng = Rng(seed);
        for _ in 0..40 {
            let device = devices[rng.below(3)];
            act(device, &mut rng).await;
            device.engine.capture().await.unwrap();
            // Exchanges happen now and then, in either direction.
            if rng.below(3) == 0 {
                let from = devices[rng.below(3)];
                let to = devices[rng.below(3)];
                if from.engine.device() != to.engine.device() {
                    send(from, to, 1 + rng.below(20)).await;
                }
            }
        }
        settle(&devices).await;
        let (va, vb, vc) = (view(&a).await, view(&b).await, view(&c).await);
        assert_eq!(va, vb, "seed {seed}: a and b differ");
        assert_eq!(vb, vc, "seed {seed}: b and c differ");
        // And it's the same Steam games: no duplicates appeared.
        for device in devices {
            assert_eq!(games(device).await.len(), TITLES.len(), "seed {seed}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_order_changes_arrive_in_doesnt_matter() {
    // The same offline changes, delivered to two fresh devices in opposite
    // orders, give the same result.
    let (a, b, c) = three().await;
    settle(&[&a, &b, &c]).await;
    let alpha_a = games(&a).await[0].clone();
    let edit = |value: &str| GameEdit {
        description: Some(TextEdit::Set {
            value: value.to_owned(),
        }),
        ..GameEdit::default()
    };
    // Offline, both edit the same game's description.
    a.store
        .edit_game(&alpha_a, &edit("from a"), None)
        .await
        .unwrap();
    a.engine.capture().await.unwrap();
    let alpha_b = b.store.game(&games(&b).await[0]).await.unwrap().unwrap().id;
    b.store
        .edit_game(&alpha_b, &edit("from b"), None)
        .await
        .unwrap();
    b.engine.capture().await.unwrap();

    send(&a, &c, 1).await;
    send(&b, &c, 1).await;
    let d = {
        let d = device('d').await;
        for (index, title) in TITLES.iter().enumerate() {
            discovered(&d, title, 100 + index as u64).await;
        }
        d
    };
    d.engine.capture().await.unwrap();
    send(&b, &d, 3).await;
    send(&a, &d, 3).await;
    send(&c, &d, 100).await;
    for device in [&c, &d] {
        device.engine.capture().await.unwrap();
    }
    send(&d, &c, 100).await;
    send(&c, &d, 100).await;
    assert_eq!(view(&c).await, view(&d).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deletion_stays_deleted_on_a_device_that_hadnt_seen_it() {
    let (a, b, c) = three().await;
    let collection = a
        .store
        .save_collection(
            &a.account,
            &SaveCollection {
                id: None,
                name: "Couch".into(),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    settle(&[&a, &b, &c]).await;
    assert!(collections(&c).await.contains(&collection.id));

    // A deletes it and tells B; C hasn't heard, and renames it meanwhile.
    a.store
        .delete_collection(&a.account, &collection.id)
        .await
        .unwrap();
    a.engine.capture().await.unwrap();
    send(&a, &b, 100).await;
    assert!(!collections(&b).await.contains(&collection.id));
    rename_collection(&c, &collection.id, "Still here?").await;
    c.engine.capture().await.unwrap();

    // C's older copy doesn't bring it back, on B or anywhere.
    send(&c, &b, 100).await;
    assert!(!collections(&b).await.contains(&collection.id));
    settle(&[&a, &b, &c]).await;
    for device in [&a, &b, &c] {
        assert!(!collections(device).await.contains(&collection.id));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clock_set_back_doesnt_let_older_edits_win() {
    let (a, b, _c) = three().await;
    // B's clock is right; A's is an hour behind.
    let hour = 3_600_000;
    a.engine.set_wall_clock(move || 10 * hour);
    b.engine.set_wall_clock(move || 11 * hour);
    settle(&[&a, &b]).await;
    let set = |value: &str| GameEdit {
        description: Some(TextEdit::Set {
            value: value.to_owned(),
        }),
        ..GameEdit::default()
    };
    let game_b = games(&b).await[0].clone();
    b.store
        .edit_game(&game_b, &set("older, from b"), None)
        .await
        .unwrap();
    b.engine.capture().await.unwrap();
    send(&b, &a, 100).await;

    // A edits after receiving B's, with its clock behind (and set further
    // back): the later edit still wins, on both.
    a.engine.set_wall_clock(move || 9 * hour);
    let game_a = a
        .store
        .sync_mapped_game(b.engine.device(), &game_b)
        .await
        .unwrap()
        .unwrap();
    a.store
        .edit_game(&game_a, &set("newer, from a"), None)
        .await
        .unwrap();
    a.engine.capture().await.unwrap();
    settle(&[&a, &b]).await;
    for (device, game) in [(&a, &game_a), (&b, &game_b)] {
        let shown = device.store.game(game).await.unwrap().unwrap();
        assert_eq!(shown.description, "newer, from a");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn compaction_keeps_only_unseen_and_live_changes() {
    let (a, b, _c) = three().await;
    settle(&[&a, &b]).await;
    pair(&a, &b).await;
    a.store
        .set_sync_peer_seen(b.engine.device(), &b.engine.seen().await.unwrap())
        .await
        .unwrap();
    // What B already has from the first exchange may go now.
    a.engine.compact().await.unwrap();
    let game = games(&a).await[0].clone();
    for round in 0..30 {
        let edit = GameEdit {
            description: Some(TextEdit::Set {
                value: format!("take {round}"),
            }),
            ..GameEdit::default()
        };
        a.store.edit_game(&game, &edit, None).await.unwrap();
        a.engine.capture().await.unwrap();
    }
    let before = a.store.sync_log_len().await.unwrap();

    // B hasn't seen them: nothing it needs goes.
    a.engine.compact().await.unwrap();
    assert_eq!(
        a.store.sync_log_len().await.unwrap(),
        before,
        "B hasn't seen them"
    );

    // Once B has, the replaced values go; the live ones stay.
    send(&a, &b, 1000).await;
    a.store
        .set_sync_peer_seen(b.engine.device(), &b.engine.seen().await.unwrap())
        .await
        .unwrap();
    a.engine.compact().await.unwrap();
    let after = a.store.sync_log_len().await.unwrap();
    let live = a.store.sync_fields().await.unwrap().len() as i64;
    assert!(
        after <= live + 8,
        "{after} changes left for {live} live fields"
    );
    assert!(after < before - 25, "{before} → {after}");

    // A device paired later still gets the whole current state.
    let d = device('d').await;
    for (index, title) in TITLES.iter().enumerate() {
        discovered(&d, title, 100 + index as u64).await;
    }
    send(&a, &d, 1000).await;
    let game_d = d
        .store
        .sync_mapped_game(a.engine.device(), &game)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        d.store.game(&game_d).await.unwrap().unwrap().description,
        "take 29"
    );
}

/// Records `b` as paired with `a`, as #226 does.
async fn pair(a: &Device, b: &Device) {
    a.store
        .save_paired_device(&caskade_core::db::PairedDeviceRecord {
            endpoint_id: b.engine.device().to_owned(),
            name: "B".into(),
            kind: "desktop".into(),
            os: "linux".into(),
            addresses: vec![],
            wol_macs: vec![],
            last_sync_ms: None,
            grant_in: "0".repeat(64),
            grant_out: "0".repeat(64),
            paired_at_ms: 0,
            forgotten_at_ms: None,
        })
        .await
        .unwrap();
}
