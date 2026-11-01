use super::*;
use crate::{
    emulation::{
        ArgsOverride, Catalog, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget,
        OsFolders, ProfileHooks, Tracking,
    },
    model::{Game, GameEntry, LaunchSpec},
};
use caskade_discovery::{GameRuntime, SourceId};
use md5::Digest;
use serde_json::json;
use std::io::Write;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const DUMP: &str = "Test Platform";
const ZIP_PATH: &str = "/api/v1/Dumps/platforms/Test%20Platform.zip";

fn game(id: i64, name: &str, aliases: &str, roms: serde_json::Value) -> String {
    json!({
        "Id": id,
        "ObjectType": "Game",
        "Name": name,
        "Metadata": [
            {"Source": "IGDB", "Id": "1234", "Status": "Mapped", "Link": "https://www.igdb.com/games/chirp"},
            {"Source": "TheGamesDb", "Id": "", "Status": "NotMapped", "Link": ""}
        ],
        "Attributes": [
            {"attributeName": "Platform", "attributeType": "ObjectRelationship", "Value": {"Id": 1}},
            {"attributeName": "ROMs", "attributeType": "EmbeddedList", "Value": roms},
            {"attributeName": "SearchAliases", "attributeType": "ShortString", "Value": aliases}
        ]
    })
    .to_string()
}

fn chirp() -> String {
    game(
        248876,
        "Chirp",
        "Chirp, Chirp: The Game",
        json!([{
            "Name": "Chirp.hex", "Size": 23800, "Crc": "8EF5EB18",
            "Md5": "18af7be2a83f4027b47acce509785508",
            "Sha1": "3ecc30ea1be47361cf44ee4e9c55bc0542aa41af", "Sha256": ""
        }]),
    )
}

fn zip_of(entries: &[(&str, String)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn md5_hex(bytes: &[u8]) -> String {
    hex(&md5::Md5::digest(bytes))
}

/// Serves `zip` as the only dump, listed with `md5` (its own by default).
async fn serve(server: &MockServer, zip: &[u8], md5: Option<&str>) {
    let listing = json!([{
        "name": format!("{DUMP}.zip"),
        "sizeBytes": zip.len(),
        "lastModifiedUtc": "2026-10-23T12:32:24.75858Z",
        "md5hash": md5.map_or_else(|| md5_hex(zip), str::to_owned),
    }]);
    Mock::given(method("GET"))
        .and(path("/api/v1/Dumps/platforms"))
        .respond_with(ResponseTemplate::new(200).set_body_json(listing))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(ZIP_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip.to_vec()))
        .mount(server)
        .await;
}

async fn downloads(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path() == ZIP_PATH)
        .count()
}

fn wanted_dump() -> BTreeSet<String> {
    BTreeSet::from([DUMP.to_owned()])
}

