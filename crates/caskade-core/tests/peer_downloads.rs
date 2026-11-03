//! Downloads from a device on the local network before the internet
//! (#236), between two cores on loopback with a wiremock source: an update
//! the Desk downloaded installs on the Deck with the source gone; a file the
//! Desk sends that fails its checks is thrown away and the source used; and
//! with `core.sync.peerDownloads` off every download goes to the source.

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
    _dir: tempfile::TempDir,
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
        _dir: dir,
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
