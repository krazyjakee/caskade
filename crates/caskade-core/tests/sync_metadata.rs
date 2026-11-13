//! Games that came from another device are filled in here (#75): automatic
//! metadata and artwork stay on each device, so the receiving one matches
//! them by the store ids their identities carry, applies hand-set matches,
//! and starts as soon as they arrive rather than at its next hourly check.

use crate::engines::{Device, device, exchange};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use caskade_core::artwork::ArtworkCache;
use caskade_core::bus::Bus;
use caskade_core::emulation::Catalog;
use caskade_core::hasheous::Hasheous;
use caskade_core::igdb::{Igdb, StoreId};
use caskade_core::metadata::{IgdbMatch, MatchSource, Metadata};
use caskade_core::model::{ArtworkKind, ExternalId, Game, GameEntry};
use serde_json::{Value, json};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// A game with one entry carrying `ids`.
async fn game(device: &Device, title: &str, ids: Vec<ExternalId>) -> String {
    let mut game = Game::new(title);
    game.platforms = vec!["windows".into()];
    let game = device.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), title);
    entry.ids = ids;
    device.store.upsert_entry(&entry).await.unwrap();
    game.id
}

async fn only_game(device: &Device) -> Game {
    let snapshot = device.store.sync_snapshot().await.unwrap();
    let ids: Vec<_> = snapshot.games.into_iter().collect();
    assert_eq!(ids.len(), 1, "one game came over");
    device.store.game(&ids[0]).await.unwrap().unwrap()
}

fn png() -> Vec<u8> {
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255])));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// A metadata server that knows Steam app 620 as IGDB game 72 (Portal 2),
/// has game 1942 (The Witcher 3), and serves every image.
async fn server() -> MockServer {
    let server = MockServer::start().await;
    let base = server.uri();
    Mock::given(method("POST"))
        .and(path("/match"))
        .respond_with(|request: &Request| {
            let ids: Vec<StoreId> = serde_json::from_slice(&request.body).unwrap();
            let matches: Vec<Value> = ids
                .iter()
                .filter(|id| id.store == "steam" && id.uid == "620")
                .map(|id| json!({"store": id.store, "uid": id.uid, "game": 72}))
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"matches": matches, "notFound": []}))
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(move |request: &Request| {
            let ids: Vec<u64> = serde_json::from_slice(&request.body).unwrap();
            let title = |id: u64| match id {
                72 => Some("Portal 2"),
                1942 => Some("The Witcher 3"),
                _ => None,
            };
            let games: Vec<Value> = ids
                .iter()
                .filter_map(|id| {
                    let title = title(*id)?;
                    let cover = format!("co{id}");
                    Some(json!({
                        "id": id,
                        "title": title,
                        "description": format!("{title} from IGDB."),
                        "genres": ["Adventure"],
                        "images": {"cover": {
                            "imageId": cover, "width": 8, "height": 8,
                            "url": format!("{base}/img/{cover}.png"),
                        }},
                    }))
                })
                .collect();
            let not_found: Vec<u64> = ids.into_iter().filter(|id| title(*id).is_none()).collect();
            ResponseTemplate::new(200).set_body_json(json!({"games": games, "notFound": not_found}))
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex("^/img/"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png()))
        .mount(&server)
        .await;
    server
}

/// The receiving device's metadata service, against `server`.
fn metadata(device: &Device, dir: &std::path::Path, server: &MockServer) -> Metadata {
    Metadata::new(
        device.store.clone(),
        Bus::default(),
        Igdb::with_base_url(dir.join("igdb"), server.uri()),
        Hasheous::with_base_url(dir.join("hasheous"), server.uri()),
        Arc::new(Catalog::default()),
        Some(
            ArtworkCache::new(dir.join("artwork"))
                .unwrap()
                .allowing_local_metadata(),
        ),
    )
}