#[tokio::test]
async fn dumps_are_downloaded_once_and_answer_offline() {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let zip = zip_of(&[
        ("Chirp (248876).json", chirp()),
        ("notes.txt", "not a game".into()),
        ("Broken (1).json", "{".into()),
    ]);
    serve(&server, &zip, None).await;
    let hasheous = Hasheous::with_base_url(temp.path(), server.uri());

    let report = hasheous.sync(&wanted_dump()).await.unwrap();
    assert_eq!(report.downloaded, [DUMP]);
    assert_eq!(report.indexed, [DUMP]);
    assert!(report.failed.is_empty() && report.unavailable.is_empty());
    assert!(temp.path().join("dumps/Test Platform.zip").is_file());
    let dumps = hasheous.dumps().await.unwrap();
    assert_eq!(dumps.len(), 1);
    assert_eq!(
        (dumps[0].games, dumps[0].md5.as_str()),
        (1, md5_hex(&zip).as_str())
    );

    // Up to date: no second download.
    let report = hasheous.sync(&wanted_dump()).await.unwrap();
    assert_eq!(report, SyncReport::default());
    assert_eq!(downloads(&server).await, 1);

    let by = |hashes: RomHashes| {
        let hasheous = hasheous.clone();
        async move { hasheous.lookup(&hashes).await.unwrap() }
    };
    let found = by(RomHashes {
        md5: Some("18AF7BE2A83F4027B47ACCE509785508".into()),
        ..RomHashes::default()
    })
    .await;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].game.name, "Chirp");
    assert_eq!(found[0].game.id, "248876");
    assert_eq!(found[0].game.dump, DUMP);
    assert_eq!(
        found[0].game.metadata,
        [MetadataId {
            source: "IGDB".into(),
            id: "1234".into(),
            link: Some("https://www.igdb.com/games/chirp".into()),
        }]
    );
    assert_eq!(found[0].rom.crc.as_deref(), Some("8ef5eb18"));
    assert_eq!(found[0].rom.sha256, None);
    // A stronger hash that doesn't match wins over a weaker one that does.
    let sha1_miss = by(RomHashes {
        sha1: Some("0000000000000000000000000000000000000000".into()),
        crc: Some("8ef5eb18".into()),
        ..RomHashes::default()
    })
    .await;
    assert_eq!(sha1_miss.len(), 1, "falls through to the CRC");
    let crc = |size| RomHashes {
        crc: Some("8EF5EB18".into()),
        size,
        ..RomHashes::default()
    };
    assert_eq!(by(crc(Some(23800))).await.len(), 1);
    assert!(by(crc(Some(1))).await.is_empty());

    let search = |title: &'static str, dumps: Vec<String>| {
        let hasheous = hasheous.clone();
        async move { hasheous.search(title, &dumps, 10).await.unwrap() }
    };
    assert_eq!(search("CHIRP", vec![]).await.len(), 1);
    assert_eq!(search("Chirp: The Game", vec![DUMP.into()]).await.len(), 1);
    assert!(search("Chirp", vec!["Other".into()]).await.is_empty());
    assert!(search("Chir", vec![]).await.is_empty());
}

#[tokio::test]
async fn a_changed_dump_replaces_the_old_one() {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    serve(&server, &zip_of(&[("Chirp (248876).json", chirp())]), None).await;
    let hasheous = Hasheous::with_base_url(temp.path(), server.uri());
    hasheous.sync(&wanted_dump()).await.unwrap();

    let other = game(
        7,
        "Other",
        "",
        json!([{"Name": "Other.bin", "Sha1": "ABCDEF0123456789ABCDEF0123456789ABCDEF01"}]),
    );
    server.reset().await;
    serve(&server, &zip_of(&[("Other (7).json", other)]), None).await;
    // Still within a day: the cached listing is used and nothing changes.
    assert_eq!(
        hasheous.sync(&wanted_dump()).await.unwrap(),
        SyncReport::default()
    );
    // A stale listing is fetched again.
    std::fs::write(
        temp.path().join("listing.json"),
        r#"{"fetched":0,"dumps":[]}"#,
    )
    .unwrap();
    let report = hasheous.sync(&wanted_dump()).await.unwrap();
    assert_eq!(report.downloaded, [DUMP]);
    assert!(hasheous.search("Chirp", &[], 10).await.unwrap().is_empty());
    let found = hasheous
        .lookup(&RomHashes {
            sha1: Some("abcdef0123456789abcdef0123456789abcdef01".into()),
            ..RomHashes::default()
        })
        .await
        .unwrap();
    assert_eq!(found[0].game.name, "Other");
    assert_eq!(found[0].rom.size, None);
}

