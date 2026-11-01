use super::watch::{MAX_OFFLINE_RETRY, MAX_RETRY, retry_delay};
use super::*;
use crate::{
    hasheous::WINDOWS,
    igdb::StoreId,
    model::{ArtworkKind, Game, GameDetails, GameEdit, GameEntry, ListEdit, TextEdit},
};
use caskade_discovery::{ExternalId, SourceId};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::{Value, json};
use std::io::Write;
use tokio_util::sync::CancellationToken;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path, path_regex},
};

fn png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255])));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// IGDB game `id` as the metadata server sends it, with images on `base`.
fn igdb_game(base: &str, id: u64, title: &str) -> Value {
    let image = |image_id: &str| json!({"imageId": image_id, "width": 8, "height": 8, "url": format!("{base}/img/{image_id}.png")});
    let artwork = |image_id: &str, kind: u64, size: u32| json!({"imageId": image_id, "width": size, "height": size, "imageType": kind, "url": format!("{base}/img/{image_id}.png")});
    // Portal 2 is rated; the others come as from a server before ratings.
    let mut game = json!({
        "id": id,
        "title": title,
        "url": format!("https://www.igdb.com/games/{id}"),
        "description": format!("{title} from IGDB."),
        "genres": ["Puzzle", " ", "Puzzle"],
        "tags": ["Science fiction", "Single player"],
        "releaseDate": "2011-04-18",
        "aliases": [title, "P2"],
        "totalRating": 91.5,
        "images": {
            "cover": image(&format!("co{id}")),
            "hero": image(&format!("ar{id}")),
            // Type 13 is an icon; the larger of two wins. Type 2 is key art.
            "artworks": [
                artwork(&format!("ic{id}"), 13, 128),
                artwork(&format!("is{id}"), 13, 64),
                artwork(&format!("ka{id}"), 2, 1920),
            ],
            "screenshots": [image(&format!("sc{id}a")), image(&format!("sc{id}b"))],
        },
    });
    if id == 72 {
        game["ageRatings"] =
            json!([{"board": "pegi", "rating": "12"}, {"board": "esrb", "rating": "E10+"}]);
    }
    game
}

fn rating(board: &str, rating: &str) -> IgdbAgeRating {
    IgdbAgeRating {
        board: board.to_owned(),
        rating: rating.to_owned(),
    }
}

/// A metadata server that knows Steam app 620 as IGDB game 72 and GOG
/// product 1 as game 1942, and serves every image.
async fn server() -> MockServer {
    let server = MockServer::start().await;
    let base = server.uri();
    Mock::given(method("POST"))
        .and(path("/match"))
        .respond_with(|request: &Request| {
            let ids: Vec<StoreId> = serde_json::from_slice(&request.body).unwrap();
            let matches: Vec<Value> = ids
                .iter()
                .filter_map(|id| {
                    let game = match (id.store.as_str(), id.uid.as_str()) {
                        ("steam", "620") => 72,
                        ("gog", "1") => 1942,
                        ("epic", "offer-crab") => 1942,
                        _ => return None,
                    };
                    Some(json!({"store": id.store, "uid": id.uid, "game": game}))
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
            let titles = HashMap::from([(72, "Portal 2"), (1942, "The Witcher 3"), (9331, "Worms")]);
            let (games, not_found): (Vec<u64>, Vec<u64>) =
                ids.into_iter().partition(|id| titles.contains_key(id));
            ResponseTemplate::new(200).set_body_json(json!({
                "games": games.iter().map(|id| igdb_game(&base, *id, titles[id])).collect::<Vec<_>>(),
                "notFound": not_found,
            }))
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

/// Serves a Windows dump with "Worms" (IGDB 9331) and indexes it.
async fn hasheous_with_worms(dir: &std::path::Path, server: &MockServer) -> Hasheous {
    let game = json!({
        "Id": 1, "ObjectType": "Game", "Name": "Worms",
        "Metadata": [
            {"Source": "IGDB", "Id": "9331", "Status": "Mapped", "Link": "https://www.igdb.com/games/worms"}
        ],
        "Attributes": []
    });
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file("Worms (1).json", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(game.to_string().as_bytes()).unwrap();
    let zip = writer.finish().unwrap().into_inner();
    use md5::Digest;
    let md5: String = md5::Md5::digest(&zip)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Mock::given(method("GET"))
        .and(path("/api/v1/Dumps/platforms"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "name": format!("{WINDOWS}.zip"), "sizeBytes": zip.len(), "md5hash": md5,
        }])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/Dumps/platforms/Microsoft%20Windows.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip))
        .mount(server)
        .await;
    let hasheous = Hasheous::with_base_url(dir, server.uri());
    let report = hasheous
        .sync(&std::collections::BTreeSet::from([WINDOWS.to_owned()]))
        .await
        .unwrap();
    assert_eq!(report.indexed, [WINDOWS]);
    hasheous
}

struct Fixture {
    store: LibraryStore,
    metadata: Metadata,
    server: MockServer,
    _dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let server = server().await;
        let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), server.uri());
        Self::with_hasheous(dir, server, hasheous).await
    }

    async fn with_hasheous(dir: tempfile::TempDir, server: MockServer, hasheous: Hasheous) -> Self {
        let store = LibraryStore::in_memory().await.unwrap();
        let mut metadata = Metadata::new(
            store.clone(),
            Bus::default(),
            Igdb::with_base_url(dir.path().join("igdb"), server.uri()),
            hasheous,
            Arc::new(Catalog::default()),
            // The images come from the mock server, on loopback.
            Some(
                ArtworkCache::new(dir.path().join("artwork"))
                    .unwrap()
                    .allowing_local_metadata(),
            ),
        );
        // Epic's catalogue is the mock server's too.
        Arc::get_mut(&mut metadata.inner).unwrap().epic = epic::EpicOffers::with_base_urls(
            &format!("{}/epic/account", server.uri()),
            &format!("{}/epic/catalog", server.uri()),
        );
        Self {
            store,
            metadata,
            server,
            _dir: dir,
        }
    }

    async fn add(&self, title: &str, ids: Vec<ExternalId>) -> Game {
        let game = self.store.upsert_game(&Game::new(title)).await.unwrap();
        let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), title);
        entry.ids = ids;
        self.store.upsert_entry(&entry).await.unwrap();
        game
    }

    async fn game(&self, id: &str) -> Game {
        self.store.game(id).await.unwrap().unwrap()
    }

    async fn artwork(&self, id: &str) -> Vec<(ArtworkKind, String)> {
        let mut art: Vec<_> = self
            .store
            .artwork_for_game(id)
            .await
            .unwrap()
            .into_iter()
            .map(|art| (art.kind, art.source))
            .collect();
        art.sort_by(|a, b| a.1.cmp(&b.1));
        art
    }

    async fn requests(&self, to: &str) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == to)
            .count()
    }
}

