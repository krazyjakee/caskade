//! Saves between two cores on loopback (#239): a save made on a Windows
//! device restores into a Linux device's Proton prefix and back, devices
//! that both played since they last agreed are asked which save to use and
//! keep both, and with Saves set to This device only versions stay put.
//! The Windows device is a fixture machine, so the Windows paths run as
//! path logic on any host.

use crate::cores::within;
use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{ConnectionPurpose, DeviceLink, Devices, MemoryKeyStore, Reach};
use caskade_core::ludusavi::Ludusavi;
use caskade_core::model::{
    ExternalId, Game, GameEntry, GameRuntime, InstallLocation, InstallState, SourceId, StorageRoot,
};
use caskade_core::paths::AppPaths;
use caskade_core::saves::Saves;
use caskade_core::saves::versions::{
    SaveBlobs, SaveCheck, SaveVersions, SyncRefresh, VersionReason,
};
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncService;
use caskade_core::sync::exchange::{self, Options};
use caskade_discovery::{HostEnv, Prefix, RunnerHint, RunnerKind};
use caskade_installer::manifest::RootManifest;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

fn write(path: impl AsRef<Path>, value: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value).unwrap();
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

struct Core {
    devices: Devices,
    settings: SettingsService,
    sync: SyncService,
    versions: SaveVersions,
    entry: String,
    /// Where the game keeps `<winDocuments>` and `<winAppData>` here.
    documents: PathBuf,
    app_data: PathBuf,
    _dir: tempfile::TempDir,
}

impl Core {
    async fn id(&self) -> String {
        self.devices.status().await.this_device.id.unwrap()
    }
}

async fn manifest_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(include_str!("fixtures/saves/manifest.yaml")),
        )
        .mount(&server)
        .await;
    server
}

