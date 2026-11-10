//! RomM servers (#1) against a mock RomM API: sync into the library,
//! incremental resync, download at launch (verify, unpack, cache), offline
//! launches, resume and cancel, platform mapping and per-game overrides,
//! the cache limit, a failed sign-in, and listings and connections that go
//! wrong (ROMs lost on the server's disk, mass disappearances, stalls,
//! pages that repeat).

use caskade_core::{
    artwork::ArtworkCache,
    bus::Bus,
    db::LibraryStore,
    emulation::{
        ArgsOverride, Catalog, EmulationService, Emulator, EmulatorInstall, EmulatorProfile,
        EmulatorTarget, PlatformSettings, ProfileHooks, ProfileRef, Readiness, RomLocation,
        Tracking,
    },
    launch::{EmulatorLaunches, LaunchChoice, LaunchConfig, LaunchService},
    metadata::{Applied, IgdbMatch, MatchSource},
    model::{ArtworkKind, GameFilter, InstallLocation, InstallState, LaunchSpec, ListGames},
    romm::{
        ConnectionState, MemorySecrets, RommAuth, RommError, RommPlatformMapping, RommRom,
        RommServerConfig, RommService, register_settings,
    },
    settings::SettingsService,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{body_string_contains, header, method, path, path_regex, query_param},
};

const PLATFORMS: &str = r#"
[[platform]]
id = "nintendo_snes"
name = "Super Nintendo Entertainment System"
short = "SNES"
extensions = ["sfc", "smc", "zip"]
ids = { igdb = 19, esde = "snes" }

[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "bin", "chd", "m3u"]
ids = { igdb = 7, esde = "psx" }
"#;

const CHRONO: &[u8] = b"chrono trigger rom bytes";
const MANA: &[u8] = b"secret of mana rom bytes, a little longer";
const DISC1_CUE: &[u8] = b"FILE \"Disc 1.bin\" BINARY\n";
const DISC1_BIN: &[u8] = b"disc one data";
const DISC2_CUE: &[u8] = b"FILE \"Disc 2.bin\" BINARY\n";
const DISC2_BIN: &[u8] = b"disc two data";

fn md5(bytes: &[u8]) -> String {
    use md5::Digest;
    md5::Md5::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn file(name: &str, bytes: &[u8]) -> Value {
    json!({
        "id": 1, "file_name": name, "file_path": "roms", "file_size_bytes": bytes.len(),
        "md5_hash": md5(bytes), "category": null
    })
}

fn snes_rom(id: u32, name: &str, fs_name: &str, bytes: &[u8], updated: &str) -> Value {
    json!({
        "id": id, "platform_id": 1, "platform_slug": "snes",
        "fs_name": fs_name, "fs_name_no_ext": fs_name.trim_end_matches(".sfc"),
        "fs_extension": "sfc", "fs_size_bytes": bytes.len(),
        "name": name, "summary": format!("{name} summary"),
        "regions": ["USA"], "languages": ["En"],
        // As RomM sends it: the resources prefix and a cache-busting query.
        "path_cover_large": format!(
            "/assets/romm/resources/roms/1/{id}/cover/big.png?ts=2026-02-09 09:41:41.675419+00:00"
        ),
        // The title and blanks are no aliases.
        "alternative_names": [format!("{name} (JP)"), name, " "],
        "merged_screenshots": [
            format!("/assets/romm/resources/roms/1/{id}/screenshots/0.jpg"),
            format!("/assets/romm/resources/roms/1/{id}/screenshots/1.jpg"),
        ],
        "has_multiple_files": false,
        "files": [file(fs_name, bytes)],
        "md5_hash": md5(bytes),
        "updated_at": updated, "missing_from_fs": false,
        "metadatum": { "genres": ["RPG"], "first_release_date": 795_225_600_000_i64 }
    })
}

fn ps1_rom() -> Value {
    json!({
        "id": 20, "platform_id": 2, "platform_slug": "ps",
        "fs_name": "Final Fantasy VII", "fs_name_no_ext": "Final Fantasy VII",
        "fs_extension": "", "fs_size_bytes": 60,
        "name": "Final Fantasy VII", "has_multiple_files": true,
        "files": [
            file("Disc 1.cue", DISC1_CUE), file("Disc 1.bin", DISC1_BIN),
            file("Disc 2.cue", DISC2_CUE), file("Disc 2.bin", DISC2_BIN)
        ],
        "updated_at": "2026-01-01T00:00:00Z"
    })
}

fn page(items: Vec<Value>) -> Value {
    json!({ "items": items, "total": items.len(), "limit": 250, "offset": 0 })
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in files {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 90])))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

async fn authed(server: &MockServer, route: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(route))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn roms(server: &MockServer, platform: &str, items: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", platform))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(items)))
        .mount(server)
        .await;
}

async fn content(server: &MockServer, id: u32, bytes: Vec<u8>) {
    Mock::given(method("GET"))
        .and(path_regex(format!("^/api/roms/{id}/content/.+$")))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .mount(server)
        .await;
}