#[tokio::test]
async fn store_ids_match_and_fill_the_game_in_once() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let unknown = fixture
        .add("Unknown", vec![ExternalId::Steam { app_id: 9 }])
        .await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.matched, report.none, report.applied, report.deferred),
        (1, 1, 1, 0)
    );
    let game = fixture.game(&portal.id).await;
    assert_eq!(game.description, "Portal 2 from IGDB.");
    assert_eq!(game.genres, ["Puzzle"]);
    assert_eq!(game.tags, ["Science fiction", "Single player"]);
    assert_eq!(game.release_date.as_deref(), Some("2011-04-18"));
    // The title itself is no alias.
    assert_eq!(game.aliases, ["P2"]);
    // The library's images come now, the page's when it opens.
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [
            (ArtworkKind::Cover, "igdb:co72".to_owned()),
            (ArtworkKind::Icon, "igdb:ic72".to_owned()),
        ]
    );
    assert!(fixture.metadata.page_images(&portal.id).await.unwrap());
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [
            (ArtworkKind::Hero, "igdb:ar72".to_owned()),
            (ArtworkKind::Cover, "igdb:co72".to_owned()),
            (ArtworkKind::Icon, "igdb:ic72".to_owned()),
            (ArtworkKind::Screenshot, "igdb:sc72a".to_owned()),
            (ArtworkKind::Screenshot, "igdb:sc72b".to_owned()),
        ]
    );
    assert!(!fixture.metadata.page_images(&portal.id).await.unwrap());
    // Without a match there is nothing to fetch.
    assert!(!fixture.metadata.page_images(&unknown.id).await.unwrap());
    let found = fixture.metadata.game(&portal.id).await.unwrap().unwrap();
    assert_eq!(
        (found.igdb_id, found.source, found.pending),
        (Some(72), MatchSource::Store, false)
    );
    assert_eq!(found.game.unwrap().title, "Portal 2");
    let none = fixture.metadata.game(&unknown.id).await.unwrap().unwrap();
    assert_eq!((none.igdb_id, none.source), (None, MatchSource::None));
    assert!(fixture.game(&unknown.id).await.description.is_empty());

    // Nothing is left to do, so nothing is asked again.
    let requests = fixture.server.received_requests().await.unwrap().len();
    assert_eq!(fixture.metadata.run().await.unwrap(), RunReport::default());
    assert_eq!(
        fixture.server.received_requests().await.unwrap().len(),
        requests
    );
}

/// Epic's catalogue on `server`: namespace `crab` sells its game as
/// `offer-crab`, Epic has no namespace `gone`, and `offline` fails.
async fn epic_catalogue(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/epic/account/account/api/oauth/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token": "token", "expires_in": 14400})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/epic/catalog/catalog/api/shared/namespace/crab/offers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"elements": [
            {"id": "offer-crab", "categories": [{"path": "games"}, {"path": "games/edition/base"}]},
            {"id": "offer-nightly", "categories": [{"path": "games"}]},
        ]})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/epic/catalog/catalog/api/shared/namespace/gone/offers",
        ))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/epic/catalog/catalog/api/shared/namespace/offline/offers",
        ))
        .respond_with(ResponseTemplate::new(503))
        .mount(server)
        .await;
}

#[tokio::test]
async fn epic_games_match_by_their_store_offer() {
    let fixture = Fixture::new().await;
    epic_catalogue(&fixture.server).await;
    let epic = |namespace: &str| {
        vec![ExternalId::Epic {
            namespace: namespace.to_owned(),
            catalog_item: format!("item-{namespace}"),
            app_name: format!("app-{namespace}"),
        }]
    };
    let found = fixture.add("Satisfactory", epic("crab")).await;
    let gone = fixture.add("Gone", epic("gone")).await;
    let offline = fixture.add("Offline", epic("offline")).await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (
            report.matched,
            report.none,
            report.deferred,
            report.unreachable
        ),
        (1, 1, 1, false)
    );
    let matched = fixture.metadata.game(&found.id).await.unwrap().unwrap();
    assert_eq!(
        (matched.igdb_id, matched.source),
        (Some(1942), MatchSource::Store)
    );
    let none = fixture.metadata.game(&gone.id).await.unwrap().unwrap();
    assert_eq!((none.igdb_id, none.source), (None, MatchSource::None));
    // Not asked about with every id it has, so it waits for the next run.
    assert!(
        fixture
            .store
            .igdb_match(&offline.id)
            .await
            .unwrap()
            .is_none()
    );

    fixture.metadata.run().await.unwrap();
    let offers =
        |namespace: &str| format!("/epic/catalog/catalog/api/shared/namespace/{namespace}/offers");
    assert_eq!(
        (
            fixture.requests(&offers("crab")).await,
            fixture.requests(&offers("gone")).await,
            fixture.requests(&offers("offline")).await,
            fixture
                .requests("/epic/account/account/api/oauth/token")
                .await,
        ),
        (1, 1, 2, 1)
    );
}

#[tokio::test]
async fn hasheous_names_come_before_store_ids() {
    let dir = tempfile::tempdir().unwrap();
    let server = server().await;
    let hasheous = hasheous_with_worms(&dir.path().join("hasheous"), &server).await;
    let fixture = Fixture::with_hasheous(dir, server, hasheous).await;
    // Steam 620 would be Portal 2; the name wins.
    let worms = fixture
        .add("Worms™", vec![ExternalId::Steam { app_id: 620 }])
        .await;

    fixture.metadata.run().await.unwrap();
    let found = fixture.metadata.game(&worms.id).await.unwrap().unwrap();
    assert_eq!(
        (found.igdb_id, found.source),
        (Some(9331), MatchSource::Hasheous)
    );
    assert_eq!(fixture.requests("/match").await, 0);
    assert_eq!(
        fixture.game(&worms.id).await.description,
        "Worms from IGDB."
    );
}