#[tokio::test]
async fn a_corrupt_download_is_discarded() {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let zip = zip_of(&[("Chirp (248876).json", chirp())]);
    serve(&server, &zip, Some("00000000000000000000000000000000")).await;
    let hasheous = Hasheous::with_base_url(temp.path(), server.uri());
    let report = hasheous.sync(&wanted_dump()).await.unwrap();
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert!(report.failed[0].1.contains("MD5"), "{report:?}");
    assert!(
        std::fs::read_dir(temp.path().join("dumps"))
            .unwrap()
            .next()
            .is_none()
    );
    assert!(hasheous.dumps().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_index_is_rebuilt_from_the_zips_without_downloading() {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    serve(&server, &zip_of(&[("Chirp (248876).json", chirp())]), None).await;
    Hasheous::with_base_url(temp.path(), server.uri())
        .sync(&wanted_dump())
        .await
        .unwrap();
    for file in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
        let _ = std::fs::remove_file(temp.path().join(file));
    }
    server.reset().await;

    let hasheous = Hasheous::with_base_url(temp.path(), server.uri());
    let report = hasheous.sync(&wanted_dump()).await.unwrap();
    assert_eq!(
        (report.downloaded.len(), report.indexed.len()),
        (0, 1),
        "{report:?}"
    );
    assert_eq!(hasheous.search("chirp", &[], 10).await.unwrap().len(), 1);
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
}

#[tokio::test]
async fn dumps_hasheous_lacks_are_reported() {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    serve(&server, &zip_of(&[]), None).await;
    let hasheous = Hasheous::with_base_url(temp.path(), server.uri());
    let report = hasheous
        .sync(&BTreeSet::from([
            "Nintendo Switch".to_owned(),
            "../x".to_owned(),
        ]))
        .await
        .unwrap();
    assert_eq!(report.unavailable, ["Nintendo Switch"]);
    assert_eq!(report.failed.len(), 1);
}

#[cfg(feature = "net")]
#[tokio::test]
async fn an_offline_first_sync_fails_so_the_watch_retries_soon() {
    let temp = tempfile::tempdir().unwrap();
    // An address nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let hasheous = Hasheous::with_base_url(temp.path(), format!("http://127.0.0.1:{port}"));
    let outcome = hasheous.sync(&wanted_dump()).await;
    let report = outcome.as_ref().unwrap();
    assert!(report.unavailable.is_empty(), "{report:?}");
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert!(
        report.failed[0].1.contains("can't reach 127.0.0.1"),
        "{report:?}"
    );
    assert!(retry_soon(&outcome));

    // Hasheous lacking a dump is no reason to hurry.
    let server = MockServer::start().await;
    serve(&server, &zip_of(&[]), None).await;
    let other = tempfile::tempdir().unwrap();
    let outcome = Hasheous::with_base_url(other.path(), server.uri())
        .sync(&BTreeSet::from(["Nintendo Switch".to_owned()]))
        .await;
    assert_eq!(outcome.as_ref().unwrap().unavailable, ["Nintendo Switch"]);
    assert!(!retry_soon(&outcome));
}

#[test]
fn dump_names_are_file_stems() {
    assert!(valid_dump("Sega Mega Drive _ Genesis"));
    assert!(valid_dump("Nintendo Game & Watch"));
    for bad in ["", " x", "a/b", "a\\b", "..", "x.zip", "a\nb", "C:"] {
        assert!(!valid_dump(bad), "{bad:?}");
    }
    let catalog = |ids: &str| {
        Catalog::load([(
            "platforms.toml",
            format!("[[platform]]\nid = \"p\"\nname = \"P\"\nshort = \"P\"\nids = {{ {ids} }}\n")
                .as_str(),
        )])
    };
    assert!(catalog("hasheous = \"Nintendo 64\"").is_ok());
    assert!(catalog("hasheous = \"Nintendo 64.zip\"").is_err());
}

fn test_catalog() -> Catalog {
    Catalog::load([(
        "platforms.toml",
        r#"
[[platform]]
id = "test_platform"
name = "Test Platform"
short = "TP"
extensions = ["tp"]
ids = { hasheous = "Test Platform" }

[[platform]]
id = "no_dump"
name = "No dump"
short = "ND"
"#,
    )])
    .unwrap()
}

fn emulator(platforms: &[&str]) -> Emulator {
    Emulator {
        id: String::new(),
        name: "Test".into(),
        definition: None,
        install: EmulatorInstall::Path {
            binary: "test-emulator".into(),
        },
        profiles: vec![EmulatorProfile::Custom {
            id: String::new(),
            name: "Default".into(),
            executable: None,
            args: vec!["{RomPath}".into()],
            working_dir: None,
            platforms: platforms.iter().map(|id| (*id).to_owned()).collect(),
            extensions: vec!["tp".into()],
            env: vec![],
            tracking: Tracking::Default,
            hooks: ProfileHooks::default(),
        }],
    }
}

async fn add_entry(store: &LibraryStore, runtime: GameRuntime, emulated: bool) {
    let game = store.upsert_game(&Game::new("A game")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), crate::model::new_id());
    entry.runtime = runtime;
    if emulated {
        entry.launch = Some(LaunchSpec::Emulator {
            platform_id: "test_platform".into(),
            target: EmulatorTarget::PlatformDefault,
            roms: vec![],
            args: ArgsOverride::default(),
        });
    }
    store.upsert_entry(&entry).await.unwrap();
}