/// The RomM API: sign-in, platforms, ROMs, content and covers.
async fn mount(server: &MockServer, snes: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path("/api/heartbeat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "SYSTEM": { "VERSION": "4.4.0" } })),
        )
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/token"))
        .and(body_string_contains("grant_type=password"))
        .and(body_string_contains("username=admin"))
        .and(body_string_contains("password=secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "tok", "refresh_token": "refresh",
            "token_type": "bearer", "expires": 900
        })))
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/token"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({ "detail": "Invalid username or password" })),
        )
        .with_priority(10)
        .mount(server)
        .await;
    authed(
        server,
        "/api/platforms",
        json!([
            { "id": 1, "slug": "snes", "fs_slug": "snes", "name": "Super Nintendo Entertainment System", "rom_count": 2, "igdb_id": 19 },
            { "id": 2, "slug": "ps", "fs_slug": "psx", "name": "PlayStation", "rom_count": 1 },
            { "id": 3, "slug": "vectrex", "fs_slug": "vectrex", "name": "Vectrex", "rom_count": 1 }
        ]),
    )
    .await;
    authed(server, "/api/collections", json!([])).await;
    roms(server, "1", snes).await;
    roms(server, "2", vec![ps1_rom()]).await;
    content(server, 10, CHRONO.to_vec()).await;
    content(server, 11, MANA.to_vec()).await;
    content(
        server,
        20,
        zip(&[
            ("Disc 1.cue", DISC1_CUE),
            ("Disc 1.bin", DISC1_BIN),
            ("Disc 2.cue", DISC2_CUE),
            ("Disc 2.bin", DISC2_BIN),
        ]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path_regex(
            "^/assets/romm/resources/roms/.+/(cover/big.png|screenshots/.+)$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png()))
        .mount(server)
        .await;
}

fn default_snes() -> Vec<Value> {
    vec![
        snes_rom(
            10,
            "Chrono Trigger",
            "Chrono Trigger (USA).sfc",
            CHRONO,
            "2026-01-01T00:00:00Z",
        ),
        snes_rom(
            11,
            "Secret of Mana",
            "Secret of Mana (USA).sfc",
            MANA,
            "2026-01-01T00:00:00Z",
        ),
    ]
}

/// How often a fixture's background syncs look for due servers.
const WATCH_EVERY: Duration = Duration::from_millis(50);

fn server_config(url: &str) -> RommServerConfig {
    RommServerConfig {
        auth: RommAuth::Password,
        username: "admin".into(),
        sync_on_startup: false,
        sync_interval: "manual".into(),
        ..RommServerConfig::new("home", "Home RomM", url)
    }
}

struct Fixture {
    temp: tempfile::TempDir,
    server: MockServer,
    store: LibraryStore,
    emulation: EmulationService,
    romm: RommService,
}

impl Fixture {
    async fn new() -> Self {
        let server = MockServer::builder().start().await;
        mount(&server, default_snes()).await;
        Self::with_server(server, "secret").await
    }

    async fn with_server(server: MockServer, password: &str) -> Self {
        Self::build(server, password, None).await
    }

    /// With a short stall (`read`) and request timeout.
    async fn with_timeouts(server: MockServer, read: Duration, request: Duration) -> Self {
        Self::build(server, "secret", Some((read, request))).await
    }

    async fn build(
        server: MockServer,
        password: &str,
        timeouts: Option<(Duration, Duration)>,
    ) -> Self {
        Self::build_with_platforms(server, password, timeouts, PLATFORMS).await
    }

    async fn build_with_platforms(
        server: MockServer,
        password: &str,
        timeouts: Option<(Duration, Duration)>,
        platforms: &str,
    ) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = LibraryStore::in_memory().await.unwrap();
        let bus = Bus::default();
        let settings = SettingsService::new(store.clone());
        register_settings(&settings).unwrap();
        let catalog = Catalog::load([("platforms.toml", platforms)]).unwrap();
        let emulation = EmulationService::new(store.clone(), bus.clone(), catalog);
        let romm = RommService::new(
            store.clone(),
            bus,
            settings,
            emulation.clone(),
            temp.path().join("fallback-cache"),
        )
        .with_secrets(Arc::new(MemorySecrets::default()))
        .with_artwork(ArtworkCache::new(temp.path().join("artwork")).unwrap())
        .with_watch_interval(WATCH_EVERY);
        let romm = match timeouts {
            Some((read, request)) => romm.with_timeouts(read, request),
            None => romm,
        };
        let mut settings = romm.settings().await.unwrap();
        settings.cache.dir = temp
            .path()
            .join("romm-cache")
            .to_string_lossy()
            .into_owned();
        romm.save_settings(settings).await.unwrap();
        romm.save_server(server_config(&server.uri()), Some(password.into()))
            .await
            .unwrap();
        Self {
            temp,
            server,
            store,
            emulation,
            romm,
        }
    }

    fn cache(&self) -> PathBuf {
        self.temp.path().join("romm-cache")
    }

    async fn record(&self, rom: u32) -> RommRom {
        self.store.romm_rom("home", rom).await.unwrap().unwrap()
    }

    /// A folder-installed emulator whose custom profile runs SNES games,
    /// the platform's default.
    async fn emulator(&self, profile: &str) -> ProfileRef {
        let dir = self.temp.path().join(format!("emulator-{profile}"));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("emulator");
        caskade_core::test_programs::write_program(&program, "#!/bin/sh\nsleep 1\n");
        let view = self
            .emulation
            .save_emulator(Emulator {
                id: format!("emu-{profile}"),
                name: format!("Emulator {profile}"),
                definition: None,
                install: EmulatorInstall::Folder {
                    dir,
                    executable: "emulator".into(),
                },
                profiles: vec![EmulatorProfile::Custom {
                    id: profile.into(),
                    name: format!("Profile {profile}"),
                    executable: None,
                    args: vec!["{RomPath}".into()],
                    working_dir: None,
                    platforms: vec!["nintendo_snes".into()],
                    extensions: vec!["sfc".into()],
                    env: Vec::new(),
                    tracking: Tracking::Default,
                    hooks: ProfileHooks::default(),
                }],
            })
            .await
            .unwrap();
        ProfileRef {
            emulator_id: view.emulator.id.clone(),
            profile_id: profile.into(),
        }
    }

    async fn requests(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .len()
    }
}

fn emulated(spec: &Option<LaunchSpec>) -> (&str, &EmulatorTarget, Vec<PathBuf>) {
    let Some(LaunchSpec::Emulator {
        platform_id,
        target,
        roms,
        ..
    }) = spec
    else {
        panic!("not an emulated entry: {spec:?}");
    };
    let paths = roms
        .iter()
        .map(|rom| match &rom.location {
            RomLocation::Absolute { path } => path.clone(),
            other => panic!("unexpected ROM location {other:?}"),
        })
        .collect();
    (platform_id, target, paths)
}

#[tokio::test]
async fn a_platform_without_a_shipped_emulator_or_extensions_still_imports() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    roms(
        &server,
        "3",
        vec![json!({
            "id": 30, "platform_id": 3, "fs_name": "Mine Storm.vec", "name": "Mine Storm"
        })],
    )
    .await;
    let platforms = format!(
        r#"{PLATFORMS}
[[platform]]
id = "vectrex"
name = "GCE Vectrex"
short = "Vectrex"
ids = {{ romm = ["vectrex"] }}
"#
    );
    let fixture = Fixture::build_with_platforms(server, "secret", None, &platforms).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.added, 4);
    assert!(report.unmatched_platforms.is_empty());
    assert!(report.warnings.is_empty());
    let record = fixture.record(30).await;
    assert_eq!(record.platform_id, "vectrex");
    let entry = fixture
        .store
        .get_entry(&record.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(emulated(&entry.launch).0, "vectrex");
    let view = fixture
        .emulation
        .platforms()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.platform.id == "vectrex")
        .unwrap();
    assert_eq!(view.rom_count, 1);
    assert!(view.compatible.is_empty());
    assert!(view.settings.default.is_none());
    let again = fixture.romm.sync("home").await.unwrap();
    assert_eq!(again.added, 0);
    assert_eq!(again.unchanged, 4);
}