#[tokio::test]
async fn the_games_own_values_and_artwork_stay() {
    let fixture = Fixture::new().await;
    let mut game = Game::new("Portal 2");
    game.description = "From the launcher.".into();
    game.release_date = Some("2011-04-19".into());
    let game = fixture.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), "620");
    entry.ids = vec![ExternalId::Steam { app_id: 620 }];
    fixture.store.upsert_entry(&entry).await.unwrap();
    let cache = ArtworkCache::new(fixture._dir.path().join("artwork")).unwrap();
    let cover = cache.import_bytes(png()).await.unwrap();
    fixture
        .store
        .upsert_artwork(&cover.record(&game.id, ArtworkKind::Cover, "steam"))
        .await
        .unwrap();

    fixture.metadata.run().await.unwrap();
    let filled = fixture.game(&game.id).await;
    assert_eq!(filled.description, "From the launcher.");
    assert_eq!(filled.release_date.as_deref(), Some("2011-04-19"));
    assert_eq!(filled.genres, ["Puzzle"]);
    fixture.metadata.page_images(&game.id).await.unwrap();
    let art = fixture.artwork(&game.id).await;
    assert!(art.contains(&(ArtworkKind::Cover, "steam".to_owned())));
    assert!(!art.iter().any(|(_, source)| source == "igdb:co72"));
    assert!(art.contains(&(ArtworkKind::Hero, "igdb:ar72".to_owned())));
}

#[tokio::test]
async fn fields_the_user_set_stay_and_reset_brings_igdbs_back() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let edit = GameEdit {
        description: Some(TextEdit::Set {
            value: String::new(),
        }),
        genres: Some(ListEdit::Set {
            values: vec!["Mine".into()],
        }),
        ..Default::default()
    };
    fixture
        .store
        .edit_game(&portal.id, &edit, None)
        .await
        .unwrap();

    fixture.metadata.run().await.unwrap();
    let state = fixture
        .store
        .game_edit_state(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.game.description, "");
    assert_eq!(state.game.genres, ["Mine"]);
    // What the user didn't set is IGDB's as before.
    assert_eq!(state.game.tags, ["Science fiction", "Single player"]);
    assert_eq!(state.automatic.description, "Portal 2 from IGDB.");
    assert_eq!(state.automatic.genres, ["Puzzle"]);

    let reset = GameEdit {
        description: Some(TextEdit::Reset),
        genres: Some(ListEdit::Reset),
        ..Default::default()
    };
    fixture
        .store
        .edit_game(&portal.id, &reset, None)
        .await
        .unwrap();
    let game = fixture.game(&portal.id).await;
    assert_eq!(game.description, "Portal 2 from IGDB.");
    assert_eq!(game.genres, ["Puzzle"]);
}

#[tokio::test]
async fn igdbs_cover_comes_beneath_one_the_user_chose() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let cache = ArtworkCache::new(fixture._dir.path().join("artwork")).unwrap();
    let mine = cache.import_bytes(png()).await.unwrap();
    fixture
        .store
        .set_user_artwork(&portal.id, ArtworkKind::Cover, &mine)
        .await
        .unwrap();
    fixture.metadata.run().await.unwrap();
    assert!(
        fixture
            .artwork(&portal.id)
            .await
            .contains(&(ArtworkKind::Cover, "igdb:co72".to_owned()))
    );
    let shown = |game: GameDetails| {
        game.artwork
            .into_iter()
            .filter(|art| art.kind == ArtworkKind::Cover)
            .map(|art| art.source)
            .collect::<Vec<_>>()
    };
    let details = fixture.store.get_game(&portal.id).await.unwrap().unwrap();
    assert_eq!(shown(details), ["user"]);
    fixture
        .store
        .reset_artwork(&portal.id, ArtworkKind::Cover)
        .await
        .unwrap();
    let details = fixture.store.get_game(&portal.id).await.unwrap().unwrap();
    assert_eq!(shown(details), ["igdb:co72"]);
}

#[tokio::test]
async fn a_match_by_hand_keeps_a_title_the_user_set() {
    let fixture = Fixture::new().await;
    let game = fixture.add("Portal II", vec![]).await;
    let edit = GameEdit {
        title: Some(TextEdit::Set {
            value: "My Portal".into(),
        }),
        ..Default::default()
    };
    fixture
        .store
        .edit_game(&game.id, &edit, None)
        .await
        .unwrap();
    fixture.metadata.set(&game.id, Some(72)).await.unwrap();
    let state = fixture
        .store
        .game_edit_state(&game.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.game.title, "My Portal");
    assert_eq!(state.automatic.title, "Portal 2");
}

#[tokio::test]
async fn igdbs_icon_joins_an_installers_and_the_library_shows_it() {
    let fixture = Fixture::new().await;
    let game = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let cache = ArtworkCache::new(fixture._dir.path().join("artwork")).unwrap();
    let exe = cache.import_bytes(png()).await.unwrap();
    fixture
        .store
        .upsert_artwork(&exe.record(&game.id, ArtworkKind::Icon, "installer"))
        .await
        .unwrap();

    fixture.metadata.run().await.unwrap();
    let art = fixture.artwork(&game.id).await;
    assert!(art.contains(&(ArtworkKind::Icon, "installer".to_owned())));
    assert!(art.contains(&(ArtworkKind::Icon, "igdb:ic72".to_owned())));
    let page = fixture
        .store
        .list_games(&crate::model::ListGames::default())
        .await
        .unwrap();
    assert_eq!(page.items[0].icon.as_ref().unwrap().source, "igdb:ic72");
}

