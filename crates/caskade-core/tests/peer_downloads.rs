//! Downloads from a device on the local network before the internet
//! (#236), between two cores on loopback with a wiremock source: an update
//! the Desk downloaded installs on the Deck with the source gone; a file the
//! Desk sends that fails its checks is thrown away and the source used; and
//! with `core.sync.peerDownloads` off every download goes to the source.
//! Downloads outside the installer (#277): an emulator, a Hasheous dump and
//! the save manifest reach a Deck that can't reach the internet.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::install::nearby::PeerDownloads;
use caskade_core::install::{InstallOptions, InstallService, JobSnapshot, JobStatus};
use caskade_core::launch::LaunchService;
use caskade_core::model::{InstallLocation, StorageRoot};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncService;
use caskade_core::sync::blobs::BlobKind;
use caskade_core::transfer::Transfers;
use caskade_installer::manifest::RootManifest;
use caskade_installer::pipeline::input;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Core {
    store: LibraryStore,
    settings: SettingsService,
    devices: Devices,
    sync: SyncService,
    downloads: PeerDownloads,
    install: InstallService,
    games: PathBuf,
    dir: tempfile::TempDir,
}

impl Core {
    async fn id(&self) -> String {
        self.devices.status().await.this_device.id.unwrap()
    }

    async fn installed(&self, url: String) -> (String, PathBuf) {
        let job = self.install.install(url, None).await.unwrap();
        let done = finished(&self.install, &job.id).await;
        assert_eq!(done.status, JobStatus::Completed, "{done:?}");
        let game = done.game_id.unwrap();
        let entry = self.store.entries_for_game(&game).await.unwrap().remove(0);
        let Some(InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
            panic!("{:?}", entry.location);
        };
        (entry.id, self.games.join(rel_dir))
    }

    async fn update(&self, entry: &str, url: String, version: &str) -> JobSnapshot {
        let job = self
            .install
            .update_game(
                entry.into(),
                url,
                version.into(),
                LaunchService::new(self.store.clone()),
            )
            .await
            .unwrap();
        finished(&self.install, &job.id).await
    }

    async fn peer_downloads(&self, on: bool) {
        let mut section = self.settings.get("core.sync").await.unwrap().value;
        section["peerDownloads"] = on.into();
        self.settings.set("core.sync", section, None).await.unwrap();
    }

    /// Syncs with `other` until it has told this device what it keeps.
    async fn hear_from(&self, other: &Core) {
        let other_id = other.id().await;
        within("the other device says what it keeps", 30, || async {
            self.sync.sync_now(&other_id);
            tokio::time::sleep(Duration::from_millis(300)).await;
            self.downloads.heard_from(&other_id) > 0
        })
        .await;
    }
}

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

async fn finished(service: &InstallService, id: &str) -> JobSnapshot {
    for _ in 0..1200 {
        let job = service.job(id).unwrap();
        if job.status.is_finished() || job.status == JobStatus::NeedsUser {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {id} did not finish: {:?}", service.job(id));
}

async fn core(name: &str) -> Core {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(&dir.path().join("app")))
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings.clone());
    let cache = dir.path().join("cache");
    let transfers = Transfers::new(store.clone(), devices.clone(), &cache)
        .await
        .unwrap();
    devices.start();
    let downloads = PeerDownloads::new(transfers, devices.clone(), settings.clone(), &cache);
    let sync = SyncService::new(store.clone(), devices.clone());
    sync.set_settings(settings.clone());
    sync.register_blobs(Arc::new(downloads.clone()));
    let games = dir.path().join("Games");
    std::fs::create_dir_all(&games).unwrap();
    let identity = RootManifest::new(format!("{}-root", name.to_lowercase())).unwrap();
    identity.write(&games).unwrap();
    let mut root = StorageRoot::from_manifest(games.canonicalize().unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let install = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    )
    .with_settings(settings.clone())
    .with_host(input::host(Some("linux")).unwrap());
    install.set_local_source(Arc::new(downloads.clone()));
    let core = Core {
        store,
        settings,
        devices,
        sync,
        downloads,
        install,
        games: games.canonicalize().unwrap(),
        dir,
    };
    within("the endpoint starts", 20, || async {
        core.devices.status().await.this_device.listening
    })
    .await;
    core.devices.rename(name).await.unwrap();
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

/// The installed release's data file, wherever its folder put it.
fn data(dir: &std::path::Path) -> Vec<u8> {
    for name in ["data.pak", "Fixture Runner/data.pak"] {
        if let Ok(bytes) = std::fs::read(dir.join(name)) {
            return bytes;
        }
    }
    panic!(
        "no data.pak in {dir:?}: {:?}",
        std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
    );
}

/// A release of the fixture game: its program and a data file.
fn release(data: &[u8]) -> Vec<u8> {
    let program = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
    ))
    .unwrap();
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let executable = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    writer
        .start_file("Fixture Runner/fixture-runner.x86_64", executable)
        .unwrap();
    writer.write_all(&program).unwrap();
    writer
        .start_file(
            "Fixture Runner/data.pak",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(data).unwrap();
    writer.finish().unwrap().into_inner()
}