#[tokio::test]
async fn sync_imports_the_selected_platforms_and_a_resync_keeps_user_fields() {
    let fixture = Fixture::new().await;
    let report = fixture
        .romm
        .test_connection(server_config(&fixture.server.uri()), None)
        .await
        .unwrap();
    assert_eq!(report.version.as_deref(), Some("4.4.0"));
    let suggested: Vec<_> = report
        .platforms
        .iter()
        .map(|found| (found.platform.slug.as_str(), found.suggested.as_deref()))
        .collect();
    assert_eq!(
        suggested,
        [
            ("snes", Some("nintendo_snes")),
            ("ps", Some("sony_playstation")),
            ("vectrex", None)
        ]
    );

    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!((report.added, report.updated, report.removed), (3, 0, 0));
    assert_eq!(report.unmatched_platforms, ["Vectrex"]);

    // Searchable, installed-looking and filterable like local games.
    let found = fixture
        .store
        .list_games(&ListGames {
            query: Some("Chrono".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found.items.len(), 1);
    let chrono = &found.items[0];
    assert!(chrono.installed);
    assert_eq!(chrono.platforms, ["Super Nintendo Entertainment System"]);
    assert_eq!(chrono.genres, ["RPG"]);
    assert!(chrono.cover.is_some(), "the server's cover is imported");
    for filter in [GameFilter::Installed, GameFilter::Emulated] {
        let page = fixture
            .store
            .list_games(&ListGames {
                filter,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.total, 3, "{filter:?}");
    }
    let details = fixture.store.get_game(&chrono.id).await.unwrap().unwrap();
    assert_eq!(details.game.release_date.as_deref(), Some("1995-03-15"));
    assert_eq!(details.game.aliases, ["Chrono Trigger (JP)"]);
    // The sync brings the cover; the screenshots wait for the game's page.
    let kinds = |details: &caskade_core::model::GameDetails| -> Vec<ArtworkKind> {
        details.artwork.iter().map(|art| art.kind).collect()
    };
    assert_eq!(kinds(&details), [ArtworkKind::Cover]);
    assert!(fixture.romm.page_images(&chrono.id).await.unwrap());
    let details = fixture.store.get_game(&chrono.id).await.unwrap().unwrap();
    assert_eq!(
        kinds(&details),
        [
            ArtworkKind::Cover,
            ArtworkKind::Screenshot,
            ArtworkKind::Screenshot
        ]
    );
    assert!(!fixture.romm.page_images(&chrono.id).await.unwrap());
    let entry = &details.entries[0];
    assert_eq!(entry.source_id.as_str(), "romm:home");
    assert_eq!(entry.external_id, "10");
    assert_eq!(entry.state, InstallState::Installed);
    let (platform, target, roms) = emulated(&entry.launch);
    assert_eq!(platform, "nintendo_snes");
    assert_eq!(*target, EmulatorTarget::PlatformDefault);
    assert!(roms[0].starts_with(fixture.cache()));
    // Not downloaded, but not "ROM missing" either: it downloads at launch.
    let game = fixture.emulation.game(&entry.id).await.unwrap();
    assert!(!matches!(game.readiness, Readiness::RomMissing { .. }));

    // The user's own fields.
    let mut game = details.game.clone();
    game.favorite = true;
    game.rating = Some(90);
    game.notes = "Play the DS version next".into();
    game.title = "Chrono Trigger (my copy)".into();
    game.aliases.push("CT".into());
    fixture.store.upsert_game(&game).await.unwrap();

    // The server changes one ROM and deletes another.
    fixture.server.reset().await;
    let mut changed = snes_rom(
        10,
        "Chrono Trigger",
        "Chrono Trigger (USA).sfc",
        CHRONO,
        "2026-02-01T00:00:00Z",
    );
    changed["summary"] = json!("A new summary.");
    changed["alternative_names"] = json!(["Kurono Torigā"]);
    mount(&fixture.server, vec![changed]).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(
        (
            report.added,
            report.updated,
            report.unchanged,
            report.removed
        ),
        (0, 1, 1, 1)
    );
    let details = fixture.store.get_game(&chrono.id).await.unwrap().unwrap();
    assert!(details.game.favorite);
    assert_eq!(details.game.rating, Some(90));
    assert_eq!(details.game.notes, "Play the DS version next");
    assert_eq!(details.game.title, "Chrono Trigger (my copy)");
    assert_eq!(details.game.description, "A new summary.");
    // The server's old name goes; the user's stays.
    assert_eq!(details.game.aliases, ["CT", "Kurono Torigā"]);
    // Unchanged screenshots aren't imported twice.
    let screenshots = details
        .artwork
        .iter()
        .filter(|art| art.kind == ArtworkKind::Screenshot)
        .count();
    assert_eq!(screenshots, 2);
    let all = fixture
        .store
        .list_games(&ListGames::default())
        .await
        .unwrap();
    assert_eq!(all.total, 2, "Secret of Mana left with the server");
}

#[tokio::test]
async fn a_resync_retries_covers_that_failed_to_import() {
    let server = MockServer::builder().start().await;
    Mock::given(method("GET"))
        .and(path_regex("^/assets/romm/resources/"))
        .respond_with(ResponseTemplate::new(404))
        .with_priority(1)
        .mount(&server)
        .await;
    mount(&server, default_snes()).await;
    let fixture = Fixture::with_server(server, "secret").await;
    let chrono = async |fixture: &Fixture| {
        let found = fixture
            .store
            .list_games(&ListGames {
                query: Some("Chrono".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        found.items[0].clone()
    };

    let cover_requests = async |fixture: &Fixture| {
        fixture
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path().ends_with("/cover/big.png"))
            .count()
    };

    fixture.romm.sync("home").await.unwrap();
    let game = chrono(&fixture).await;
    assert!(game.cover.is_none());
    assert!(!fixture.romm.page_images(&game.id).await.unwrap());
    let details = fixture.store.get_game(&game.id).await.unwrap().unwrap();
    assert!(details.artwork.is_empty());
    // A cover the server refused isn't asked for at every sync.
    let asked = cover_requests(&fixture).await;
    assert!(asked > 0);
    fixture.romm.sync("home").await.unwrap();
    assert_eq!(cover_requests(&fixture).await, asked);

    // A week on, the server's covers work again, though the ROMs are
    // unchanged.
    let mut record = fixture.record(10).await;
    let failed = record
        .cover_unavailable
        .as_mut()
        .expect("the 404 is recorded");
    failed.at_ms -= caskade_core::romm::COVER_RETRY.as_millis() as i64;
    fixture.store.save_romm_rom(&record).await.unwrap();
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.added, 0);
    let game = chrono(&fixture).await;
    assert!(game.cover.is_some());
    assert!(fixture.record(10).await.cover_unavailable.is_none());
    // The page tries its screenshots again too.
    assert!(fixture.romm.page_images(&game.id).await.unwrap());
    let details = fixture.store.get_game(&game.id).await.unwrap().unwrap();
    assert_eq!(details.artwork.len(), 3, "the cover and both screenshots");
}

#[tokio::test]
async fn romms_values_and_artwork_replace_what_igdb_filled_in() {
    let server = MockServer::builder().start().await;
    let rom = |updated: &str| {
        snes_rom(
            10,
            "Chrono Trigger",
            "Chrono Trigger (USA).sfc",
            CHRONO,
            updated,
        )
    };
    // At first RomM has no summary or artwork for the ROM.
    let mut bare = rom("2026-01-01T00:00:00Z");
    bare["summary"] = Value::Null;
    bare["path_cover_large"] = Value::Null;
    bare["merged_screenshots"] = json!([]);
    mount(&server, vec![bare]).await;
    let fixture = Fixture::with_server(server, "secret").await;
    fixture.romm.sync("home").await.unwrap();
    let id = fixture
        .store
        .list_games(&ListGames {
            query: Some("Chrono".into()),
            ..Default::default()
        })
        .await
        .unwrap()
        .items[0]
        .id
        .clone();

    // IGDB fills the description in and adds its cover and a screenshot.
    let mut game = fixture.store.get_game(&id).await.unwrap().unwrap().game;
    game.description = "From IGDB.".into();
    fixture.store.upsert_game(&game).await.unwrap();
    fixture
        .store
        .save_igdb_match(&IgdbMatch {
            game_id: id.clone(),
            igdb_id: Some(1),
            source: MatchSource::Romm,
            pending: false,
            checked_at_ms: 0,
            dumps: String::new(),
            applied: Applied {
                description: Some("From IGDB.".into()),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let cache = ArtworkCache::new(fixture.temp.path().join("artwork")).unwrap();
    let image = cache.import_bytes(png()).await.unwrap();
    for (kind, source) in [
        (ArtworkKind::Cover, "igdb:co1"),
        (ArtworkKind::Screenshot, "igdb:sc1"),
    ] {
        fixture
            .store
            .upsert_artwork(&image.record(&id, kind, source))
            .await
            .unwrap();
    }

    // RomM gets a summary and artwork of its own.
    fixture.server.reset().await;
    mount(&fixture.server, vec![rom("2026-02-01T00:00:00Z")]).await;
    fixture.romm.sync("home").await.unwrap();
    fixture.romm.page_images(&id).await.unwrap();
    let details = fixture.store.get_game(&id).await.unwrap().unwrap();
    assert_eq!(details.game.description, "Chrono Trigger summary");
    let art: Vec<_> = details
        .artwork
        .iter()
        .map(|art| (art.kind, art.source.as_str()))
        .collect();
    assert_eq!(
        art,
        [
            (ArtworkKind::Cover, "romm:home"),
            (ArtworkKind::Screenshot, "romm:home"),
            (ArtworkKind::Screenshot, "romm:home"),
        ]
    );
}

#[tokio::test]
async fn a_failed_sign_in_is_reported() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    let fixture = Fixture::with_server(server, "wrong").await;
    let error = fixture
        .romm
        .test_connection(server_config(&fixture.server.uri()), None)
        .await
        .unwrap_err();
    assert!(matches!(error, RommError::Auth(_)), "{error}");
    assert!(error.to_string().contains("Invalid username or password"));
    assert!(matches!(
        fixture.romm.sync("home").await,
        Err(RommError::Auth(_))
    ));
    // A refused sign-in needs a new one, not a better connection (#73).
    let overview = fixture.romm.overview().await.unwrap();
    assert_eq!(overview.servers[0].state, ConnectionState::SignInNeeded);
    assert!(overview.servers[0].has_secret);
    assert_eq!(
        fixture
            .store
            .list_games(&ListGames::default())
            .await
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn launching_downloads_verifies_and_caches_then_plays_offline() {
    let fixture = Fixture::new().await;
    let profile = fixture.emulator("snes").await;
    fixture
        .emulation
        .set_platform_settings(PlatformSettings {
            platform: "nintendo_snes".into(),
            default: Some(profile),
            bios_dir: None,
        })
        .await
        .unwrap();
    fixture.romm.sync("home").await.unwrap();
    let record = fixture.record(10).await;
    let mut progress = fixture.romm.subscribe();

    let launch = LaunchService::new(fixture.store.clone())
        .with_emulators(EmulatorLaunches::new(
            fixture.store.clone(),
            fixture.emulation.clone(),
        ))
        .with_preparer(Arc::new(fixture.romm.clone()))
        .with_config(LaunchConfig {
            poll_interval: Duration::from_millis(20),
            launch_timeout: Duration::from_secs(10),
            exit_grace: Duration::from_millis(100),
            heartbeat_interval: Duration::from_millis(200),
        });
    #[cfg(unix)]
    {
        let handle = launch.launch(&record.entry_id).await.unwrap();
        let session = handle.wait().await.unwrap();
        assert_eq!(session.game_id, record.game_id);
        let sessions = fixture
            .store
            .sessions_for_game(&record.game_id, 10)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1, "a play session is recorded");
    }
    #[cfg(not(unix))]
    {
        // No shell script emulator here: prepare as the launch would.
        let _ = &launch;
        assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    }

    let mut phases = Vec::new();
    while let Ok(update) = progress.try_recv() {
        phases.push(update.phase);
    }
    assert_eq!(
        phases.first(),
        Some(&caskade_core::romm::DownloadPhase::Downloading)
    );
    assert_eq!(
        phases.last(),
        Some(&caskade_core::romm::DownloadPhase::Done)
    );

    let record = fixture.record(10).await;
    let cache = record.cache.clone().expect("cached");
    assert_eq!(cache.dir, fixture.cache().join("home").join("10"));
    assert_eq!(std::fs::read(&cache.roms[0]).unwrap(), CHRONO);
    assert!(!fixture.cache().join(".staging/home-10").exists());
    let entry = fixture
        .store
        .get_entry(&record.entry_id)
        .await
        .unwrap()
        .unwrap();
    let (_, _, roms) = emulated(&entry.launch);
    assert_eq!(roms, cache.roms);
    assert_eq!(
        entry.location,
        Some(InstallLocation::External {
            path: cache.dir.clone()
        })
    );
    let prepared = EmulatorLaunches::new(fixture.store.clone(), fixture.emulation.clone())
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(
        prepared.command.args,
        [cache.roms[0].to_string_lossy().into_owned()]
    );

    // The second launch needs no network, even with the server gone.
    let before = fixture.requests().await;
    assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    assert_eq!(fixture.requests().await, before);
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let (romm, store) = (fixture.romm.clone(), fixture.store.clone());
    romm.save_server(server_config(&format!("http://{closed}")), None)
        .await
        .unwrap();
    assert!(romm.prepare_entry(&record.entry_id).await.unwrap());

    // An uncached game is unavailable, with a clear message.
    let mana = store.romm_rom("home", 11).await.unwrap().unwrap();
    let error = romm.prepare_entry(&mana.entry_id).await.unwrap_err();
    assert!(error.to_string().contains("can't be reached"), "{error}");
    let status = romm.game_status(&mana.entry_id).await.unwrap().unwrap();
    assert_eq!(status.status, "unavailable");
    let entry = store.get_entry(&mana.entry_id).await.unwrap().unwrap();
    assert!(matches!(entry.state, InstallState::Broken { .. }));
    // The cached one still is.
    let status = romm.game_status(&record.entry_id).await.unwrap().unwrap();
    assert_eq!(status.status, "cached");
}

#[tokio::test]
async fn a_multi_file_rom_is_verified_unpacked_and_gets_a_playlist() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    let record = fixture.record(20).await;
    assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    let cache = fixture.record(20).await.cache.unwrap();
    assert_eq!(cache.roms, [cache.dir.join("Final Fantasy VII.m3u")]);
    assert_eq!(
        std::fs::read_to_string(&cache.roms[0]).unwrap(),
        "Disc 1.cue\nDisc 2.cue\n"
    );
    assert_eq!(
        std::fs::read(cache.dir.join("Disc 2.bin")).unwrap(),
        DISC2_BIN
    );
    let entry = fixture
        .store
        .get_entry(&record.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(emulated(&entry.launch).0, "sony_playstation");

    // A file that doesn't match the server's hash fails and isn't cached.
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()).await;
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/11/content/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"corrupted".to_vec()))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let mana = fixture.record(11).await;
    let error = fixture
        .romm
        .prepare_entry(&mana.entry_id)
        .await
        .unwrap_err();
    assert!(matches!(error, RommError::Verify(_)), "{error}");
    assert!(fixture.record(11).await.cache.is_none());
}

#[tokio::test]
async fn an_interrupted_download_resumes_and_a_download_can_be_cancelled() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    // Half of Secret of Mana is already on disk.
    let staging = fixture.cache().join(".staging/home-11");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("download.part"), &MANA[..10]).unwrap();
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/11/content/.+$"))
        .and(header("range", "bytes=10-"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header(
                    "content-range",
                    format!("bytes 10-{}/{}", MANA.len() - 1, MANA.len()).as_str(),
                )
                .set_body_bytes(MANA[10..].to_vec()),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let mana = fixture.record(11).await;
    assert!(fixture.romm.prepare_entry(&mana.entry_id).await.unwrap());
    let cache = fixture.record(11).await.cache.unwrap();
    assert_eq!(std::fs::read(&cache.roms[0]).unwrap(), MANA);
    let ranged: Vec<Request> = fixture
        .server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.headers.contains_key("range"))
        .collect();
    assert_eq!(ranged.len(), 1);

    // A slow download, cancelled.
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/10/content/.+$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(CHRONO.to_vec())
                .set_delay(Duration::from_secs(30)),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let chrono = fixture.record(10).await;
    let romm = fixture.romm.clone();
    let entry = chrono.entry_id.clone();
    let running = tokio::spawn(async move { romm.prepare_entry(&entry).await });
    for _ in 0..200 {
        if fixture.romm.cancel_download(&chrono.entry_id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let outcome = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("cancelling stops the download")
        .unwrap();
    assert!(matches!(outcome, Err(RommError::Cancelled)));
    assert!(fixture.record(10).await.cache.is_none());
}

#[tokio::test]
async fn the_platform_mapping_and_a_per_game_override_take_effect_at_launch() {
    let fixture = Fixture::new().await;
    let first = fixture.emulator("first").await;
    let second = fixture.emulator("second").await;
    fixture.romm.sync("home").await.unwrap();

    // Map SNES to the first emulator.
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.servers[0].platforms = vec![RommPlatformMapping {
        slug: "snes".into(),
        enabled: true,
        platform: Some("nintendo_snes".into()),
        emulator_id: Some(first.emulator_id.clone()),
        profile_id: Some(first.profile_id.clone()),
    }];
    fixture.romm.save_settings(settings).await.unwrap();
    let chrono = fixture.record(10).await;
    assert!(fixture.romm.prepare_entry(&chrono.entry_id).await.unwrap());
    let entry = fixture
        .store
        .get_entry(&chrono.entry_id)
        .await
        .unwrap()
        .unwrap();
    let expected = EmulatorTarget::Profile {
        emulator_id: first.emulator_id.clone(),
        profile_id: first.profile_id.clone(),
    };
    assert_eq!(*emulated(&entry.launch).1, expected);

    // The game page pins Secret of Mana to the second emulator; a resync
    // and its launch keep that.
    let mana = fixture.record(11).await;
    let override_target = EmulatorTarget::Profile {
        emulator_id: second.emulator_id.clone(),
        profile_id: second.profile_id.clone(),
    };
    fixture
        .emulation
        .set_game(
            &mana.entry_id,
            override_target.clone(),
            ArgsOverride::Default,
        )
        .await
        .unwrap();
    fixture.romm.sync("home").await.unwrap();
    assert!(fixture.romm.prepare_entry(&mana.entry_id).await.unwrap());
    let entry = fixture
        .store
        .get_entry(&mana.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*emulated(&entry.launch).1, override_target);
    let prepared = EmulatorLaunches::new(fixture.store.clone(), fixture.emulation.clone())
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(
        prepared.command.emulation.unwrap().emulator_id,
        second.emulator_id
    );
}

/// A cached record with a pretend size (the limit is in GiB).
async fn cached(fixture: &Fixture, rom: u32, gib_tenths: u64, used: i64, pinned: bool) -> RommRom {
    let mut record = fixture.record(rom).await;
    let dir = fixture.cache().join("home").join(rom.to_string());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("rom.sfc"), b"x").unwrap();
    record.pinned = pinned;
    record.cache = Some(caskade_core::romm::CachedRom {
        roms: vec![dir.join("rom.sfc")],
        dir,
        size: (gib_tenths << 30) / 10,
        cached_at_ms: used,
        last_used_ms: used,
    });
    fixture.store.save_romm_rom(&record).await.unwrap();
    record
}

#[tokio::test]
async fn the_cache_limit_evicts_least_recently_played_but_never_pinned_or_favourites() {
    let server = MockServer::builder().start().await;
    let snes: Vec<Value> = (10..16)
        .map(|id| {
            snes_rom(
                id,
                &format!("Game {id}"),
                &format!("Game {id}.sfc"),
                CHRONO,
                "2026-01-01T00:00:00Z",
            )
        })
        .collect();
    mount(&server, snes).await;
    let fixture = Fixture::with_server(server, "secret").await;
    fixture.romm.sync("home").await.unwrap();
    // Oldest first: 10 pinned, 11 a favourite, 12 and 13 plain, 14 newest.
    let pinned = cached(&fixture, 10, 4, 1, true).await;
    let favourite = cached(&fixture, 11, 4, 2, false).await;
    let old = cached(&fixture, 12, 4, 3, false).await;
    let older_used = cached(&fixture, 13, 4, 4, false).await;
    let newest = cached(&fixture, 14, 4, 5, false).await;
    let mut game = fixture
        .store
        .game(&favourite.game_id)
        .await
        .unwrap()
        .unwrap();
    game.favorite = true;
    fixture.store.upsert_game(&game).await.unwrap();

    // 2 GiB cached, 1 GiB allowed.
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.cache.limit_gb = 1;
    let overview = fixture.romm.save_settings(settings).await.unwrap();
    let kept: Vec<u32> = overview
        .cache
        .games
        .iter()
        .map(|game| game.rom_id)
        .collect();
    assert!(kept.contains(&pinned.rom_id));
    assert!(kept.contains(&favourite.rom_id));
    assert!(
        !kept.contains(&old.rom_id),
        "least recently played goes first"
    );
    assert!(!kept.contains(&older_used.rom_id));
    assert!(!kept.contains(&newest.rom_id));
    assert!(!old.cache.unwrap().dir.exists());
    assert!(pinned.cache.unwrap().dir.exists());
    // Evicted games stay in the library and download again at launch.
    let entry = fixture
        .store
        .get_entry(&old.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.state, InstallState::Installed);
    let status = fixture
        .romm
        .game_status(&old.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.status, "remote");

    // A ROM deleted on the server stays while cached, then goes with it.
    // The whole platform went at once: the first sync keeps them, the next
    // one the user starts confirms it.
    fixture.server.reset().await;
    mount(&fixture.server, Vec::new()).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!((report.orphaned, report.removed), (0, 0));
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.orphaned, 2);
    assert_eq!(report.removed, 4);
    assert!(fixture.romm.evict(&favourite.entry_id).await.unwrap());
    assert!(
        fixture
            .store
            .get_entry(&favourite.entry_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!Path::new(&fixture.cache().join("home/11")).exists());
}

#[tokio::test]
async fn bios_files_come_from_the_servers_firmware_when_their_md5_matches() {
    use caskade_core::emulation::{Bios, BiosProvider};
    const BIOS: &[u8] = b"playstation bios";
    const OTHER: &[u8] = b"another dump";
    let f = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/api/firmware"))
        .and(query_param("platform_id", "2"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": 7, "platform_id": 2, "file_name": "SCPH5501.BIN", "md5_hash": md5(BIOS), "missing_from_fs": false },
            { "id": 8, "platform_id": 2, "file_name": "scph5502.bin", "md5_hash": md5(OTHER), "missing_from_fs": false },
            { "id": 9, "platform_id": 2, "file_name": "scph1001.bin", "md5_hash": null, "missing_from_fs": true }
        ])))
        .mount(&f.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/firmware/7/content/SCPH5501.BIN"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BIOS))
        .mount(&f.server)
        .await;
    let bios = |file: &str, md5: Option<String>| Bios {
        file: file.into(),
        md5,
        region: None,
        required: true,
    };
    let wanted = [
        bios("scph5501.bin", Some(md5(BIOS))),
        // Not the dump the definition names.
        bios("scph5502.bin", Some(md5(b"the right dump"))),
        // Gone from the server's disk.
        bios("scph1001.bin", None),
    ];
    let dir = f.temp.path().join("bios");
    let fetched = BiosProvider::fetch(&f.romm, "sony_playstation", &wanted, &dir)
        .await
        .unwrap();
    assert_eq!(fetched, ["scph5501.bin"]);
    assert_eq!(std::fs::read(dir.join("scph5501.bin")).unwrap(), BIOS);
    assert!(!dir.join("scph5502.bin").exists());
    // Written beside it and renamed: nothing half-written is left.
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["scph5501.bin"]);
    // Platforms the server doesn't map to fetch nothing.
    let none = BiosProvider::fetch(&f.romm, "nintendo_gba", &wanted, &dir)
        .await
        .unwrap();
    assert!(none.is_empty());
}

/// RomM hashes what an archive holds, not the archive: a zipped ROM's
/// SHA-1 is its contents', every member in path order.
#[tokio::test]
async fn zipped_roms_are_checked_by_what_they_hold_as_romm_hashes_them() {
    const BUGS: &[u8] = b"a bug's life game boy color rom";
    const CUE: &[u8] = b"FILE \"Ape Escape.bin\" BINARY\n";
    const BIN: &[u8] = b"ape escape disc data";
    let sha1 = |parts: &[&[u8]]| {
        use sha1::Digest;
        let mut hasher = sha1::Sha1::new();
        for part in parts {
            hasher.update(part);
        }
        format!("{:x}", hasher.finalize())
    };
    let zipped = |id: u32, platform: u32, slug: &str, fs_name: &str, bytes: &[u8], sha1: String| {
        json!({
            "id": id, "platform_id": platform, "platform_slug": slug,
            "fs_name": fs_name, "fs_name_no_ext": fs_name.trim_end_matches(".zip"),
            "fs_extension": "zip", "fs_size_bytes": bytes.len(),
            "name": fs_name.trim_end_matches(".zip"), "has_multiple_files": false,
            "files": [{
                "id": id, "file_name": fs_name, "file_path": "roms",
                "file_size_bytes": bytes.len(), "sha1_hash": sha1, "category": "game"
            }],
            "sha1_hash": sha1,
            "updated_at": "2026-01-01T00:00:00Z", "missing_from_fs": false
        })
    };
    let server = MockServer::builder().start().await;
    let bugs = zip(&[("Bug's Life, A (U) [C][!].sfc", BUGS)]);
    let other = zip(&[("Other.sfc", b"other bytes")]);
    let mut snes = default_snes();
    snes.push(zipped(
        12,
        1,
        "snes",
        "Bug's Life, A (U) [C][!].zip",
        &bugs,
        sha1(&[BUGS]),
    ));
    // RomM hashed something else than what the server sends.
    snes.push(zipped(
        13,
        1,
        "snes",
        "Other.zip",
        &other,
        sha1(&[b"not these bytes"]),
    ));
    mount(&server, snes).await;
    content(&server, 12, bugs).await;
    content(&server, 13, other).await;
    // A disc in one zip: RomM hashes its members sorted by name.
    let ape = zip(&[("Ape Escape.cue", CUE), ("Ape Escape.bin", BIN)]);
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![
            ps1_rom(),
            zipped(21, 2, "ps", "Ape Escape.zip", &ape, sha1(&[BIN, CUE])),
        ])))
        .with_priority(1)
        .mount(&server)
        .await;
    content(&server, 21, ape).await;
    let fixture = Fixture::with_server(server, "secret").await;
    fixture.romm.sync("home").await.unwrap();

    // SNES takes zips: the archive is kept for the emulator.
    let record = fixture.record(12).await;
    assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    let cache = fixture.record(12).await.cache.unwrap();
    assert_eq!(cache.roms, [cache.dir.join("Bug's Life, A (U) [C][!].zip")]);
    assert_eq!(std::fs::read_dir(&cache.dir).unwrap().count(), 1);

    // PS1 doesn't: the disc is unpacked and checked as unpacked.
    let record = fixture.record(21).await;
    assert!(fixture.romm.prepare_entry(&record.entry_id).await.unwrap());
    let cache = fixture.record(21).await.cache.unwrap();
    assert_eq!(cache.roms, [cache.dir.join("Ape Escape.cue")]);

    let record = fixture.record(13).await;
    let error = fixture
        .romm
        .prepare_entry(&record.entry_id)
        .await
        .unwrap_err();
    assert!(matches!(error, RommError::Verify(_)), "{error}");
    assert!(fixture.record(13).await.cache.is_none());
}