async fn covers(device: &Device, game: &str) -> Vec<String> {
    device
        .store
        .artwork_for_game(game)
        .await
        .unwrap()
        .into_iter()
        .filter(|art| art.kind == ArtworkKind::Cover)
        .map(|art| art.source)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_from_another_device_is_matched_by_its_store_ids_and_filled_in() {
    let server = server().await;
    let dir = tempfile::tempdir().unwrap();
    let desk = device('a').await;
    let laptop = device('b').await;
    game(&desk, "Portal 2", vec![ExternalId::Steam { app_id: 620 }]).await;

    exchange(&desk.engine, &laptop.engine).await;
    // A title only: its entries, metadata and artwork stayed on the desk.
    let portal = only_game(&laptop).await;
    assert!(portal.description.is_empty());
    assert!(
        laptop
            .store
            .entries_for_game(&portal.id)
            .await
            .unwrap()
            .is_empty()
    );

    let report = metadata(&laptop, dir.path(), &server).run().await.unwrap();
    assert_eq!((report.matched, report.applied), (1, 1), "{report:?}");
    let portal = laptop.store.game(&portal.id).await.unwrap().unwrap();
    assert_eq!(portal.description, "Portal 2 from IGDB.");
    assert_eq!(covers(&laptop, &portal.id).await, ["igdb:co72"]);
    let found = metadata(&laptop, dir.path(), &server)
        .game(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (found.igdb_id, found.source, found.pending),
        (Some(72), MatchSource::Store, false)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_match_set_by_hand_on_another_device_is_filled_in_here() {
    let server = server().await;
    let dir = tempfile::tempdir().unwrap();
    let desk = device('a').await;
    let laptop = device('b').await;
    let arrivals = Arc::new(AtomicUsize::new(0));
    laptop.engine.on_games_arrived({
        let arrivals = arrivals.clone();
        move || {
            arrivals.fetch_add(1, Ordering::SeqCst);
        }
    });
    // No store id IGDB knows: only the user's choice names its game.
    let witcher = game(&desk, "Wild Hunt", vec![]).await;
    desk.store
        .save_igdb_match(&IgdbMatch {
            game_id: witcher,
            igdb_id: Some(1942),
            source: MatchSource::Manual,
            pending: false,
            checked_at_ms: 1,
            dumps: String::new(),
            applied: Default::default(),
        })
        .await
        .unwrap();

    exchange(&desk.engine, &laptop.engine).await;
    assert!(
        arrivals.load(Ordering::SeqCst) > 0,
        "the receiving device is told games arrived"
    );
    let arrived = arrivals.load(Ordering::SeqCst);
    // Nothing new: nobody is told again.
    exchange(&desk.engine, &laptop.engine).await;
    assert_eq!(arrivals.load(Ordering::SeqCst), arrived);

    let witcher = only_game(&laptop).await;
    let report = metadata(&laptop, dir.path(), &server).run().await.unwrap();
    assert_eq!(report.applied, 1, "{report:?}");
    let witcher = laptop.store.game(&witcher.id).await.unwrap().unwrap();
    assert_eq!(witcher.description, "The Witcher 3 from IGDB.");
    assert_eq!(covers(&laptop, &witcher.id).await, ["igdb:co1942"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn games_arriving_start_matching_without_waiting_for_the_hourly_check() {
    let server = server().await;
    let dir = tempfile::tempdir().unwrap();
    let desk = device('a').await;
    let laptop = device('b').await;
    let metadata = metadata(&laptop, dir.path(), &server);
    laptop.engine.on_games_arrived({
        let metadata = metadata.clone();
        move || metadata.wake()
    });
    // Its first run, over an empty library, is over before games come.
    let watch = metadata.watch();
    tokio::time::sleep(Duration::from_millis(500)).await;

    game(&desk, "Portal 2", vec![ExternalId::Steam { app_id: 620 }]).await;
    exchange(&desk.engine, &laptop.engine).await;
    let portal = only_game(&laptop).await;
    let filled = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let game = laptop.store.game(&portal.id).await.unwrap().unwrap();
            if !game.description.is_empty() {
                return game;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the synced game is filled in soon after it arrives");
    watch.abort();
    assert_eq!(filled.description, "Portal 2 from IGDB.");
}

/// Saves the automatic match the sending device found: by its RomM server,
/// say, for a ROM with no store id.
async fn matched_there(device: &Device, game_id: &str, igdb_id: u64) {
    device
        .store
        .save_igdb_match(&IgdbMatch {
            game_id: game_id.to_owned(),
            igdb_id: Some(igdb_id),
            source: MatchSource::Romm,
            pending: false,
            checked_at_ms: 1,
            dumps: String::new(),
            applied: Default::default(),
        })
        .await
        .unwrap();
}

async fn match_requests(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/match")
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_matched_without_a_store_id_takes_its_devices_igdb_id() {
    let server = server().await;
    let dir = tempfile::tempdir().unwrap();
    let desk = device('a').await;
    let laptop = device('b').await;
    // A ROM the desk's RomM server matched: no store id on either device.
    let witcher = game(&desk, "The Witcher 3", vec![]).await;
    matched_there(&desk, &witcher, 1942).await;

    exchange(&desk.engine, &laptop.engine).await;
    let synced = only_game(&laptop).await;
    assert!(synced.description.is_empty());

    let report = metadata(&laptop, dir.path(), &server).run().await.unwrap();
    assert_eq!((report.matched, report.applied), (1, 1), "{report:?}");
    let synced = laptop.store.game(&synced.id).await.unwrap().unwrap();
    assert_eq!(synced.description, "The Witcher 3 from IGDB.");
    assert_eq!(covers(&laptop, &synced.id).await, ["igdb:co1942"]);
    let row = laptop.store.igdb_match(&synced.id).await.unwrap().unwrap();
    assert_eq!((row.igdb_id, row.source), (Some(1942), MatchSource::Device));
    // The desk's match was the answer: nothing to ask the server to match.
    assert_eq!(match_requests(&server).await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn store_ids_here_are_asked_about_before_the_other_devices_match() {
    let server = server().await;
    let dir = tempfile::tempdir().unwrap();
    let desk = device('a').await;
    let laptop = device('b').await;
    let there = game(&desk, "Portal 2", vec![]).await;
    matched_there(&desk, &there, 1942).await;
    // The same game on the laptop, from Steam.
    let here = game(&laptop, "Portal 2", vec![ExternalId::Steam { app_id: 620 }]).await;

    exchange(&desk.engine, &laptop.engine).await;
    assert_eq!(only_game(&laptop).await.id, here, "the two are one game");

    let report = metadata(&laptop, dir.path(), &server).run().await.unwrap();
    assert_eq!(report.matched, 1, "{report:?}");
    let row = laptop.store.igdb_match(&here).await.unwrap().unwrap();
    assert_eq!((row.igdb_id, row.source), (Some(72), MatchSource::Store));
    assert_eq!(match_requests(&server).await, 1);
}