async fn source() -> MockServer {
    let server = MockServer::start().await;
    for (version, data) in [("1.0", b"levels 1.0"), ("1.1", b"levels 1.1")] {
        Mock::given(method("GET"))
            .and(path(format!("/fixture-runner-{version}.zip")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/zip")
                    .insert_header("ETag", format!("\"fixture-{version}\""))
                    .set_body_bytes(release(data)),
            )
            .mount(&server)
            .await;
    }
    server
}

async fn downloads_of(server: &MockServer, version: &str) -> usize {
    let wanted = format!("/fixture-runner-{version}.zip");
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.method.as_str() == "GET" && request.url.path() == wanted)
        .count()
}

/// The first criterion: an update the Desk downloaded installs on the Deck
/// with the internet unplugged.
#[tokio::test(flavor = "multi_thread")]
async fn an_update_downloaded_on_one_device_installs_on_another_without_the_internet() {
    let desk = core("Desk").await;
    let deck = core("Deck").await;
    pair(&desk, &deck).await;
    let (_a, _b) = (desk.sync.start(), deck.sync.start());
    let server = source().await;
    let url = |version: &str| format!("{}/fixture-runner-{version}.zip", server.uri());

    // Both have 1.0; the Desk updates to 1.1 from the internet.
    let (desk_entry, _) = desk.installed(url("1.0")).await;
    let (deck_entry, deck_dir) = deck.installed(url("1.0")).await;
    let updated = desk.update(&desk_entry, url("1.1"), "1.1").await;
    assert_eq!(updated.status, JobStatus::Completed, "{updated:?}");
    assert_eq!(downloads_of(&server, "1.1").await, 1);
    let update_url = url("1.1");

    // The internet goes; the Deck hears what the Desk keeps and updates.
    drop(server);
    deck.hear_from(&desk).await;
    let updated = deck.update(&deck_entry, update_url, "1.1").await;
    assert_eq!(updated.status, JobStatus::Completed, "{updated:?}");
    assert_eq!(data(&deck_dir), b"levels 1.1");
}

/// The second criterion: a file from the Desk that isn't what it said is
/// discarded, and the Deck downloads from the internet.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_s_file_that_fails_its_checks_is_discarded_and_downloaded_from_the_source() {
    let desk = core("Desk").await;
    let deck = core("Deck").await;
    pair(&desk, &deck).await;
    let (_a, _b) = (desk.sync.start(), deck.sync.start());
    let server = source().await;
    let url = format!("{}/fixture-runner-1.1.zip", server.uri());
    desk.installed(url.clone()).await;
    deck.hear_from(&desk).await;
    // No exchange after this one puts back what the Desk really said.
    let (a, b) = (_a, _b);
    a.abort();
    b.abort();

    // What the Deck heard, with a digest the file won't have.
    let deck_id = deck.id().await;
    let mut note = desk.downloads.announce(&deck_id).await.unwrap();
    note["kept"][0]["sha256"] = "0".repeat(64).into();
    deck.downloads.heard(&desk.id().await, note).await;

    let (_, dir) = deck.installed(url).await;
    assert_eq!(data(&dir), b"levels 1.1");
    assert_eq!(
        downloads_of(&server, "1.1").await,
        2,
        "the Deck used the source"
    );
}

/// The third criterion: turned off, every download goes to the source.
#[tokio::test(flavor = "multi_thread")]
async fn turned_off_every_download_goes_to_the_source() {
    let desk = core("Desk").await;
    let deck = core("Deck").await;
    pair(&desk, &deck).await;
    let (_a, _b) = (desk.sync.start(), deck.sync.start());
    let server = source().await;
    let url = format!("{}/fixture-runner-1.1.zip", server.uri());
    desk.installed(url.clone()).await;
    deck.hear_from(&desk).await;

    deck.peer_downloads(false).await;
    let (_, dir) = deck.installed(url).await;
    assert_eq!(data(&dir), b"levels 1.1");
    assert_eq!(
        downloads_of(&server, "1.1").await,
        2,
        "the Deck used the source"
    );
    // Nor does the Deck offer what it downloaded.
    assert!(deck.downloads.announce(&desk.id().await).await.is_none());
}