async fn entry_state(fixture: &Fixture, rom: u32) -> InstallState {
    let record = fixture.record(rom).await;
    let entry = fixture.store.get_entry(&record.entry_id).await.unwrap();
    entry.expect("still in the library").state
}

async fn server_status(fixture: &Fixture) -> caskade_core::romm::ServerStatus {
    let overview = fixture.romm.overview().await.unwrap();
    overview.servers.into_iter().next().unwrap()
}

/// The RomM host's NAS goes away and a rescan flags every ROM as missing:
/// the games stay, unavailable, and come back with the files.
#[tokio::test]
async fn roms_lost_on_the_servers_disk_stay_unavailable() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    let missing = |mut rom: Value| {
        rom["missing_from_fs"] = json!(true);
        rom
    };
    // Every game, and a new ROM that was never in the library.
    let mut snes: Vec<Value> = default_snes().into_iter().map(missing).collect();
    snes.push(missing(snes_rom(
        12,
        "Lost",
        "Lost.sfc",
        CHRONO,
        "2026-01-01T00:00:00Z",
    )));
    fixture.server.reset().await;
    mount(&fixture.server, snes).await;
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![missing(ps1_rom())])))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!((report.added, report.removed, report.orphaned), (0, 0, 0));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    for rom in [10, 11, 20] {
        assert!(
            matches!(
                entry_state(&fixture, rom).await,
                InstallState::Broken { .. }
            ),
            "{rom}"
        );
    }
    let chrono = fixture.record(10).await;
    let status = fixture
        .romm
        .game_status(&chrono.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.status, "unavailable");
    assert!(status.message.unwrap().contains("can't find"));
    assert!(fixture.store.romm_rom("home", 12).await.unwrap().is_none());

    // The files are back.
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()).await;
    fixture.romm.sync("home").await.unwrap();
    for rom in [10, 11, 20] {
        assert_eq!(
            entry_state(&fixture, rom).await,
            InstallState::Installed,
            "{rom}"
        );
    }
    let status = fixture
        .romm
        .game_status(&chrono.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.status, "remote");
}

