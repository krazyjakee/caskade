//! Copying an installed game between two cores on loopback (#233): a
//! Windows game added by hand on the Desk arrives on the TV as a
//! Caskade-managed install of the same game, with a `caskade-install` id of
//! its own, its program relative to its folder and a Windows runtime (Proton
//! on Linux); a launcher's install is refused; the sender can't move or
//! uninstall it while sending.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::install::{
    InstallOptions, InstallService, JobSnapshot, JobStatus, PeerCopyTarget,
};
use caskade_core::model::{
    Game, GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec, StorageRoot,
};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::transfer::Transfers;
use caskade_installer::manifest::RootManifest;
use caskade_installer::pipeline::input;

struct Core {
    store: LibraryStore,
    devices: Devices,
    transfers: Transfers,
    install: InstallService,
    games: PathBuf,
    dir: tempfile::TempDir,
}

/// Waits up to `seconds` for `check`.
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
    let transfers = Transfers::new(store.clone(), devices.clone(), &dir.path().join("cache"))
        .await
        .unwrap();
    devices.start();
    // A storage root for what arrives.
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
    install.set_peer_fetcher(Arc::new(transfers.clone()));
    let core = Core {
        store,
        devices,
        transfers,
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
    let b_id = b.devices.status().await.this_device.id.unwrap();
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

fn fixture_exe() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../caskade-installer/tests/fixtures/bin/fixture-game-windows-x86_64.exe")
}

/// Celeste, a Windows game added by hand on `core` from a folder of its own.
async fn add_celeste(core: &Core) -> (String, String, PathBuf) {
    let folder = core.dir.path().join("My Games").join("Celeste");
    std::fs::create_dir_all(folder.join("Content/Levels")).unwrap();
    std::fs::copy(fixture_exe(), folder.join("Celeste.exe")).unwrap();
    std::fs::write(folder.join("Content/Levels/1.bin"), vec![3u8; 50_000]).unwrap();
    let big: Vec<u8> = (0..6 * 1024 * 1024).map(|i| (i % 253) as u8).collect();
    std::fs::write(folder.join("Content/Celeste.pak"), big).unwrap();
    let game = core.store.upsert_game(&Game::new("Celeste")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, "manual".to_owned().into(), "celeste");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: folder.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: folder.join("Celeste.exe"),
        args: Vec::new(),
        cwd: None,
        env: Default::default(),
    });
    entry.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    let entry = core.store.upsert_entry(&entry).await.unwrap();
    (game.id, entry.id, folder)
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

async fn id(core: &Core) -> String {
    core.devices.status().await.this_device.id.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_windows_game_added_by_hand_arrives_as_a_managed_install_of_the_same_game() {
    let desk = core("Desk").await;
    let tv = core("TV").await;
    pair(&desk, &tv).await;
    let (_, desk_entry, folder) = add_celeste(&desk).await;
    let tv_game = tv.store.upsert_game(&Game::new("Celeste")).await.unwrap();

    let started = Instant::now();
    let job = tv
        .install
        .copy_from_device(
            PeerCopyTarget {
                device_id: id(&desk).await,
                device_name: "Desk".into(),
                entry_id: desk_entry.clone(),
                game_id: tv_game.id.clone(),
                title: "Celeste".into(),
            },
            None,
        )
        .await
        .unwrap();
    let done = finished(&tv.install, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    eprintln!("copied and installed in {:?}", started.elapsed());

    // The same game, a managed install with its own install id.
    assert_eq!(done.game_id.as_deref(), Some(tv_game.id.as_str()));
    let entries = tv.store.entries_for_game(&tv_game.id).await.unwrap();
    let copy = entries
        .iter()
        .find(|entry| entry.source_id.as_str() == "caskade-storage")
        .expect("a managed release");
    assert!(uuid::Uuid::parse_str(&copy.external_id).is_ok());
    assert_ne!(copy.external_id, desk_entry);
    let Some(InstallLocation::Managed { rel_dir, .. }) = &copy.location else {
        panic!("not managed: {:?}", copy.location)
    };
    // A Windows build: it runs here through Proton.
    assert!(matches!(copy.runtime, GameRuntime::Windows { .. }));
    let Some(LaunchSpec::Executable { path, .. }) = &copy.launch else {
        panic!("no program: {:?}", copy.launch)
    };
    assert!(path.is_relative(), "{path:?}");
    assert!(path.ends_with("Celeste.exe"), "{path:?}");
    // Byte for byte, and nothing left in the staging folder.
    let installed = tv.games.join(rel_dir);
    for file in ["Celeste.exe", "Content/Levels/1.bin", "Content/Celeste.pak"] {
        assert_eq!(
            std::fs::read(installed.join(file)).unwrap(),
            std::fs::read(folder.join(file)).unwrap(),
            "{file}"
        );
    }
    let staging = tv.dir.path().join("cache/copies/receive/incoming");
    assert!(
        !staging.exists() || std::fs::read_dir(&staging).unwrap().next().is_none(),
        "staging left behind"
    );
    // The Desk is done sending it.
    within("the Desk stops sending", 10, || async {
        !caskade_core::install::PeerFetcher::sending(&desk.transfers, &desk_entry)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_launcher_s_install_is_refused_and_says_why() {
    let desk = core("Desk").await;
    let tv = core("TV").await;
    pair(&desk, &tv).await;
    let game = desk
        .store
        .upsert_game(&Game::new("Portal 2"))
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), "620");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: desk.dir.path().join("steamapps/common/Portal 2"),
    });
    entry.launch = Some(LaunchSpec::Uri {
        uri: "steam://rungameid/620".into(),
    });
    let entry = desk.store.upsert_entry(&entry).await.unwrap();
    let tv_game = tv.store.upsert_game(&Game::new("Portal 2")).await.unwrap();
    let job = tv
        .install
        .copy_from_device(
            PeerCopyTarget {
                device_id: id(&desk).await,
                device_name: "Desk".into(),
                entry_id: entry.id,
                game_id: tv_game.id,
                title: "Portal 2".into(),
            },
            None,
        )
        .await
        .unwrap();
    let done = finished(&tv.install, &job.id).await;
    assert_eq!(done.status, JobStatus::Failed);
    assert!(
        done.error.unwrap_or_default().contains("launcher"),
        "the reason names the launcher"
    );
}