#[tokio::test]
async fn a_match_by_hand_replaces_what_the_last_one_wrote() {
    let fixture = Fixture::new().await;
    let game = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    // The user edits the genres; IGDB's description stays IGDB's.
    let mut edited = fixture.game(&game.id).await;
    edited.genres = vec!["Mine".into()];
    fixture.store.upsert_game(&edited).await.unwrap();

    let set = fixture.metadata.set(&game.id, Some(1942)).await.unwrap();
    assert_eq!(
        (set.igdb_id, set.source, set.pending),
        (Some(1942), MatchSource::Manual, false)
    );
    let now = fixture.game(&game.id).await;
    // The game takes the chosen game's name, sort title too.
    assert_eq!(
        (now.title.as_str(), now.sort_title.as_str()),
        ("The Witcher 3", "The Witcher 3")
    );
    assert_eq!(now.description, "The Witcher 3 from IGDB.");
    assert_eq!(now.genres, ["Mine"]);
    let art = fixture.artwork(&game.id).await;
    assert!(art.contains(&(ArtworkKind::Cover, "igdb:co1942".to_owned())));
    assert!(!art.iter().any(|(_, source)| source.contains("72")));

    // No game: IGDB's values and artwork go, the user's stay.
    let none = fixture.metadata.set(&game.id, None).await.unwrap();
    assert_eq!((none.igdb_id, none.source), (None, MatchSource::Manual));
    let cleared = fixture.game(&game.id).await;
    assert!(cleared.description.is_empty());
    assert_eq!(cleared.release_date, None);
    assert!(cleared.aliases.is_empty());
    assert_eq!(cleared.genres, ["Mine"]);
    assert!(fixture.artwork(&game.id).await.is_empty());
    // Runs leave a choice made by hand alone, even one made mid-run.
    let auto = IgdbMatch::new(&game.id, Some(72), MatchSource::Store, "");
    assert!(!fixture.store.save_auto_igdb_match(&auto).await.unwrap());
    fixture.metadata.run().await.unwrap();
    assert_eq!(
        fixture
            .metadata
            .game(&game.id)
            .await
            .unwrap()
            .unwrap()
            .source,
        MatchSource::Manual
    );

    // Matching automatically again finds the store match.
    let again = fixture.metadata.rematch(&game.id).await.unwrap().unwrap();
    assert_eq!(
        (again.igdb_id, again.source),
        (Some(72), MatchSource::Store)
    );
    assert_eq!(
        fixture.game(&game.id).await.description,
        "Portal 2 from IGDB."
    );

    let error = fixture.metadata.set(&game.id, Some(5)).await.unwrap_err();
    assert!(error.to_string().contains("IGDB has no game 5"), "{error}");
}

#[tokio::test]
async fn a_match_by_hand_keeps_a_sort_title_of_the_users() {
    let fixture = Fixture::new().await;
    let game = fixture.add("witcher3", vec![]).await;
    let mut edited = fixture.game(&game.id).await;
    edited.sort_title = "Witcher 3".into();
    fixture.store.upsert_game(&edited).await.unwrap();

    fixture.metadata.set(&game.id, Some(1942)).await.unwrap();
    let now = fixture.game(&game.id).await;
    assert_eq!(
        (now.title.as_str(), now.sort_title.as_str()),
        ("The Witcher 3", "Witcher 3")
    );
}

#[tokio::test]
async fn an_unreachable_server_leaves_games_for_the_next_run() {
    let dir = tempfile::tempdir().unwrap();
    let down = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&down)
        .await;
    let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), down.uri());
    let fixture = Fixture::with_hasheous(dir, down, hasheous).await;
    let game = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.matched, report.deferred, report.unreachable),
        (0, 1, true)
    );
    assert_eq!(fixture.metadata.game(&game.id).await.unwrap(), None);
    // Still due, but the server is left alone for a while.
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 1);
    fixture.metadata.inner.igdb.unpause();
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 2);
}

#[tokio::test]
async fn a_run_stops_at_the_first_failure() {
    let dir = tempfile::tempdir().unwrap();
    let down = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&down)
        .await;
    let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), down.uri());
    let fixture = Fixture::with_hasheous(dir, down, hasheous).await;
    for app_id in 0..(CHUNK as u32 * 2 + 1) {
        fixture
            .add(
                &format!("Game {app_id}"),
                vec![ExternalId::Steam { app_id }],
            )
            .await;
    }

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.deferred, report.unreachable), (CHUNK * 2 + 1, true));
    assert_eq!(fixture.server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn new_dumps_look_again_locally_and_the_server_only_weekly() {
    let fixture = Fixture::new().await;
    let game = fixture
        .add("Unknown", vec![ExternalId::Steam { app_id: 9 }])
        .await;
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 1);
    let mut row = fixture.store.igdb_match(&game.id).await.unwrap().unwrap();
    let checked = row.checked_at_ms;
    row.dumps = "older dumps".into();
    fixture.store.save_igdb_match(&row).await.unwrap();
    // Unknown dumps are no change.
    assert!(
        fixture
            .store
            .igdb_pending(None, 0)
            .await
            .unwrap()
            .is_empty()
    );

    // Only the Hasheous dumps are asked again.
    assert_eq!(fixture.metadata.run().await.unwrap().deferred, 0);
    assert_eq!(fixture.requests("/match").await, 1);
    let row = fixture.store.igdb_match(&game.id).await.unwrap().unwrap();
    assert_eq!(row.dumps, fixture.metadata.dumps().await.unwrap());
    assert_eq!(
        (row.source, row.checked_at_ms),
        (MatchSource::None, checked)
    );
    assert_eq!(fixture.metadata.run().await.unwrap(), RunReport::default());

    // A week on, the server is asked again.
    let mut row = row;
    row.checked_at_ms = 0;
    fixture.store.save_igdb_match(&row).await.unwrap();
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 2);
}

#[tokio::test]
async fn a_game_the_server_does_not_send_undoes_nothing() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    // From now on the server answers 1942 with neither a game nor "none".
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"games": [], "notFound": []})),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let mut row = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    row.igdb_id = Some(1942);
    row.pending = true;
    fixture.store.save_igdb_match(&row).await.unwrap();

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.applied, report.deferred, report.unreachable),
        (0, 1, false)
    );
    let row = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    assert_eq!((row.igdb_id, row.pending), (Some(1942), true));
    assert_eq!(
        fixture.game(&portal.id).await.description,
        "Portal 2 from IGDB."
    );
    let error = fixture.metadata.refresh(&portal.id).await.unwrap_err();
    assert!(
        error.to_string().contains("no readable game 1942"),
        "{error}"
    );
    assert_eq!(
        fixture.game(&portal.id).await.description,
        "Portal 2 from IGDB."
    );
}