/// A listing that loses a platform's games, or most of the server's, is
/// kept back with a note until a sync of the user's finds them gone again.
#[tokio::test]
async fn games_that_vanish_at_once_stay_until_a_second_sync_confirms() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    let games = || async {
        fixture
            .store
            .list_games(&ListGames::default())
            .await
            .unwrap()
            .total
    };
    assert_eq!(games().await, 3);

    // Every SNES game gone in one listing: kept, with a note.
    fixture.server.reset().await;
    mount(&fixture.server, Vec::new()).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert_eq!(games().await, 3);
    let status = server_status(&fixture).await;
    assert_eq!(status.state, ConnectionState::Connected);
    assert!(status.error.unwrap().contains("Sync again"));
    // They are back: nothing is held any more.
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()).await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert!(report.warnings.is_empty());
    assert!(server_status(&fixture).await.error.is_none());

    // Most of the server's games gone, though no platform is empty.
    fixture.server.reset().await;
    mount(&fixture.server, default_snes()[..1].to_vec()).await;
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(Vec::new())))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(games().await, 3);
    // The user syncs again and they are still gone: now they go.
    let report = fixture.romm.sync("home").await.unwrap();
    assert_eq!(report.removed, 2);
    assert!(report.warnings.is_empty());
    assert_eq!(games().await, 1);
    assert!(server_status(&fixture).await.error.is_none());
}

