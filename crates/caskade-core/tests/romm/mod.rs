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
        ConnectionState, RommAuth, RommError, RommPlatformMapping, RommRom, RommServerConfig,
        RommService, register_settings,
    },
    secrets::MemorySecrets,
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

mod cache;
mod credentials;
mod sync;

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

async fn entry_state(fixture: &Fixture, rom: u32) -> InstallState {
    let record = fixture.record(rom).await;
    let entry = fixture.store.get_entry(&record.entry_id).await.unwrap();
    entry.expect("still in the library").state
}

async fn server_status(fixture: &Fixture) -> caskade_core::romm::ServerStatus {
    let overview = fixture.romm.overview().await.unwrap();
    overview.servers.into_iter().next().unwrap()
}