#[test]
fn retries_wait_longer_after_each_failure() {
    let minutes = |failures, random| retry_delay(failures, random, MAX_RETRY).as_secs() / 60;
    // `random` 2000 is no jitter; 0 is a fifth less, 3999 nearly a fifth more.
    assert_eq!(minutes(1, 2000), 10);
    assert_eq!(minutes(2, 2000), 20);
    assert_eq!(minutes(3, 2000), 40);
    assert_eq!(minutes(7, 2000), 360);
    assert_eq!(minutes(u32::MAX, 2000), 360);
    assert_eq!(minutes(1, 0), 8);
    assert_eq!(minutes(7, 3999), 431);
    // Offline, an hour at most, so the network coming back is noticed.
    let offline = |failures| retry_delay(failures, 2000, MAX_OFFLINE_RETRY).as_secs() / 60;
    assert_eq!(offline(3), 40);
    assert_eq!(offline(4), 60);
    assert_eq!(offline(u32::MAX), 60);
}

#[tokio::test]
async fn an_image_that_cannot_come_leaves_the_game_without_it() {
    let fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/img/co72.png"))
        .respond_with(ResponseTemplate::new(404))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.applied, report.deferred, report.unreachable),
        (1, 0, false)
    );
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [(ArtworkKind::Icon, "igdb:ic72".to_owned())]
    );
    let row = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    assert!(!row.pending);
    // Nothing is left to do, so the cover isn't asked for again.
    assert_eq!(fixture.metadata.run().await.unwrap(), RunReport::default());
    assert_eq!(fixture.requests("/img/co72.png").await, 1);
}

#[tokio::test]
async fn a_new_cover_that_cannot_download_keeps_the_old_one() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    Mock::given(method("GET"))
        .and(path("/img/co1942.png"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&fixture.server)
        .await;

    let set = fixture.metadata.set(&portal.id, Some(1942)).await.unwrap();
    assert!(set.pending);
    // The new icon replaced the old; the old cover stays until the new one
    // is in.
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [
            (ArtworkKind::Cover, "igdb:co72".to_owned()),
            (ArtworkKind::Icon, "igdb:ic1942".to_owned()),
        ]
    );

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.applied, report.deferred), (1, 0));
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [
            (ArtworkKind::Cover, "igdb:co1942".to_owned()),
            (ArtworkKind::Icon, "igdb:ic1942".to_owned()),
        ]
    );
}

#[tokio::test]
async fn an_unreachable_image_host_stops_the_run() {
    let fixture = Fixture::new().await;
    // The metadata server answers, but its images are somewhere unreachable.
    let gone = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(move |request: &Request| {
            let ids: Vec<u64> = serde_json::from_slice(&request.body).unwrap();
            let games: Vec<Value> = ids
                .iter()
                .map(|id| igdb_game(&gone, *id, "Portal 2"))
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"games": games, "notFound": []}))
        })
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    // Different IGDB games, so none of them merge.
    for app_id in 0..=CHUNK {
        let game = fixture.add(&format!("Game {app_id}"), vec![]).await;
        let mut row = IgdbMatch::new(&game.id, Some(1000 + app_id as u64), MatchSource::Store, "");
        row.pending = true;
        fixture.store.save_igdb_match(&row).await.unwrap();
    }

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.deferred, report.unreachable), (CHUNK + 1, true));
    // Only the first chunk was looked up; its text went in.
    assert_eq!(fixture.requests("/games").await, 1);
    let first = fixture.store.igdb_pending(None, 0).await.unwrap();
    assert_eq!(first.len(), CHUNK + 1);
    assert_eq!(
        fixture.game(&first[0]).await.description,
        "Portal 2 from IGDB."
    );
}

#[tokio::test]
async fn a_page_image_download_does_not_hold_up_a_match_by_hand() {
    let fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/img/ar72.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(png())
                .set_delay(std::time::Duration::from_secs(3)),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();

    let page = tokio::spawn({
        let metadata = fixture.metadata.clone();
        let id = portal.id.clone();
        async move { metadata.page_images(&id).await.unwrap() }
    });
    while fixture.requests("/img/ar72.png").await == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let started = std::time::Instant::now();
    let set = fixture.metadata.set(&portal.id, None).await.unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "the match waited for the page's images"
    );
    assert_eq!((set.igdb_id, set.source), (None, MatchSource::Manual));
    // The page's images are for a match that is gone.
    assert!(!page.await.unwrap());
    assert!(fixture.artwork(&portal.id).await.is_empty());
}