/// An answer without `items` is no page of ROMs: the sync fails and
/// removes nothing.
#[tokio::test]
async fn a_rom_listing_without_items_fails_the_sync() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "detail": "upgrading" })))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Server(_)), "{error}");
    assert!(fixture.store.romm_rom("home", 10).await.unwrap().is_some());
    assert!(fixture.store.romm_rom("home", 11).await.unwrap().is_some());
}

/// A server that stalls mid-request answered: the request fails, but the
/// server isn't offline and its games stay playable.
#[tokio::test]
async fn a_stalled_server_is_not_taken_for_an_offline_one() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    let fixture =
        Fixture::with_timeouts(server, Duration::from_millis(300), Duration::from_secs(1)).await;
    fixture.romm.sync("home").await.unwrap();

    // A ROM download that stalls.
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/11/content/.+$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(MANA.to_vec())
                .set_delay(Duration::from_secs(5)),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let mana = fixture.record(11).await;
    let error = fixture
        .romm
        .prepare_entry(&mana.entry_id)
        .await
        .unwrap_err();
    assert!(matches!(error, RommError::Interrupted(_)), "{error}");
    assert_ne!(
        server_status(&fixture).await.state,
        ConnectionState::Offline
    );
    for rom in [10, 11, 20] {
        assert_eq!(
            entry_state(&fixture, rom).await,
            InstallState::Installed,
            "{rom}"
        );
    }
    let status = fixture
        .romm
        .game_status(&mana.entry_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.status, "remote");

    // A listing that stalls fails the sync the same way.
    Mock::given(method("GET"))
        .and(path("/api/platforms"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([]))
                .set_delay(Duration::from_secs(5)),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Interrupted(_)), "{error}");
    assert_ne!(
        server_status(&fixture).await.state,
        ConnectionState::Offline
    );
    assert_eq!(entry_state(&fixture, 10).await, InstallState::Installed);

    // A refused connection is offline.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    fixture
        .romm
        .save_server(server_config(&format!("http://{closed}")), None)
        .await
        .unwrap();
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Unreachable(_)), "{error}");
    // It says why, not reqwest's "error sending request".
    assert!(
        !error.to_string().contains("error sending request"),
        "{error}"
    );
    assert_eq!(
        server_status(&fixture).await.state,
        ConnectionState::Offline
    );
    assert!(matches!(
        entry_state(&fixture, 10).await,
        InstallState::Broken { .. }
    ));
}