/// A Windows machine, or a Linux one playing the game through Proton.
async fn core(name: &str, windows: bool, manifest: &MockServer) -> Core {
    let dir = tempfile::tempdir().unwrap();
    let machine = dir.path().join("machine");
    fs::create_dir_all(machine.join("root/home/player")).unwrap();
    write(
        machine.join("machine.toml"),
        if windows {
            "os = 'windows'\n[env]\nUSERPROFILE = 'C:\\Users\\player'\nUSERNAME = 'player'\n"
        } else {
            "os = 'linux'\n[env]\nHOME = '/home/player'\nUSER = 'player'\n"
        },
    );
    let env = HostEnv::fixture(&machine).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts,
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings.clone());
    devices.start();

    // The same Steam game on both, installed natively or through Proton.
    let game = store
        .upsert_game(&Game::new("Steam Fixture"))
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), "620");
    entry.ids = vec![ExternalId::Steam { app_id: 620 }];
    entry.state = InstallState::Installed;
    let (documents, app_data) = if windows {
        let base = env.map_windows_path("C:\\Games\\Fixture").unwrap();
        fs::create_dir_all(&base).unwrap();
        entry.location = Some(InstallLocation::External { path: base });
        let documents = env
            .known_folder(caskade_discovery::env::KnownFolder::Documents)
            .unwrap()
            .to_owned();
        let app_data = env
            .known_folder(caskade_discovery::env::KnownFolder::AppData)
            .unwrap()
            .to_owned();
        (documents, app_data)
    } else {
        let root = env.fs_root().to_owned();
        let base = root.join("steam/steamapps/common/FixtureGame");
        fs::create_dir_all(&base).unwrap();
        let prefix = root.join("steam/steamapps/compatdata/620/pfx");
        let user = prefix.join("drive_c/users/steamuser");
        fs::create_dir_all(user.join("Documents")).unwrap();
        fs::create_dir_all(user.join("AppData/Roaming")).unwrap();
        fs::create_dir_all(prefix.join("drive_c/ProgramData")).unwrap();
        write(
            prefix.join("user.reg"),
            "WINE REGISTRY Version 2\n#arch=win64\n",
        );
        write(
            prefix.join("system.reg"),
            "WINE REGISTRY Version 2\n#arch=win64\n",
        );
        entry.location = Some(InstallLocation::External { path: base });
        entry.runtime = GameRuntime::Windows {
            prefix: Some(Prefix {
                path: prefix,
                arch: None,
                owner: None,
            }),
            runner: None,
        };
        (user.join("Documents"), user.join("AppData/Roaming"))
    };
    let entry = store.upsert_entry(&entry).await.unwrap();

    let ludusavi = Ludusavi::with_url(dir.path().join("cache"), manifest.uri());
    ludusavi.refresh(true).await.unwrap();
    let saves = Saves::with_environment(store.clone(), ludusavi, env);
    let versions = SaveVersions::new(saves, dir.path().join("blobs"));
    versions.set_settings(settings.clone());
    // The same household account on both (accounts sync, #228).
    versions.set_account(|| Some("player".into()));
    let sync = SyncService::new(store.clone(), devices.clone());
    sync.set_settings(settings.clone());
    sync.register_blobs(Arc::new(SaveBlobs(versions.clone())));
    let engine_of = sync.clone();
    versions.set_device(move || engine_of.engine().map(|engine| engine.device().to_owned()));
    versions.set_wait(Duration::from_secs(15));

    let core = Core {
        devices,
        settings,
        sync,
        versions,
        entry: entry.id,
        documents,
        app_data,
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

/// One exchange from `a` to `b`, as the background sync runs it.
async fn exchange_now(a: &Core, b: &Core) {
    let engine = a.sync.engine().unwrap();
    let b_id = b.id().await;
    let mut session = a
        .devices
        .connect(&b_id, ConnectionPurpose::Sync)
        .await
        .unwrap();
    exchange::exchange(&engine, &b_id, &mut session, &Options::default())
        .await
        .unwrap();
    session.end().await;
    // The other side finishes keeping what it received.
    tokio::time::sleep(Duration::from_millis(300)).await;
}

async fn session(core: &Core) -> caskade_core::saves::versions::SaveVersion {
    core.versions
        .capture(&core.entry, None, VersionReason::Session)
        .await
        .unwrap()
        .expect("a version")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_windows_save_starts_the_game_on_a_proton_device_and_comes_back() {
    let manifest = manifest_server().await;
    let desktop = core("Desktop", true, &manifest).await;
    let deck = core("Deck", false, &manifest).await;
    pair(&desktop, &deck).await;
    desktop.versions.set_remote(SyncRefresh::new(
        desktop.sync.clone(),
        desktop.devices.clone(),
    ));
    deck.versions
        .set_remote(SyncRefresh::new(deck.sync.clone(), deck.devices.clone()));
    let (_a, _b) = (desktop.sync.start(), deck.sync.start());

    // Played on the Windows desktop, then quit.
    write(desktop.documents.join("Fixture/slot1.sav"), "from windows");
    write(
        desktop.app_data.join("Fixture/nested/progress.dat"),
        "level 3",
    );
    let played = session(&desktop).await;
    assert_eq!(played.file_count, 2);

    // Started on the Deck: the desktop's save is in the prefix first.
    let check = deck.versions.before_play(&deck.entry).await.unwrap();
    assert!(
        check.checked,
        "{check:?} {:?}",
        deck.devices.status().await.devices
    );
    assert_eq!(
        check.check,
        SaveCheck::Restored {
            version: deck
                .versions
                .store()
                .save_version(&played.id)
                .await
                .unwrap()
                .unwrap()
        }
    );
    assert_eq!(
        read(deck.documents.join("Fixture/slot1.sav")),
        "from windows"
    );
    assert_eq!(
        read(deck.app_data.join("Fixture/nested/progress.dat")),
        "level 3"
    );

    // Played on the Deck, quit; the desktop starts with the Deck's save.
    write(deck.documents.join("Fixture/slot1.sav"), "from the deck");
    let on_deck = session(&deck).await;
    assert_eq!(on_deck.parents, vec![played.id.clone()]);
    let check = desktop.versions.before_play(&desktop.entry).await.unwrap();
    assert!(
        matches!(&check.check, SaveCheck::Restored { version } if version.id == on_deck.id),
        "{check:?} {:?}",
        desktop.devices.status().await.devices
    );
    assert_eq!(
        read(desktop.documents.join("Fixture/slot1.sav")),
        "from the deck"
    );
    assert_eq!(
        read(desktop.app_data.join("Fixture/nested/progress.dat")),
        "level 3"
    );
    // Nothing new since: both up to date.
    assert_eq!(
        deck.versions.before_play(&deck.entry).await.unwrap().check,
        SaveCheck::UpToDate
    );
}

/// A copied game that never started here has no prefix yet: the save waits
/// in it for Proton or Wine to build the rest around (#14). The real thing is
/// in `proton_real`.
#[tokio::test(flavor = "multi_thread")]
async fn a_save_reaches_a_managed_game_whose_prefix_isnt_built_yet() {
    for (kind, user) in [(RunnerKind::Umu, "steamuser"), (RunnerKind::Wine, "player")] {
        let manifest = manifest_server().await;
        let desktop = core("Desktop", true, &manifest).await;
        let deck = core("Deck", false, &manifest).await;
        pair(&desktop, &deck).await;

        let store = deck.versions.store();
        let root = deck._dir.path().join("machine/root/Games");
        fs::create_dir_all(root.join("Fixture")).unwrap();
        let identity = RootManifest::new("deck-root").unwrap();
        identity.write(&root).unwrap();
        store
            .upsert_root(&StorageRoot::from_manifest(root.clone(), &identity))
            .await
            .unwrap();
        let mut entry = store.get_entry(&deck.entry).await.unwrap().unwrap();
        entry.location = Some(InstallLocation::Managed {
            root_id: identity.root_id().into(),
            rel_dir: "Fixture".into(),
        });
        entry.runtime = GameRuntime::Windows {
            prefix: Some(Prefix {
                path: ".caskade/prefix".into(),
                arch: None,
                owner: None,
            }),
            runner: Some(RunnerHint {
                kind,
                name: None,
                path: None,
            }),
        };
        store.upsert_entry(&entry).await.unwrap();

        write(desktop.documents.join("Fixture/slot1.sav"), "from windows");
        let played = session(&desktop).await;
        exchange_now(&desktop, &deck).await;
        within("the files arrive", 20, || async {
            deck.versions
                .store()
                .save_version(&played.id)
                .await
                .unwrap()
                .is_some_and(|version| deck.versions.complete(&version))
        })
        .await;

        let check = deck.versions.before_play(&deck.entry).await.unwrap();
        assert!(
            matches!(&check.check, SaveCheck::Restored { version } if version.id == played.id),
            "{check:?}"
        );
        assert!(check.warnings.is_empty(), "{check:?}");
        let home = root.join("Fixture/.caskade/prefix/drive_c/users");
        assert_eq!(
            read(home.join(user).join("Documents/Fixture/slot1.sav")),
            "from windows"
        );
        // Not a whole prefix: wineboot still has to run.
        assert!(!root.join("Fixture/.caskade/prefix/dosdevices").exists());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn both_played_offline_asks_which_save_and_keeps_both() {
    let manifest = manifest_server().await;
    let desktop = core("Desktop", true, &manifest).await;
    let deck = core("Deck", false, &manifest).await;
    pair(&desktop, &deck).await;

    // They agree on a first save.
    write(desktop.documents.join("Fixture/slot1.sav"), "base");
    let base = session(&desktop).await;
    exchange_now(&desktop, &deck).await;
    assert!(matches!(
        deck.versions.before_play(&deck.entry).await.unwrap().check,
        SaveCheck::Restored { .. }
    ));

    // Apart, both play.
    write(desktop.documents.join("Fixture/slot1.sav"), "desktop run");
    let on_desktop = session(&desktop).await;
    write(deck.documents.join("Fixture/slot1.sav"), "deck run");
    let on_deck = session(&deck).await;
    assert_eq!(on_desktop.parents, vec![base.id.clone()]);
    assert_eq!(on_deck.parents, vec![base.id.clone()]);
    exchange_now(&desktop, &deck).await;

    // Each asks, restores neither, and has both.
    for (core, mine) in [(&desktop, "desktop run"), (&deck, "deck run")] {
        let check = core.versions.before_play(&core.entry).await.unwrap();
        let SaveCheck::Conflict { versions } = check.check else {
            panic!("expected a question, got {check:?}");
        };
        let mut ids: Vec<_> = versions.iter().map(|v| v.id.clone()).collect();
        ids.sort();
        let mut expected = vec![on_desktop.id.clone(), on_deck.id.clone()];
        expected.sort();
        assert_eq!(ids, expected);
        assert_eq!(read(core.documents.join("Fixture/slot1.sav")), mine);
    }

    // The deck picks the desktop's run; the choice settles it everywhere.
    let chosen = deck
        .versions
        .choose(&deck.entry, &on_desktop.id)
        .await
        .unwrap();
    assert_eq!(
        read(deck.documents.join("Fixture/slot1.sav")),
        "desktop run"
    );
    exchange_now(&deck, &desktop).await;
    assert_eq!(
        desktop
            .versions
            .before_play(&desktop.entry)
            .await
            .unwrap()
            .check,
        SaveCheck::UpToDate
    );
    assert_eq!(
        deck.versions.before_play(&deck.entry).await.unwrap().check,
        SaveCheck::UpToDate
    );
    // Both runs remain as versions on both.
    for core in [&desktop, &deck] {
        for id in [&on_desktop.id, &on_deck.id, &chosen.id] {
            assert!(
                core.versions
                    .store()
                    .save_version(id)
                    .await
                    .unwrap()
                    .is_some(),
                "{id}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn with_saves_on_this_device_only_versions_stay_put() {
    let manifest = manifest_server().await;
    let desktop = core("Desktop", true, &manifest).await;
    let deck = core("Deck", false, &manifest).await;
    pair(&desktop, &deck).await;
    deck.settings
        .set(
            "core.sync",
            json!({ "categories": { "saves": "thisDeviceOnly" }, "neverRelay": false, "port": null }),
            None,
        )
        .await
        .unwrap();

    write(deck.documents.join("Fixture/slot1.sav"), "deck only");
    let on_deck = session(&deck).await;
    write(desktop.documents.join("Fixture/slot1.sav"), "desktop");
    let on_desktop = session(&desktop).await;
    exchange_now(&desktop, &deck).await;
    exchange_now(&deck, &desktop).await;

    // Neither went across, records or files.
    assert!(
        desktop
            .versions
            .store()
            .save_version(&on_deck.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!desktop.versions.blobs().has(&on_deck.content_hash));
    assert!(
        deck.versions
            .store()
            .save_version(&on_desktop.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!deck.versions.blobs().has(&on_desktop.content_hash));
    assert_eq!(
        deck.versions.before_play(&deck.entry).await.unwrap().check,
        SaveCheck::UpToDate
    );
    assert_eq!(read(deck.documents.join("Fixture/slot1.sav")), "deck only");

    // Back on: they travel.
    deck.settings
        .set(
            "core.sync",
            json!({ "categories": { "saves": "sync" }, "neverRelay": false, "port": null }),
            None,
        )
        .await
        .unwrap();
    deck.sync.engine().unwrap().apply_pending().await.unwrap();
    caskade_core::sync::exchange::replay(
        &deck.sync.engine().unwrap(),
        &[caskade_core::sync::policy::Category::Saves],
    )
    .await
    .unwrap();
    exchange_now(&deck, &desktop).await;
    within("the deck's version reaches the desktop", 10, || async {
        desktop
            .versions
            .store()
            .save_version(&on_deck.id)
            .await
            .unwrap()
            .is_some()
            && desktop.versions.complete(&on_deck)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_out_of_reach_never_holds_up_a_launch_for_long_and_is_reported() {
    let manifest = manifest_server().await;
    let desktop = core("Desktop", true, &manifest).await;
    let deck = core("Deck", false, &manifest).await;
    pair(&desktop, &deck).await;
    desktop.versions.set_remote(SyncRefresh::new(
        desktop.sync.clone(),
        desktop.devices.clone(),
    ));
    desktop.versions.set_wait(Duration::from_secs(3));
    let _sync = desktop.sync.start();
    write(desktop.documents.join("Fixture/slot1.sav"), "here");
    session(&desktop).await;
    // The deck is switched off.
    deck.devices.shutdown().await;

    let started = Instant::now();
    let check = desktop.versions.before_play(&desktop.entry).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(!check.checked, "{check:?}");
    assert_eq!(check.check, SaveCheck::UpToDate);
    assert_eq!(read(desktop.documents.join("Fixture/slot1.sav")), "here");

    // Once it's known to be out of reach (offline, and the last try
    // failed), a launch doesn't wait for it at all, and still says it
    // wasn't reached.
    within("the deck reads offline", 30, || async {
        desktop
            .devices
            .status()
            .await
            .devices
            .iter()
            .all(|device| device.link == DeviceLink::Offline && device.sync_error.is_some())
    })
    .await;
    let started = Instant::now();
    let check = desktop.versions.before_play(&desktop.entry).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert!(!check.checked, "{check:?}");
    assert_eq!(check.check, SaveCheck::UpToDate);
}