#[tokio::test]
async fn merged_games_keep_a_match_and_are_applied_again() {
    let fixture = Fixture::new().await;
    let keep = fixture.add("Portal 2", vec![]).await;
    let donor = fixture
        .add("Portal 2 (Steam)", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();

    fixture
        .store
        .merge_games(&keep.id, &donor.id)
        .await
        .unwrap();
    let moved = fixture.store.igdb_match(&keep.id).await.unwrap().unwrap();
    assert_eq!((moved.igdb_id, moved.pending), (Some(72), true));
    fixture.metadata.run().await.unwrap();
    let merged = fixture.game(&keep.id).await;
    assert_eq!(merged.description, "Portal 2 from IGDB.");
    let covers = fixture
        .artwork(&keep.id)
        .await
        .into_iter()
        .filter(|(kind, _)| *kind == ArtworkKind::Cover)
        .count();
    assert_eq!(covers, 1);
}

#[tokio::test]
async fn a_match_by_hand_that_merges_the_game_answers_for_the_merged_game() {
    let fixture = Fixture::new().await;
    let release = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.set(&release.id, Some(72)).await.unwrap();
    let added = fixture.add("Portal Two", vec![]).await;
    fixture.metadata.run().await.unwrap();

    // Both chosen by hand: the older one stays and the added one goes into it.
    let set = fixture.metadata.set(&added.id, Some(72)).await.unwrap();
    assert_eq!(set.game_id, release.id);
    assert_eq!((set.igdb_id, set.source), (Some(72), MatchSource::Manual));
    assert!(fixture.store.game(&added.id).await.unwrap().is_none());
    assert_eq!(
        fixture
            .store
            .entries_for_game(&release.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn age_ratings_apply_and_a_game_without_any_is_unrated() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let witcher = fixture
        .add("The Witcher 3", vec![ExternalId::Gog { product_id: 1 }])
        .await;
    let unknown = fixture
        .add("Unknown", vec![ExternalId::Steam { app_id: 9 }])
        .await;
    fixture.metadata.run().await.unwrap();

    let rated = fixture
        .store
        .game_age_ratings(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rated.ratings,
        [rating("pegi", "12"), rating("esrb", "E10+")]
    );
    assert_eq!(rated.min_age, Some(12));
    // Matched without ratings, or not matched: unrated, not 0.
    for id in [&witcher.id, &unknown.id] {
        let unrated = fixture.store.game_age_ratings(id).await.unwrap().unwrap();
        assert_eq!(unrated, crate::db::GameAgeRatings::default());
    }

    // Ratings of the game's own stay, like its other fields.
    let own = [rating("usk", "18")];
    fixture
        .store
        .set_game_age_ratings(&portal.id, &own)
        .await
        .unwrap();
    fixture.metadata.refresh(&portal.id).await.unwrap();
    let kept = fixture
        .store
        .game_age_ratings(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (kept.ratings.as_slice(), kept.min_age),
        (own.as_slice(), Some(18))
    );
}

#[tokio::test]
async fn games_matched_before_ratings_get_them_once_without_a_rematch() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    // As a library from before ratings, after migration 0015: no ratings,
    // the match pending again, and the cache's answer from before them.
    fixture
        .store
        .set_game_age_ratings(&portal.id, &[])
        .await
        .unwrap();
    let mut row = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    row.pending = true;
    row.applied.age_ratings = None;
    fixture.store.save_igdb_match(&row).await.unwrap();
    let base = fixture.server.uri();
    let mut old = igdb_game(&base, 72, "Portal 2");
    old.as_object_mut().unwrap().remove("ageRatings");
    fixture.metadata.inner.igdb.cache_outdated(72, &old).await;
    let matches = fixture.requests("/match").await;
    let games = fixture.requests("/games").await;

    // Offline, the game keeps what it has and waits.
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.deferred, report.unreachable), (1, true));
    let waiting = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    assert!(waiting.pending);
    assert_eq!(
        fixture.game(&portal.id).await.description,
        "Portal 2 from IGDB."
    );

    // Back online, the game is fetched again once and gets its ratings.
    fixture.metadata.inner.igdb.unpause();
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.applied, report.deferred), (1, 0));
    let rated = fixture
        .store
        .game_age_ratings(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rated.min_age, Some(12));
    assert_eq!(fixture.requests("/games").await, games + 2);
    assert_eq!(fixture.requests("/match").await, matches);
    assert_eq!(fixture.metadata.run().await.unwrap(), RunReport::default());
    assert_eq!(fixture.requests("/games").await, games + 2);
}

#[tokio::test]
async fn a_merged_game_takes_the_highest_rating_of_its_releases() {
    let fixture = Fixture::new().await;
    let keep = fixture.add("Portal 2", vec![]).await;
    let donor = fixture.add("Portal 2 (Japan)", vec![]).await;
    fixture
        .store
        .set_game_age_ratings(&keep.id, &[rating("pegi", "12")])
        .await
        .unwrap();
    fixture
        .store
        .set_game_age_ratings(&donor.id, &[rating("cero", "C"), rating("pegi", "12")])
        .await
        .unwrap();
    fixture
        .store
        .merge_games(&keep.id, &donor.id)
        .await
        .unwrap();
    let merged = fixture
        .store
        .game_age_ratings(&keep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(merged.ratings, [rating("pegi", "12"), rating("cero", "C")]);
    assert_eq!(merged.min_age, Some(15));
}

#[tokio::test]
async fn catalog_games_in_the_library_are_owned_and_maybe_installed() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    let owned = fixture
        .store
        .igdb_owned(&[72, 1942, u64::MAX])
        .await
        .unwrap();
    assert_eq!(
        owned,
        [crate::db::IgdbOwned {
            igdb_id: 72,
            game_id: portal.id.clone(),
            installed: false,
        }]
    );
    fixture
        .store
        .set_game_hidden(&portal.id, true)
        .await
        .unwrap();
    assert!(fixture.store.igdb_owned(&[72]).await.unwrap().is_empty());
}

#[tokio::test]
async fn romm_games_match_by_romms_id_and_keep_romms_values_and_artwork() {
    let fixture = Fixture::new().await;
    let mut game = Game::new("Portal 2");
    game.description = "From RomM.".into();
    let game = fixture.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, crate::romm::source_id("home"), "10");
    // Its Steam id would match too, but RomM's own match comes first.
    entry.ids = vec![ExternalId::Steam { app_id: 620 }];
    let entry = fixture.store.upsert_entry(&entry).await.unwrap();
    let rom = serde_json::from_value(json!({
        "id": 10, "platform_id": 1, "fs_name": "portal.iso", "igdb_id": 72
    }))
    .unwrap();
    fixture
        .store
        .save_romm_rom(&crate::romm::RommRom {
            server_id: "home".into(),
            rom_id: 10,
            entry_id: entry.id.clone(),
            game_id: game.id.clone(),
            platform_id: "sony_playstation".into(),
            rom,
            synced_target: crate::emulation::EmulatorTarget::PlatformDefault,
            pinned: false,
            orphaned: false,
            cache: None,
        })
        .await
        .unwrap();
    let cache = ArtworkCache::new(fixture._dir.path().join("artwork")).unwrap();
    let image = cache.import_bytes(png()).await.unwrap();
    for kind in [ArtworkKind::Cover, ArtworkKind::Screenshot] {
        fixture
            .store
            .upsert_artwork(&image.record(&game.id, kind, "romm:home"))
            .await
            .unwrap();
    }

    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 0);
    let found = fixture.metadata.game(&game.id).await.unwrap().unwrap();
    assert_eq!((found.igdb_id, found.source), (Some(72), MatchSource::Romm));
    // IGDB fills only what RomM left empty, and adds a hero and an icon.
    fixture.metadata.page_images(&game.id).await.unwrap();
    let filled = fixture.game(&game.id).await;
    assert_eq!(filled.description, "From RomM.");
    assert_eq!(filled.genres, ["Puzzle"]);
    assert_eq!(
        fixture.artwork(&game.id).await,
        [
            (ArtworkKind::Hero, "igdb:ar72".to_owned()),
            (ArtworkKind::Icon, "igdb:ic72".to_owned()),
            (ArtworkKind::Cover, "romm:home".to_owned()),
            (ArtworkKind::Screenshot, "romm:home".to_owned()),
        ]
    );
}