/// A server that ignores `offset` and sends no `total` repeats its first
/// page: paging stops at the first page with nothing new.
#[tokio::test]
async fn paging_stops_when_a_page_repeats() {
    let server = MockServer::builder().start().await;
    mount(&server, Vec::new()).await;
    let items: Vec<Value> = (1000..1250)
        .map(|id| {
            json!({
                "id": id, "platform_id": 1, "platform_slug": "snes",
                "fs_name": format!("Game {id}.sfc"), "name": format!("Game {id}"),
                "files": [file(&format!("Game {id}.sfc"), CHRONO)]
            })
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/api/roms"))
        .and(query_param("platform_ids", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": items })))
        .with_priority(1)
        .mount(&server)
        .await;
    let fixture = Fixture::with_server(server, "secret").await;
    let report = tokio::time::timeout(Duration::from_secs(60), fixture.romm.sync("home"))
        .await
        .expect("paging ends")
        .unwrap();
    assert_eq!(report.added, 251);
    let pages = fixture
        .server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| {
            request.url.path() == "/api/roms"
                && request
                    .url
                    .query_pairs()
                    .any(|(key, value)| key == "platform_ids" && value == "1")
        })
        .count();
    assert_eq!(pages, 2);
}

fn closed_port() -> String {
    let addr = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    format!("http://{addr}")
}

async fn cached_ids(fixture: &Fixture) -> Vec<u32> {
    let mut ids: Vec<u32> = fixture
        .romm
        .cache_status()
        .await
        .unwrap()
        .games
        .iter()
        .map(|game| game.rom_id)
        .collect();
    ids.sort_unstable();
    ids
}

/// Play on a server that can't be reached frees no room: the download
/// never started, so other downloads stay.
#[tokio::test]
async fn play_with_the_server_unreachable_evicts_nothing() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    cached(&fixture, 10, 4, 1, false).await;
    cached(&fixture, 20, 4, 2, false).await;
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.cache.limit_gb = 1;
    fixture.romm.save_settings(settings).await.unwrap();
    assert_eq!(cached_ids(&fixture).await, [10, 20]);
    // Secret of Mana needs half a GiB more than fits.
    let mut mana = fixture.record(11).await;
    mana.rom.fs_size_bytes = 1 << 29;
    fixture.store.save_romm_rom(&mana).await.unwrap();

    fixture
        .romm
        .save_server(server_config(&closed_port()), None)
        .await
        .unwrap();
    let error = fixture
        .romm
        .prepare_entry(&mana.entry_id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("can't be reached"), "{error}");
    assert_eq!(cached_ids(&fixture).await, [10, 20]);
    for rom in [10, 20] {
        assert!(fixture.record(rom).await.cache.unwrap().dir.exists());
        assert_eq!(entry_state(&fixture, rom).await, InstallState::Installed);
    }

    // A server that answers with an error frees nothing either.
    fixture
        .romm
        .save_server(server_config(&fixture.server.uri()), None)
        .await
        .unwrap();
    Mock::given(method("GET"))
        .and(path_regex("^/api/roms/11/content/.+$"))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    fixture
        .romm
        .prepare_entry(&mana.entry_id)
        .await
        .unwrap_err();
    assert_eq!(cached_ids(&fixture).await, [10, 20]);
}

/// Automatic eviction waits for a working connection: a lower limit while
/// the server is offline evicts nothing, and the next sync that reaches it
/// evicts down to the limit.
#[tokio::test]
async fn the_cache_limit_waits_for_the_server_to_answer() {
    let fixture = Fixture::new().await;
    fixture.romm.sync("home").await.unwrap();
    cached(&fixture, 10, 4, 1, false).await;
    cached(&fixture, 11, 4, 2, false).await;
    cached(&fixture, 20, 4, 3, false).await;

    fixture
        .romm
        .save_server(server_config(&closed_port()), None)
        .await
        .unwrap();
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Unreachable(_)), "{error}");
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.cache.limit_gb = 1;
    fixture.romm.save_settings(settings).await.unwrap();
    assert_eq!(cached_ids(&fixture).await, [10, 11, 20]);
    // Unpinning enforces the limit too, and holds back the same way.
    let pinned = fixture.record(20).await;
    for pin in [true, false] {
        fixture
            .romm
            .set_pinned(&pinned.entry_id, pin)
            .await
            .unwrap();
    }
    assert_eq!(cached_ids(&fixture).await, [10, 11, 20]);
    // Downloaded games stay playable offline.
    for rom in [10, 11, 20] {
        assert_eq!(entry_state(&fixture, rom).await, InstallState::Installed);
    }

    // Back online: the sync evicts the least recently played.
    fixture
        .romm
        .save_server(server_config(&fixture.server.uri()), None)
        .await
        .unwrap();
    fixture.romm.sync("home").await.unwrap();
    assert_eq!(cached_ids(&fixture).await, [11, 20]);
    assert!(fixture.record(10).await.cache.is_none());
}

