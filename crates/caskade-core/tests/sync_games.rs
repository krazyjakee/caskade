//! The same game on different devices (#229): two libraries built
//! differently, as discovery on two machines would, recognise each other's
//! games by store id, IGDB id or title, and keep that match through renames,
//! merges and games kept apart.

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::model::{ExternalId, Game, GameEdit, GameEntry, PlaySession, TextEdit};
use caskade_core::sync::SyncEngine;

struct Device {
    engine: SyncEngine,
    store: LibraryStore,
}

async fn device(name: char) -> Device {
    let store = LibraryStore::in_memory().await.unwrap();
    AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    Device {
        engine: SyncEngine::new(store.clone(), name.to_string().repeat(64)),
        store,
    }
}

async fn send(from: &Device, to: &Device) {
    loop {
        let cursor = to.engine.seen().await.unwrap();
        let changes = from.engine.changes_for(&cursor, 100).await.unwrap();
        if changes.is_empty() {
            break;
        }
        to.engine.receive(&changes).await.unwrap();
    }
}

async fn sync(a: &Device, b: &Device) {
    for _ in 0..3 {
        a.engine.capture().await.unwrap();
        b.engine.capture().await.unwrap();
        send(a, b).await;
        send(b, a).await;
    }
}

/// A game with one entry per `(source, external id, store id)`.
async fn game(
    device: &Device,
    title: &str,
    entries: &[(&str, &str, ExternalId)],
) -> (String, Vec<String>) {
    let mut game = Game::new(title);
    game.release_date = Some("2016-05-13".into());
    game.platforms = vec!["windows".into()];
    let game = device.store.upsert_game(&game).await.unwrap();
    let mut ids = Vec::new();
    for (source, external, id) in entries {
        let mut entry = GameEntry::new(&game.id, source.to_string().into(), *external);
        entry.ids = vec![id.clone()];
        let entry = device.store.upsert_entry(&entry).await.unwrap();
        ids.push(entry.id);
    }
    (game.id, ids)
}

async fn played(device: &Device, game: &str, entry: &str, started: i64, minutes: u64) -> String {
    let mut session = PlaySession::new(game, entry, started);
    session.ended_at_ms = Some(started + (minutes * 60_000) as i64);
    session.duration_ms = minutes * 60_000;
    session.heartbeat_at_ms = started + (minutes * 60_000) as i64;
    device.store.upsert_session(&session).await.unwrap().id
}

async fn all_games(device: &Device) -> Vec<Game> {
    let snapshot = device.store.sync_snapshot().await.unwrap();
    let mut games = Vec::new();
    for id in snapshot.games {
        games.push(device.store.game(&id).await.unwrap().unwrap());
    }
    games.sort_by(|a, b| a.id.cmp(&b.id));
    games
}

