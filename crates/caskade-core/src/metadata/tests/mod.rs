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

mod choices;
mod matching;
mod ratings;
mod refresh;
mod releases;
mod runs;

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

/// Indexes a Windows dump with "Worms" (9331, including ROM hashes) and
/// "Portal 2" (72, title only).
async fn hasheous_with_worms(dir: &std::path::Path, server: &MockServer) -> Hasheous {
    let game = json!({
        "Id": 1, "ObjectType": "Game", "Name": "Worms",
        "Metadata": [
            {"Source": "IGDB", "Id": "9331", "Status": "Mapped", "Link": "https://www.igdb.com/games/worms"}
        ],
        "Attributes": [{"attributeName": "ROMs", "attributeType": "EmbeddedList", "Value": [{
            "Name": "worms.bin", "Size": 23800, "Crc": "08F5EB18",
            "Md5": "18af7be2a83f4027b47acce509785508",
            "Sha1": "3ecc30ea1be47361cf44ee4e9c55bc0542aa41af"
        }]}]
    });
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file("Worms (1).json", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(game.to_string().as_bytes()).unwrap();
    writer
        .start_file(
            "Portal 2 (2).json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer
        .write_all(
            json!({
                "Id": 2, "ObjectType": "Game", "Name": "Portal 2",
                "Metadata": [{"Source": "IGDB", "Id": "72", "Status": "Mapped"}],
                "Attributes": []
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
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

/// The ids of an Epic game in `namespace`.
fn epic(namespace: &str) -> Vec<ExternalId> {
    vec![ExternalId::Epic {
        namespace: namespace.to_owned(),
        catalog_item: format!("item-{namespace}"),
        app_name: format!("app-{namespace}"),
    }]
}