/// No request to `server` carried the saved password or a bearer token.
async fn assert_no_credentials(server: &MockServer) {
    for request in server.received_requests().await.unwrap_or_default() {
        let body = String::from_utf8_lossy(&request.body);
        assert!(
            !body.contains("password=secret"),
            "{} got the password",
            request.url
        );
        assert!(
            !request.headers.contains_key("authorization"),
            "{} got credentials",
            request.url
        );
    }
}

/// A stored password goes only to the address it was saved for (#157): a
/// test of an unsaved address, or settings that point the server elsewhere,
/// send it nowhere else.
#[tokio::test]
async fn a_saved_password_never_follows_the_server_to_another_address() {
    let fixture = Fixture::new().await;
    let other = MockServer::builder().start().await;
    mount(&other, default_snes()).await;

    // Testing another address without typing the password again.
    let error = fixture
        .romm
        .test_connection(server_config(&other.uri()), None)
        .await
        .unwrap_err();
    assert!(!matches!(error, RommError::Unreachable(_)), "{error}");
    assert_no_credentials(&other).await;

    // Settings changed underneath `save_server`.
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.servers[0].url = other.uri();
    fixture.romm.save_settings(settings).await.unwrap();
    assert!(fixture.romm.sync("home").await.is_err());
    assert_no_credentials(&other).await;
    let status = fixture.romm.overview().await.unwrap().servers;
    assert!(!status[0].has_secret, "the password isn't for this address");

    // Back at the address it was saved for, it works again.
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.servers[0].url = fixture.server.uri();
    fixture.romm.save_settings(settings).await.unwrap();
    fixture.romm.sync("home").await.unwrap();
}

/// A redirect to another host never takes the sign-in form with it (#157).
#[tokio::test]
async fn sign_in_does_not_follow_redirects_to_another_host() {
    let other = MockServer::builder().start().await;
    mount(&other, default_snes()).await;
    let server = MockServer::builder().start().await;
    Mock::given(method("POST"))
        .and(path("/api/token"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/api/token", other.uri()).as_str()),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    mount(&server, default_snes()).await;
    // Another port on the same host is another origin.
    let fixture = Fixture::with_server(server, "secret").await;
    assert!(fixture.romm.sync("home").await.is_err());
    assert_no_credentials(&other).await;
}

/// A server that signs in with an API token and syncs in the background.
fn token_server(url: &str) -> RommServerConfig {
    RommServerConfig {
        auth: RommAuth::Token,
        username: String::new(),
        sync_on_startup: true,
        sync_interval: "1h".into(),
        ..RommServerConfig::new("home", "Home RomM", url)
    }
}

/// Listings without the right bearer token get RomM's 401.
async fn refuse_unsigned(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path_regex("^/api/(platforms|collections|roms)"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({ "detail": "Not authenticated" })),
        )
        .with_priority(10)
        .mount(server)
        .await;
}

/// The requests `server` got for `route`.
async fn requests_to(server: &MockServer, route: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path() == route)
        .count()
}

/// Waits up to five seconds for the server's state to become `state`.
async fn wait_for_state(
    fixture: &Fixture,
    state: ConnectionState,
) -> caskade_core::romm::ServerStatus {
    for _ in 0..100 {
        let status = server_status(fixture).await;
        if status.state == state && !status.syncing {
            return status;
        }
        tokio::time::sleep(WATCH_EVERY).await;
    }
    panic!(
        "the server never became {state:?}: {:?}",
        server_status(fixture).await
    );
}

/// A server synced from another device arrives without its API token,
/// which stays in each device's keychain (#73): it needs signing in here,
/// and the background syncs wait for a token without asking the server.
#[tokio::test]
async fn a_server_without_a_sign_in_here_waits_for_one() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    refuse_unsigned(&server).await;
    let fixture = Fixture::with_server(server, "secret").await;
    // A new sign-in method forgets the saved password.
    fixture
        .romm
        .save_server(token_server(&fixture.server.uri()), None)
        .await
        .unwrap();
    let status = server_status(&fixture).await;
    assert_eq!(status.state, ConnectionState::SignInNeeded);
    assert!(!status.has_secret);
    assert_eq!(status.error, None);

    let asked = fixture.server.received_requests().await.unwrap().len();
    let watcher = fixture.romm.watch();
    tokio::time::sleep(WATCH_EVERY * 8).await;
    assert_eq!(
        fixture.server.received_requests().await.unwrap().len(),
        asked,
        "the background syncs asked a server they can't sign in to"
    );
    assert_eq!(
        server_status(&fixture).await.state,
        ConnectionState::SignInNeeded
    );

    // Sync now still asks, and says why it can't.
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Auth(_)), "{error}");
    let status = server_status(&fixture).await;
    assert_eq!(status.state, ConnectionState::SignInNeeded);
    assert_eq!(status.error.as_deref(), Some(error.to_string().as_str()));

    // A token entered here: the background syncs pick the server up.
    fixture
        .romm
        .save_server(token_server(&fixture.server.uri()), Some("tok".into()))
        .await
        .unwrap();
    let status = wait_for_state(&fixture, ConnectionState::Connected).await;
    assert!(status.has_secret);
    assert!(status.games > 0);
    watcher.abort();
}

/// A token the server refuses (wrong, expired or revoked) is tried once
/// in the background, then waits for a new one instead of being sent
/// again at every check (#73).
#[tokio::test]
async fn a_refused_sign_in_is_not_retried_in_the_background() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    refuse_unsigned(&server).await;
    let fixture = Fixture::with_server(server, "secret").await;
    fixture
        .romm
        .save_server(token_server(&fixture.server.uri()), Some("expired".into()))
        .await
        .unwrap();
    assert_eq!(
        server_status(&fixture).await.state,
        ConnectionState::Untested
    );

    let watcher = fixture.romm.watch();
    let status = wait_for_state(&fixture, ConnectionState::SignInNeeded).await;
    assert!(status.has_secret);
    assert!(
        status
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Not authenticated")),
        "{status:?}"
    );
    tokio::time::sleep(WATCH_EVERY * 8).await;
    assert_eq!(requests_to(&fixture.server, "/api/platforms").await, 1);

    fixture
        .romm
        .save_server(token_server(&fixture.server.uri()), Some("tok".into()))
        .await
        .unwrap();
    wait_for_state(&fixture, ConnectionState::Connected).await;
    assert_eq!(requests_to(&fixture.server, "/api/platforms").await, 2);
    watcher.abort();
}
