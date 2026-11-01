//! The Playnite milestone's editing scenario (#194), end to end over one
//! library file: edit a game and its artwork, bulk-edit a selection,
//! refresh the selection's details from IGDB (a fake metadata server), let
//! discovery import the games again, then restart. Manual values, chosen
//! artwork, sessions, a managed install's path identity and collections
//! must all come through.

use caskade_core::{
    accounts::AccountService,
    artwork::ArtworkCache,
    bus::Bus,
    db::LibraryStore,
    emulation::Catalog,
    hasheous::Hasheous,
    igdb::Igdb,
    metadata::{Metadata, RefreshMode},
    model::{
        ArtworkKind, CollectionKind, ExternalId, Game, GameEdit, GameEntry, InstallLocation,
        ListEdit, NotesEdit, PlaySession, RatingEdit, SaveCollection, SourceId, StorageRoot,
        TextEdit,
    },
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path, path_regex},
};

/// An 8×8 PNG, for covers.
fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// A metadata server that knows Steam apps 400 and 620 as IGDB games 71
/// (Portal) and 72 (Portal 2), with covers.
async fn igdb() -> MockServer {
    let server = MockServer::start().await;
    let base = server.uri();
    Mock::given(method("POST"))
        .and(path("/match"))
        .respond_with(|request: &Request| {
            let ids: Vec<Value> = serde_json::from_slice(&request.body).unwrap();
            let matches: Vec<Value> = ids
                .iter()
                .filter_map(|id| {
                    let game = match id["uid"].as_str()? {
                        "400" => 71,
                        "620" => 72,
                        _ => return None,
                    };
                    Some(json!({"store": id["store"], "uid": id["uid"], "game": game}))
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"matches": matches, "notFound": []}))
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(move |request: &Request| {
            let ids: Vec<u64> = serde_json::from_slice(&request.body).unwrap();
            let games: Vec<Value> = ids
                .iter()
                .map(|id| {
                    let title = if *id == 71 { "Portal" } else { "Portal 2" };
                    let cover = format!("co{id}");
                    json!({
                        "id": id, "title": title,
                        "url": format!("https://www.igdb.com/games/{id}"),
                        "description": format!("{title} from IGDB."),
                        "genres": ["Puzzle"], "tags": ["Science fiction"],
                        "releaseDate": "2011-04-18", "aliases": [title],
                        "images": {"cover": {"imageId": cover, "width": 8, "height": 8,
                            "url": format!("{base}/img/{cover}.png")}},
                    })
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"games": games, "notFound": []}))
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

/// What discovery proposes for a Steam game: a fresh game record and an
/// entry with fresh ids, identified by its source and external id.
fn steam(title: &str, app: u32, dir: &Path) -> (Game, GameEntry) {
    let game = Game::new(title);
    let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), app.to_string());
    entry.ids = vec![ExternalId::Steam { app_id: app }];
    entry.location = Some(InstallLocation::External {
        path: dir.join(title),
    });
    (game, entry)
}

struct Library {
    store: LibraryStore,
    account: String,
}

async fn open(file: &Path) -> Library {
    let store = LibraryStore::open(file).await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let account = accounts.active_id().expect("the first account");
    Library { store, account }
}

/// Everything the scenario must keep, read back from the library.
async fn check(
    library: &Library,
    ids: &Ids,
    collection: &str,
    expected_sessions: usize,
    when: &str,
) {
    let store = &library.store;
    let portal = store.game(&ids.portal).await.unwrap().unwrap();
    assert_eq!(portal.title, "Portal (my copy)", "{when}: title");
    assert_eq!(
        portal.description, "Mine, not IGDB's.",
        "{when}: description"
    );
    assert_eq!(portal.rating, Some(90), "{when}: rating");
    assert_eq!(portal.notes, "Finish the commentary run.", "{when}: notes");
    // The bulk edit's tag, beside whatever automatic tags came.
    assert!(
        portal.tags.contains(&"Couch".to_owned()),
        "{when}: tags {:?}",
        portal.tags
    );
    let portal2 = store.game(&ids.portal2).await.unwrap().unwrap();
    assert!(
        portal2.tags.contains(&"Couch".to_owned()),
        "{when}: bulk tag"
    );
    // The chosen cover is still the user's, whatever IGDB brought.
    let covers: Vec<_> = store
        .shown_artwork(&ids.portal)
        .await
        .unwrap()
        .into_iter()
        .filter(|art| art.kind == ArtworkKind::Cover)
        .collect();
    assert_eq!(covers.len(), 1, "{when}: one cover shown");
    assert_eq!(covers[0].source, "user", "{when}: the user's cover");
    // The session, and the play time it counted.
    assert_eq!(
        store
            .sessions_for_game(&ids.portal, 10)
            .await
            .unwrap()
            .len(),
        expected_sessions,
        "{when}: sessions"
    );
    // The managed install keeps its root and folder; the entries keep their ids.
    let managed = store.get_entry(&ids.managed).await.unwrap().unwrap();
    assert_eq!(managed.game_id, ids.portal2, "{when}: managed entry's game");
    assert_eq!(
        managed.location,
        Some(InstallLocation::Managed {
            root_id: "milestone-root".into(),
            rel_dir: "Portal 2".into(),
        }),
        "{when}: managed path identity"
    );
    let steam = store.get_entry(&ids.portal_entry).await.unwrap().unwrap();
    assert_eq!(steam.game_id, ids.portal, "{when}: steam entry's game");
    // The collection still holds both games.
    let members = store
        .collection_games(&library.account, collection, 0, 50)
        .await
        .unwrap();
    let mut members: Vec<String> = members.items.into_iter().map(|game| game.id).collect();
    members.sort();
    let mut expected = vec![ids.portal.clone(), ids.portal2.clone()];
    expected.sort();
    assert_eq!(members, expected, "{when}: collection");
}

struct Ids {
    portal: String,
    portal2: String,
    portal_entry: String,
    managed: String,
}

#[tokio::test]
async fn manual_values_survive_bulk_edits_refreshes_rescans_and_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("library.sqlite");
    let installs = dir.path().join("steamapps");
    let library = open(&file).await;
    let store = &library.store;

    // Discovery finds two Steam games; Caskade installed another copy of
    // Portal 2 into its own storage root.
    let (game, entry) = steam("Portal", 400, &installs);
    let portal = store.import_group(&game, &[entry], None).await.unwrap();
    let (game, entry) = steam("Portal 2", 620, &installs);
    let portal2 = store.import_group(&game, &[entry], None).await.unwrap();
    store
        .upsert_root(&StorageRoot {
            id: "milestone-root".into(),
            path: dir.path().join("games"),
            is_default: true,
            label: None,
        })
        .await
        .unwrap();
    let mut managed = GameEntry::new(&portal2.game_id, SourceId::new("caskade-install"), "p2");
    managed.location = Some(InstallLocation::Managed {
        root_id: "milestone-root".into(),
        rel_dir: "Portal 2".into(),
    });
    let managed = store.upsert_entry(&managed).await.unwrap();
    let ids = Ids {
        portal: portal.game_id.clone(),
        portal2: portal2.game_id.clone(),
        portal_entry: portal.entry_ids[0].clone(),
        managed: managed.id.clone(),
    };

    // A finished session, and a collection of both.
    let session = store
        .start_session(&PlaySession::new(&ids.portal, &ids.portal_entry, 1_000))
        .await
        .unwrap();
    store
        .finish_session(&session.id, 3_601_000, 3_600_000)
        .await
        .unwrap();
    let collection = store
        .save_collection(
            &library.account,
            &SaveCollection {
                id: None,
                name: "Couch co-op".into(),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    store
        .add_to_collection(
            &library.account,
            &collection.id,
            &[ids.portal.clone(), ids.portal2.clone()],
        )
        .await
        .unwrap();

    // The editor: title, description, rating and notes, and an imported cover.
    let edit = GameEdit {
        title: Some(TextEdit::Set {
            value: "Portal (my copy)".into(),
        }),
        description: Some(TextEdit::Set {
            value: "Mine, not IGDB's.".into(),
        }),
        rating: Some(RatingEdit { value: Some(90) }),
        notes: Some(NotesEdit {
            value: "Finish the commentary run.".into(),
        }),
        ..Default::default()
    };
    store.edit_game(&ids.portal, &edit, None).await.unwrap();
    let cover = dir.path().join("cover.png");
    std::fs::write(&cover, png()).unwrap();
    let cache = ArtworkCache::new(dir.path().join("artwork"))
        .unwrap()
        .allowing_local_metadata();
    store
        .import_user_artwork(&cache, &ids.portal, ArtworkKind::Cover, &cover)
        .await
        .unwrap();

    // A bulk edit over the selection, previewed then applied.
    let bulk = GameEdit {
        tags: Some(ListEdit::Add {
            values: vec!["Couch".into()],
        }),
        ..Default::default()
    };
    let selection = vec![ids.portal.clone(), ids.portal2.clone()];
    let preview = store
        .bulk_edit_games(&selection, &bulk, false)
        .await
        .unwrap();
    assert_eq!(preview.len(), 2);
    assert!(
        store
            .game(&ids.portal)
            .await
            .unwrap()
            .unwrap()
            .tags
            .is_empty()
    );
    store
        .bulk_edit_games(&selection, &bulk, true)
        .await
        .unwrap();

    // Refresh the selection's details, automatic ones fetched again, through
    // the same matching and applies as the background runs.
    let server = igdb().await;
    let metadata = Metadata::new(
        store.clone(),
        Bus::default(),
        Igdb::with_base_url(dir.path().join("igdb"), server.uri()),
        Hasheous::with_base_url(dir.path().join("hasheous"), server.uri()),
        Arc::new(Catalog::default()),
        Some(cache.clone()),
    );
    let status = metadata
        .refresh_games(
            &selection,
            RefreshMode::Automatic,
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    assert_eq!(status.updated, 2, "{status:?}");
    // The refresh filled what the user didn't set.
    let portal2_game = store.game(&ids.portal2).await.unwrap().unwrap();
    assert_eq!(portal2_game.description, "Portal 2 from IGDB.");
    assert!(portal2_game.genres.contains(&"Puzzle".to_owned()));
    check(&library, &ids, &collection.id, 1, "after the refresh").await;

    // Discovery imports both again, as a rescan does: fresh records with
    // other titles, the same source identities.
    let (game, entry) = steam("PORTAL", 400, &installs);
    let again = store.import_group(&game, &[entry], None).await.unwrap();
    assert_eq!(
        (again.game_id.as_str(), again.created),
        (ids.portal.as_str(), false)
    );
    let (game, entry) = steam("Portal 2 (Steam)", 620, &installs);
    store.import_group(&game, &[entry], None).await.unwrap();
    check(&library, &ids, &collection.id, 1, "after a rescan").await;

    // Restart: the library is opened again from its file.
    drop(metadata);
    drop(library);
    let library = open(&file).await;
    check(&library, &ids, &collection.id, 1, "after a restart").await;
    // And a refresh after the restart still keeps them.
    let metadata = Metadata::new(
        library.store.clone(),
        Bus::default(),
        Igdb::with_base_url(dir.path().join("igdb"), server.uri()),
        Hasheous::with_base_url(dir.path().join("hasheous"), server.uri()),
        Arc::new(Catalog::default()),
        Some(cache),
    );
    metadata
        .refresh_games(
            &selection,
            RefreshMode::Automatic,
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    check(
        &library,
        &ids,
        &collection.id,
        1,
        "after a refresh after the restart",
    )
    .await;
}