#[tokio::test]
async fn wanted_dumps_follow_emulators_and_pc_games() {
    let store = LibraryStore::in_memory().await.unwrap();
    let emulation = EmulationService::new(store.clone(), Bus::default(), test_catalog())
        .with_host(OperatingSystem::Linux, OsFolders::host());
    let names = |set: BTreeSet<String>| set.into_iter().collect::<Vec<_>>();
    assert!(wanted(&emulation, &store).await.unwrap().is_empty());

    emulation
        .save_emulator(emulator(&["no_dump"]))
        .await
        .unwrap();
    add_entry(&store, GameRuntime::Native, true).await;
    assert!(
        wanted(&emulation, &store).await.unwrap().is_empty(),
        "emulated games aren't PC games"
    );

    emulation
        .save_emulator(emulator(&["test_platform"]))
        .await
        .unwrap();
    assert_eq!(names(wanted(&emulation, &store).await.unwrap()), [DUMP]);

    add_entry(&store, GameRuntime::Native, false).await;
    assert_eq!(
        names(wanted(&emulation, &store).await.unwrap()),
        [WINDOWS, DUMP]
    );

    // Native games on a Mac also want the Mac dump.
    let mac = EmulationService::new(store.clone(), Bus::default(), test_catalog())
        .with_host(OperatingSystem::Macos, OsFolders::host());
    assert_eq!(
        names(wanted(&mac, &store).await.unwrap()),
        [MAC, WINDOWS, DUMP]
    );
}

#[tokio::test]
async fn windows_games_want_only_the_windows_dump_on_a_mac() {
    let store = LibraryStore::in_memory().await.unwrap();
    let mac = EmulationService::new(store.clone(), Bus::default(), test_catalog())
        .with_host(OperatingSystem::Macos, OsFolders::host());
    add_entry(
        &store,
        GameRuntime::Windows {
            prefix: None,
            runner: None,
        },
        false,
    )
    .await;
    assert_eq!(
        wanted(&mac, &store)
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        [WINDOWS]
    );
}

#[tokio::test]
async fn the_watcher_syncs_when_an_emulator_is_configured() {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    serve(&server, &zip_of(&[("Chirp (248876).json", chirp())]), None).await;
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let emulation = EmulationService::new(store.clone(), bus.clone(), test_catalog());
    let hasheous = Hasheous::with_base_url(temp.path(), server.uri());
    let watcher = hasheous.watch(&bus, emulation.clone(), store.clone());

    // After the start's sync (which has nothing to do), so the event does it.
    tokio::time::sleep(Duration::from_millis(300)).await;
    emulation
        .save_emulator(emulator(&["test_platform"]))
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while hasheous.dumps().await.unwrap().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the dump was not synced"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    watcher.abort();
    assert_eq!(downloads(&server).await, 1);
}