#[tokio::test]
async fn a_romm_row_without_an_id_is_matched_again() {
    let fixture = Fixture::new().await;
    let game = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    // Saved by a version that left RomM games alone.
    let mut row = IgdbMatch::new(&game.id, None, MatchSource::Romm, "");
    row.pending = false;
    fixture.store.save_igdb_match(&row).await.unwrap();

    fixture.metadata.run().await.unwrap();
    let found = fixture.metadata.game(&game.id).await.unwrap().unwrap();
    assert_eq!(
        (found.igdb_id, found.source),
        (Some(72), MatchSource::Store)
    );
}

impl Fixture {
    /// A ROM on `platform` (the default catalog has no names, so the id is
    /// its name) that RomM matched to `igdb_id`.
    async fn add_rom(&self, title: &str, platform: &str, igdb_id: u64) -> Game {
        let mut game = Game::new(title);
        game.platforms = vec![platform.to_owned()];
        let game = self.store.upsert_game(&game).await.unwrap();
        let mut entry = GameEntry::new(
            &game.id,
            SourceId::new("roms"),
            format!("{platform}/{title}"),
        );
        entry.state = caskade_discovery::InstallState::Installed;
        entry.launch = Some(crate::model::LaunchSpec::Emulator {
            platform_id: platform.to_owned(),
            target: crate::emulation::EmulatorTarget::PlatformDefault,
            roms: vec![],
            args: crate::emulation::ArgsOverride::Default,
        });
        self.store.upsert_entry(&entry).await.unwrap();
        let row = IgdbMatch::new(&game.id, Some(igdb_id), MatchSource::Romm, "");
        self.store.save_igdb_match(&row).await.unwrap();
        game
    }

    async fn entries(&self, game_id: &str) -> Vec<String> {
        self.store
            .entries_for_game(game_id)
            .await
            .unwrap()
            .into_iter()
            .map(|entry| entry.external_id)
            .collect()
    }
}

#[tokio::test]
async fn releases_of_one_igdb_game_become_one_game_with_an_entry_each() {
    let fixture = Fixture::new().await;
    let snes = fixture.add_rom("Portal 2", "snes", 72).await;
    let genesis = fixture.add_rom("Portal 2", "genesis", 72).await;
    let steam = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let hidden = fixture.add_rom("Portal 2", "gb", 72).await;
    fixture
        .store
        .set_game_hidden(&hidden.id, true)
        .await
        .unwrap();
    let other = fixture.add_rom("Worms", "snes", 9331).await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(report.merged, 2);
    // The oldest stays, with every release's entry and platform.
    assert!(fixture.store.game(&genesis.id).await.unwrap().is_none());
    assert!(fixture.store.game(&steam.id).await.unwrap().is_none());
    assert_eq!(
        fixture.entries(&snes.id).await,
        ["genesis/Portal 2", "snes/Portal 2", "Portal 2"]
    );
    let game = fixture.game(&snes.id).await;
    assert_eq!(game.platforms, ["genesis", "snes"]);
    assert_eq!(game.description, "Portal 2 from IGDB.");
    // A hidden game stays hidden, apart; other games stay as they were.
    assert_eq!(fixture.entries(&hidden.id).await, ["gb/Portal 2"]);
    assert_eq!(fixture.entries(&other.id).await, ["snes/Worms"]);
    assert_eq!(fixture.metadata.run().await.unwrap().merged, 0);
}

#[tokio::test]
async fn a_separated_release_stays_apart() {
    let fixture = Fixture::new().await;
    let snes = fixture.add_rom("Portal 2", "snes", 72).await;
    let mut pc = fixture.add_rom("Portal 2", "genesis", 72).await;
    pc.platforms = vec!["PC".into()];
    fixture.store.upsert_game(&pc).await.unwrap();
    let mut steam = GameEntry::new(&pc.id, SourceId::new("steam"), "620");
    steam.state = caskade_discovery::InstallState::Installed;
    fixture.store.upsert_entry(&steam).await.unwrap();
    fixture.metadata.run().await.unwrap();
    // The game with the most entries is the one kept.
    assert!(fixture.store.game(&snes.id).await.unwrap().is_none());
    assert_eq!(fixture.game(&pc.id).await.platforms, ["PC", "snes"]);
    let genesis = fixture
        .store
        .entries_for_game(&pc.id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.external_id == "genesis/Portal 2")
        .unwrap();

    let apart = fixture.metadata.separate(&genesis.id).await.unwrap();
    assert_eq!(fixture.entries(&apart).await, ["genesis/Portal 2"]);
    let separated = fixture.game(&apart).await;
    assert_eq!(separated.platforms, ["genesis"]);
    assert_eq!(separated.description, "Portal 2 from IGDB.");
    assert!(!fixture.artwork(&apart).await.is_empty());
    assert_eq!(
        fixture
            .metadata
            .game(&apart)
            .await
            .unwrap()
            .unwrap()
            .igdb_id,
        Some(72)
    );
    // The PC entry keeps the platforms that aren't an emulated entry's.
    assert_eq!(fixture.game(&pc.id).await.platforms, ["PC", "snes"]);
    assert_eq!(fixture.metadata.run().await.unwrap().merged, 0);
    assert_eq!(fixture.entries(&apart).await, ["genesis/Portal 2"]);
    // A game's last entry has nowhere to go.
    assert!(fixture.metadata.separate(&genesis.id).await.is_err());
}

impl Fixture {
    /// A ROM on `platform` released on `released`, which RomM matched to
    /// `igdb_id` or (`None`) left without one.
    async fn dated_rom(
        &self,
        title: &str,
        platform: &str,
        released: &str,
        igdb_id: Option<u64>,
    ) -> Game {
        let game = match igdb_id {
            Some(id) => self.add_rom(title, platform, id).await,
            None => {
                let mut game = Game::new(title);
                game.platforms = vec![platform.to_owned()];
                let game = self.store.upsert_game(&game).await.unwrap();
                let mut entry = GameEntry::new(
                    &game.id,
                    SourceId::new("roms"),
                    format!("{platform}/{title}"),
                );
                entry.launch = Some(crate::model::LaunchSpec::Emulator {
                    platform_id: platform.to_owned(),
                    target: crate::emulation::EmulatorTarget::PlatformDefault,
                    roms: vec![],
                    args: crate::emulation::ArgsOverride::Default,
                });
                self.store.upsert_entry(&entry).await.unwrap();
                game
            }
        };
        let mut game = self.game(&game.id).await;
        game.release_date = Some(released.to_owned());
        self.store.upsert_game(&game).await.unwrap()
    }
}