const PLATFORMS: &str = r#"
[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue"]
recommended = ["fakestation"]
bios = [{ file = "scph5501.bin", md5 = "6ff2f3ebc6981e5154f55e21ea8a5742", required = true }]
"#;

const EMULATOR: &str = r#"
id = "fakestation"
name = "FakeStation"

[detect.linux]
executable = "^fakestation$"

[vars]
bios = "{EmulatorDir}/bios"

[install.linux]
source = "github"
repo = "fake/station"
asset = '^fakestation-[\d.]+-linux\.zip$'

[[profile]]
name = "Default"
platforms = ["sony_playstation"]
extensions = ["cue"]
args = ["{RomPath}"]
"#;

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// fake/station's release v1.0 on a fake GitHub.
async fn emulator_release(server: &MockServer) {
    Mock::given(method("HEAD"))
        .and(path("/fake/station/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "/fake/station/releases/tag/v1.0"),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fake/station/releases/expanded_assets/v1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<ul><li><a href="/fake/station/releases/download/v1.0/fakestation-1.0-linux.zip" rel="nofollow">l</a></li></ul>"#,
        ))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/fake/station/releases/download/v1.0/fakestation-1.0-linux.zip",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "\"fakestation-1.0\"")
                .set_body_bytes(zip_of(&[
                    ("fakestation-1.0/fakestation", b"1.0"),
                    ("fakestation-1.0/resources/ui.dat", b"1.0"),
                ])),
        )
        .mount(server)
        .await;
}

/// One-click emulator setup on `core`, from the fake GitHub at `base`.
async fn emulator_setup(core: &Core, base: &str) -> caskade_core::emulation::EmulatorSetup {
    use caskade_core::emulation::{
        Catalog, EmulationService, EmulatorSetup, OperatingSystem, OsFolders, rom_entry,
    };
    let catalog = Catalog::load([
        ("platforms.toml", PLATFORMS),
        ("fakestation.toml", EMULATOR),
    ])
    .unwrap();
    let bus = Bus::default();
    let emulation = EmulationService::new(core.store.clone(), bus.clone(), catalog.clone())
        .with_host(
            OperatingSystem::Linux,
            OsFolders {
                user_home: Some(core.dir.path().join("home")),
                ..OsFolders::default()
            },
        );
    let roms = core.dir.path().join("roms");
    std::fs::create_dir_all(&roms).unwrap();
    std::fs::write(roms.join("Game.cue"), "").unwrap();
    let (game, entry) = rom_entry(
        "sony_playstation",
        &roms,
        &[roms.join("Game.cue")],
        Some(&catalog),
    );
    core.store.upsert_game(&game).await.unwrap();
    core.store.upsert_entry(&entry).await.unwrap();
    let hosts = caskade_core::managed::Hosts {
        github: caskade_core::updates::GitHub::new(url::Url::parse(&format!("{base}/")).unwrap()),
        dolphin: url::Url::parse(&format!("{base}/dolphin/")).unwrap(),
    };
    let setup = EmulatorSetup::new(
        core.store.clone(),
        bus,
        emulation,
        &AppPaths::under(&core.dir.path().join("setup")),
    )
    .with_hosts(hosts, core.dir.path().join("no-flatpak"));
    setup.set_nearby(core.downloads.clone());
    setup
}

/// RomM's firmware, for the Desk: the BIOS file it asks for.
struct Firmware;

impl caskade_core::emulation::BiosProvider for Firmware {
    fn fetch<'a>(
        &'a self,
        _platform: &'a str,
        files: &'a [caskade_core::emulation::Bios],
        dir: &'a std::path::Path,
    ) -> caskade_core::emulation::BiosFuture<'a> {
        Box::pin(async move {
            for file in files {
                std::fs::write(dir.join(&file.file), b"the bios").unwrap();
            }
            Ok(files.iter().map(|file| file.file.clone()).collect())
        })
    }
}