#[tokio::test(flavor = "multi_thread")]
async fn the_same_steam_game_is_one_game_with_both_devices_sessions() {
    let desk = device('a').await;
    let laptop = device('b').await;
    // Discovered on each: different titles, different extra releases.
    let steam = ExternalId::Steam { app_id: 379_720 };
    let (desk_doom, desk_entries) =
        game(&desk, "DOOM", &[("steam", "379720", steam.clone())]).await;
    let (laptop_doom, laptop_entries) = game(
        &laptop,
        "Doom (2016)",
        &[
            ("steam", "379720", steam.clone()),
            (
                "gog",
                "1390579243",
                ExternalId::Gog {
                    product_id: 1_390_579_243,
                },
            ),
        ],
    )
    .await;
    let desk_session = played(&desk, &desk_doom, &desk_entries[0], 1_000_000, 30).await;
    let laptop_session = played(&laptop, &laptop_doom, &laptop_entries[0], 2_000_000, 45).await;
    // And a game only the desk has.
    game(
        &desk,
        "Celeste",
        &[("steam", "504230", ExternalId::Steam { app_id: 504_230 })],
    )
    .await;

    sync(&desk, &laptop).await;

    // One Doom on each, holding both sessions.
    let laptop_games = all_games(&laptop).await;
    let dooms: Vec<&Game> = laptop_games
        .iter()
        .filter(|g| g.title.contains("oom"))
        .collect();
    assert_eq!(dooms.len(), 1, "{laptop_games:?}");
    assert_eq!(dooms[0].id, laptop_doom);
    let sessions = laptop
        .store
        .sessions_for_game(&laptop_doom, 10)
        .await
        .unwrap();
    let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&desk_session.as_str()) && ids.contains(&laptop_session.as_str()));
    let desk_sessions = desk.store.sessions_for_game(&desk_doom, 10).await.unwrap();
    assert_eq!(desk_sessions.len(), 2);
    assert_eq!(
        all_games(&desk)
            .await
            .iter()
            .filter(|g| g.title.contains("OOM") || g.title.contains("oom"))
            .count(),
        1
    );
    // A desk-only game arrives as a title-only game, its session list whole.
    let celeste: Vec<&Game> = laptop_games
        .iter()
        .filter(|g| g.title == "Celeste")
        .collect();
    assert_eq!(celeste.len(), 1);
    assert_eq!(
        laptop
            .store
            .sync_mapped_game(desk.engine.device(), &desk_doom)
            .await
            .unwrap(),
        Some(laptop_doom.clone())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_kept_apart_on_one_device_stays_apart_on_the_other() {
    let desk = device('a').await;
    let laptop = device('b').await;
    let steam = ExternalId::Steam { app_id: 2280 };
    let gog = ExternalId::Gog {
        product_id: 1_440_164_514,
    };
    // Both have one Doom with a Steam and a GOG release (merged).
    let (_, desk_entries) = game(
        &desk,
        "Ultimate Doom",
        &[
            ("steam", "2280", steam.clone()),
            ("gog", "1440164514", gog.clone()),
        ],
    )
    .await;
    let (laptop_doom, laptop_entries) = game(
        &laptop,
        "The Ultimate DOOM",
        &[
            ("steam", "2280", steam.clone()),
            ("gog", "1440164514", gog.clone()),
        ],
    )
    .await;
    sync(&desk, &laptop).await;
    assert_eq!(all_games(&laptop).await.len(), 1);

    // On the desk, the GOG release is separated into a game of its own.
    let platforms = vec!["windows".to_owned()];
    let separated = desk
        .store
        .separate_entry(&desk_entries[1], &platforms, &platforms)
        .await
        .unwrap();
    sync(&desk, &laptop).await;

    // The laptop separates the same release, and keeps it apart.
    let games = all_games(&laptop).await;
    assert_eq!(games.len(), 2, "{games:?}");
    let gog_game = laptop
        .store
        .sync_mapped_game(desk.engine.device(), &separated.id)
        .await
        .unwrap()
        .expect("the separated game is matched");
    assert_ne!(gog_game, laptop_doom);
    let entries = laptop
        .store
        .sync_game_entry_stores(&gog_game)
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, laptop_entries[1]);
    let snapshot = laptop.store.sync_snapshot().await.unwrap();
    assert!(snapshot.values.contains_key(&(
        caskade_core::sync::change::Entity::KeptApart,
        gog_game.clone(),
        String::new()
    )));
    // And nothing merges them back on another round.
    sync(&desk, &laptop).await;
    assert_eq!(all_games(&laptop).await.len(), 2);
    assert_eq!(all_games(&desk).await.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_game_doesnt_break_the_match() {
    let desk = device('a').await;
    let laptop = device('b').await;
    // Matched by title, platform and year: no store ids in common.
    let (desk_game, _) = game(&desk, "Hollow Knight", &[]).await;
    let (laptop_game, _) = game(&laptop, "Hollow Knight", &[]).await;
    sync(&desk, &laptop).await;
    assert_eq!(all_games(&laptop).await.len(), 1);

    let rename = GameEdit {
        title: Some(TextEdit::Set { value: "HK".into() }),
        ..GameEdit::default()
    };
    desk.store
        .edit_game(&desk_game, &rename, None)
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    let shown = laptop.store.game(&laptop_game).await.unwrap().unwrap();
    assert_eq!(shown.title, "HK", "the rename arrived on the same game");
    assert_eq!(all_games(&laptop).await.len(), 1);

    // Later changes still find it.
    desk.store
        .set_game_favorite(&desk_game, true)
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    assert!(
        laptop
            .store
            .game(&laptop_game)
            .await
            .unwrap()
            .unwrap()
            .favorite
    );
    // And the laptop's own changes reach the desk's game.
    laptop
        .store
        .set_game_hidden(&laptop_game, true)
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    assert!(desk.store.game(&desk_game).await.unwrap().unwrap().hidden);
    assert_eq!(all_games(&desk).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn machine_specific_ids_never_match_games() {
    let desk = device('a').await;
    let laptop = device('b').await;
    // A shortcut's computed id comes from local paths: the same number on
    // two machines is two different programs. Manual entries' ids likewise.
    let shortcut = ExternalId::SteamShortcut {
        app_id: 3_000_000_001,
    };
    game(
        &desk,
        "Emulator Frontend",
        &[("steam", "shortcut-1", shortcut.clone())],
    )
    .await;
    game(
        &laptop,
        "Photo Editor",
        &[("steam", "shortcut-1", shortcut)],
    )
    .await;
    game(
        &desk,
        "Homebrew",
        &[("manual", "a", ExternalId::Steam { app_id: 42 })],
    )
    .await;
    game(
        &laptop,
        "Something Else",
        &[("manual", "b", ExternalId::Steam { app_id: 42 })],
    )
    .await;
    sync(&desk, &laptop).await;
    let titles: Vec<String> = all_games(&laptop)
        .await
        .into_iter()
        .map(|g| g.title)
        .collect();
    for title in [
        "Emulator Frontend",
        "Photo Editor",
        "Homebrew",
        "Something Else",
    ] {
        assert!(titles.iter().any(|t| t == title), "{title} in {titles:?}");
    }
}