#[tokio::test]
async fn a_release_romm_left_unmatched_joins_its_game_offline_and_stays_joined() {
    let dir = tempfile::tempdir().unwrap();
    let down = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&down)
        .await;
    let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), down.uri());
    let fixture = Fixture::with_hasheous(dir, down, hasheous).await;
    // RomM matched the N64 ROM; the PlayStation one has no IGDB id.
    let n64 = fixture
        .dated_rom("A Bug's Life", "n64", "1998-11-01", Some(2847))
        .await;
    let psx = fixture
        .dated_rom("A Bug's Life", "psx", "1998-11-18", None)
        .await;
    // A game of the same name from another era, and a sequel.
    let remake = fixture
        .dated_rom("A Bug's Life", "ps4", "2024-01-01", None)
        .await;
    let sequel = fixture
        .dated_rom("A Bug's Life 2", "psx", "1999-06-01", None)
        .await;

    let report = fixture.metadata.run().await.unwrap();
    // The server is down, but the merge needs only the library.
    assert!(report.unreachable);
    assert_eq!((report.merged, report.none), (1, 3));
    assert!(fixture.store.game(&psx.id).await.unwrap().is_none());
    let mut entries = fixture.entries(&n64.id).await;
    entries.sort();
    assert_eq!(entries, ["n64/A Bug's Life", "psx/A Bug's Life"]);
    let row = fixture.store.igdb_match(&n64.id).await.unwrap().unwrap();
    // Applied once the server answers again.
    assert_eq!((row.igdb_id, row.pending), (Some(2847), true));
    assert_eq!(fixture.entries(&remake.id).await, ["ps4/A Bug's Life"]);
    assert_eq!(fixture.entries(&sequel.id).await, ["psx/A Bug's Life 2"]);

    // Later runs (and restarts) leave it one game.
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(report.merged, 0);
    let mut entries = fixture.entries(&n64.id).await;
    entries.sort();
    assert_eq!(entries, ["n64/A Bug's Life", "psx/A Bug's Life"]);
}

#[tokio::test]
async fn refreshing_chosen_games_matches_fills_in_and_respects_choices() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let unknown = fixture
        .add("Unknown", vec![ExternalId::Steam { app_id: 9 }])
        .await;
    let mine = fixture
        .add("Witcher 3", vec![ExternalId::Gog { product_id: 1 }])
        .await;
    fixture.metadata.set(&mine.id, None).await.unwrap();
    let ids = vec![
        portal.id.clone(),
        unknown.id.clone(),
        mine.id.clone(),
        "gone".to_owned(),
    ];
    let mut seen = Vec::new();
    let status = fixture
        .metadata
        .refresh_games(
            &ids,
            RefreshMode::Missing,
            &CancellationToken::new(),
            |status| seen.push(status.done),
        )
        .await
        .unwrap();
    assert_eq!(
        (
            status.updated,
            status.unmatched,
            status.skipped,
            status.failed.len()
        ),
        (1, 1, 2, 0)
    );
    assert!(!status.running);
    assert_eq!(seen.last(), Some(&4));
    assert_eq!(
        fixture.game(&portal.id).await.description,
        "Portal 2 from IGDB."
    );
    // "Not on IGDB" stays so.
    let none = fixture.metadata.game(&mine.id).await.unwrap().unwrap();
    assert_eq!((none.igdb_id, none.source), (None, MatchSource::Manual));

    // A field the user set stays theirs through an automatic refresh, and
    // the refresh asks for the matched games in one request.
    let edit = GameEdit {
        description: Some(TextEdit::Set {
            value: "Mine.".into(),
        }),
        ..Default::default()
    };
    fixture
        .store
        .edit_game(&portal.id, &edit, None)
        .await
        .unwrap();
    let before = fixture.requests("/games").await;
    let status = fixture
        .metadata
        .refresh_games(
            &ids,
            RefreshMode::Automatic,
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    assert!(status.failed.is_empty());
    assert_eq!(fixture.game(&portal.id).await.description, "Mine.");
    assert!(fixture.requests("/games").await <= before + 1);
}

#[tokio::test]
async fn a_cancelled_refresh_leaves_the_rest_and_a_failed_one_can_be_retried() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let status = fixture
        .metadata
        .refresh_games(
            std::slice::from_ref(&portal.id),
            RefreshMode::Missing,
            &cancel,
            |_| {},
        )
        .await
        .unwrap();
    assert!(status.cancelled);
    assert_eq!(status.remaining, std::slice::from_ref(&portal.id));
    assert!(fixture.game(&portal.id).await.description.is_empty());

    // The server down: the game is listed to retry, and keeps what it had.
    let dir = tempfile::tempdir().unwrap();
    let down = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&down)
        .await;
    let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), down.uri());
    let offline = Fixture::with_hasheous(dir, down, hasheous).await;
    let game = offline
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let status = offline
        .metadata
        .refresh_games(
            std::slice::from_ref(&game.id),
            RefreshMode::Automatic,
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    assert_eq!(status.failed, std::slice::from_ref(&game.id));
    assert_eq!(offline.game(&game.id).await.title, "Portal 2");
}

#[tokio::test]
async fn one_refresh_runs_at_a_time_and_reports_as_it_goes() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let mut updates = fixture.metadata.refresh_updates();
    fixture
        .metadata
        .start_refresh(vec![portal.id.clone()], RefreshMode::Missing)
        .unwrap();
    assert!(
        fixture
            .metadata
            .start_refresh(vec![portal.id.clone()], RefreshMode::Missing)
            .is_err()
    );
    loop {
        updates.changed().await.unwrap();
        let status = updates.borrow().clone().unwrap();
        if !status.running {
            assert_eq!(status.updated, 1);
            break;
        }
    }
    // Done, so another may start.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!fixture.metadata.cancel_refresh());
    fixture
        .metadata
        .start_refresh(vec![portal.id.clone()], RefreshMode::Missing)
        .unwrap();
}