async fn installed_emulator(
    setup: &caskade_core::emulation::EmulatorSetup,
) -> caskade_core::emulation::SetupJob {
    let job = setup.install("fakestation").await.unwrap();
    for _ in 0..400 {
        let job = setup.job(&job.id).unwrap();
        if job.status.finished() {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the install didn't finish");
}

/// #277's first criterion: an emulator installed on the Desk installs on
/// the Deck with the internet unplugged, release and file both from the Desk.
#[tokio::test(flavor = "multi_thread")]
async fn an_emulator_installed_on_one_device_installs_on_another_without_the_internet() {
    let desk = core("Desk").await;
    let deck = core("Deck").await;
    pair(&desk, &deck).await;
    let (_a, _b) = (desk.sync.start(), deck.sync.start());
    let server = MockServer::start().await;
    emulator_release(&server).await;
    let base = server.uri();
    let desk_setup = emulator_setup(&desk, &base).await;
    desk_setup.set_bios(std::sync::Arc::new(Firmware));
    // The Deck has no RomM server.
    let deck_setup = emulator_setup(&deck, &base).await;

    let job = installed_emulator(&desk_setup).await;
    assert_eq!(
        job.status,
        caskade_core::emulation::SetupStatus::Done,
        "{:?}",
        job.error
    );

    // The internet goes: GitHub can't say which release is newest.
    drop(server);
    deck.hear_from(&desk).await;
    let job = installed_emulator(&deck_setup).await;
    assert_eq!(
        job.status,
        caskade_core::emulation::SetupStatus::Done,
        "{:?}",
        job.error
    );
    assert_eq!(job.version.as_deref(), Some("v1.0"));
    let dir = deck_setup.root().join("fakestation");
    assert_eq!(std::fs::read(dir.join("fakestation")).unwrap(), b"1.0");
    assert_eq!(job.downloaded, job.total.unwrap(), "progress while it came");
    // Its BIOS file came from the Desk too, checked against the MD5.
    assert_eq!(job.fetched_bios, ["scph5501.bin"]);
    assert_eq!(
        std::fs::read(dir.join("bios/scph5501.bin")).unwrap(),
        b"the bios"
    );
}

/// #277's second criterion: a Hasheous dump and the save manifest the Desk
/// downloaded reach a Deck that has never been online.
#[tokio::test(flavor = "multi_thread")]
async fn a_hasheous_dump_and_the_save_manifest_reach_a_device_never_online() {
    use caskade_core::hasheous::Hasheous;
    use caskade_core::ludusavi::{Ludusavi, Refresh};
    use md5::Digest;
    let desk = core("Desk").await;
    let deck = core("Deck").await;
    pair(&desk, &deck).await;
    let (_a, _b) = (desk.sync.start(), deck.sync.start());
    let server = MockServer::start().await;
    let game = serde_json::json!({
        "Id": 248876, "ObjectType": "Game", "Name": "Chirp",
        "Metadata": [{"Source": "IGDB", "Id": "1234", "Status": "Mapped", "Link": ""}],
        "Attributes": [{"attributeName": "ROMs", "attributeType": "EmbeddedList", "Value": [{
            "Name": "Chirp.hex", "Size": 23800, "Crc": "8EF5EB18",
            "Md5": "18af7be2a83f4027b47acce509785508",
            "Sha1": "3ecc30ea1be47361cf44ee4e9c55bc0542aa41af", "Sha256": ""
        }]}]
    })
    .to_string();
    let dump = zip_of(&[("Chirp (248876).json", game.as_bytes())]);
    let md5: String = md5::Md5::digest(&dump)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Mock::given(method("GET"))
        .and(path("/api/v1/Dumps/platforms"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "name": "Test Platform.zip", "sizeBytes": dump.len(),
                "lastModifiedUtc": "2026-10-23T12:32:24Z", "md5hash": md5,
            }])),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/Dumps/platforms/Test%20Platform.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(dump))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/manifest.yaml"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .set_body_string(include_str!("fixtures/ludusavi/manifest.yaml")),
        )
        .mount(&server)
        .await;
    let base = server.uri();
    let manifest_url = format!("{base}/manifest.yaml");
    let wanted = std::collections::BTreeSet::from(["Test Platform".to_owned()]);
    let services = |core: &Core| {
        let hasheous = Hasheous::with_base_url(core.dir.path().join("hasheous"), base.clone());
        hasheous.set_nearby(core.downloads.clone());
        let ludusavi = Ludusavi::with_url(core.dir.path().join("ludusavi"), manifest_url.clone());
        ludusavi.set_nearby(core.downloads.clone());
        (hasheous, ludusavi)
    };
    let (desk_hasheous, desk_ludusavi) = services(&desk);
    let (deck_hasheous, deck_ludusavi) = services(&deck);

    // The Desk is online.
    let report = desk_hasheous.sync(&wanted).await.unwrap();
    assert_eq!(report.downloaded, ["Test Platform"], "{report:?}");
    assert!(matches!(
        desk_ludusavi.refresh(true).await.unwrap(),
        Refresh::Updated { .. }
    ));

    // The Deck never was: everything comes from the Desk.
    drop(server);
    deck.hear_from(&desk).await;
    let report = deck_hasheous.sync(&wanted).await.unwrap();
    assert_eq!(report.downloaded, ["Test Platform"], "{report:?}");
    assert_eq!(report.indexed, ["Test Platform"], "{report:?}");
    let found = deck_hasheous.search("Chirp", &[], 5).await.unwrap();
    assert_eq!(found[0].name, "Chirp");
    assert!(matches!(
        deck_ludusavi.refresh(false).await.unwrap(),
        Refresh::Updated { .. }
    ));
    assert!(
        !deck_ludusavi
            .by_title("Example: Game")
            .await
            .unwrap()
            .is_empty()
    );
}
