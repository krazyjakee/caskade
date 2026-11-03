//! Copying an installed game between two cores on loopback (#233): a
//! Windows game added by hand on the Desk arrives on the TV as a
//! Caskade-managed install of the same game, with a `caskade-install` id of
//! its own, its program relative to its folder and a Windows runtime (Proton
//! on Linux); a launcher's install is refused; the sender can't move or
//! uninstall it while sending. Sent from the Desk (#235), it waits on the TV
//! until confirmed there.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use caskade_core::install::{InstallService, JobSnapshot, JobStatus, PeerCopyTarget};
use caskade_core::model::{
    Game, GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec,
};

use crate::cores::{Core, core, pair, within};

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

/// #279: an emulated game whose folder is a ROM folder holding other games
/// arrives as its own ROM file alone, placed as a release of the same game.
#[tokio::test(flavor = "multi_thread")]
async fn an_emulated_game_arrives_as_its_rom_alone_not_its_folder() {
    let desk = core("Desk").await;
    let tv = core("TV").await;
    pair(&desk, &tv).await;
    let roms = desk.dir.path().join("ROMs/gba");
    std::fs::create_dir_all(&roms).unwrap();
    std::fs::write(roms.join("Golden Sun (Europe).gba"), vec![7u8; 40_000]).unwrap();
    std::fs::write(roms.join("Metroid Fusion (USA).gba"), vec![9u8; 30_000]).unwrap();
    let (game, mut entry) = caskade_core::emulation::rom_entry(
        "gba",
        &roms,
        &[PathBuf::from("Golden Sun (Europe).gba")],
        None,
    );
    let game = desk.store.upsert_game(&game).await.unwrap();
    entry.game_id = game.id.clone();
    let entry = desk.store.upsert_entry(&entry).await.unwrap();
    let tv_game = tv
        .store
        .upsert_game(&Game::new("Golden Sun"))
        .await
        .unwrap();

    let job = tv
        .install
        .copy_from_device(
            PeerCopyTarget {
                device_id: id(&desk).await,
                device_name: "Desk".into(),
                entry_id: entry.id.clone(),
                game_id: tv_game.id.clone(),
                title: "Golden Sun".into(),
            },
            None,
        )
        .await
        .unwrap();
    let done = finished(&tv.install, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert_eq!(done.game_id.as_deref(), Some(tv_game.id.as_str()));
    let copy = tv
        .store
        .entries_for_game(&tv_game.id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| matches!(entry.launch, Some(LaunchSpec::Emulator { .. })))
        .expect("an emulated release of the same game");
    let Some(LaunchSpec::Emulator {
        platform_id, roms, ..
    }) = &copy.launch
    else {
        unreachable!()
    };
    assert_eq!(platform_id, "gba");
    assert_eq!(roms.len(), 1);
    // Only its own ROM came, into the TV's storage.
    let mut arrived = Vec::new();
    for entry in walk(&tv.games) {
        arrived.push(entry.file_name().unwrap().to_string_lossy().into_owned());
    }
    assert!(
        arrived.contains(&"Golden Sun (Europe).gba".to_owned()),
        "{arrived:?}"
    );
    assert!(
        !arrived.iter().any(|name| name.contains("Metroid")),
        "{arrived:?}"
    );
}

/// Every file under `dir`.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
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

#[tokio::test(flavor = "multi_thread")]
async fn a_game_sent_from_the_desk_waits_on_the_tv_until_confirmed() {
    let desk = core("Desk").await;
    let tv = core("TV").await;
    pair(&desk, &tv).await;
    let (_, desk_entry, folder) = add_celeste(&desk).await;
    let (sent, mut requests) = tokio::sync::mpsc::unbounded_channel();
    tv.transfers.on_send(move |request| {
        let _ = sent.send(request);
    });

    desk.transfers
        .send_to(&id(&tv).await, &desk_entry)
        .await
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(10), requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(request.device_id, id(&desk).await);
    assert_eq!(request.entry_id, desk_entry);
    assert_eq!(request.title, "Celeste");
    // Nothing is written on the TV until it's confirmed there.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(tv.install.jobs().is_empty());
    let incoming = tv.dir.path().join("cache/copies/receive/incoming");
    assert!(!incoming.exists() || std::fs::read_dir(&incoming).unwrap().next().is_none());
    assert_eq!(
        std::fs::read_dir(&tv.games)
            .unwrap()
            .filter(|entry| !entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.'))
            .count(),
        0
    );

    // Confirmed: copied from the Desk.
    let job = tv
        .install
        .copy_from_device(
            PeerCopyTarget {
                device_id: request.device_id,
                device_name: "Desk".into(),
                entry_id: request.entry_id,
                game_id: request.game_id.unwrap_or_default(),
                title: request.title,
            },
            None,
        )
        .await
        .unwrap();
    let done = finished(&tv.install, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    let game = done.game_id.unwrap();
    let entries = tv.store.entries_for_game(&game).await.unwrap();
    let Some(InstallLocation::Managed { rel_dir, .. }) = &entries[0].location else {
        panic!("not managed")
    };
    assert_eq!(
        std::fs::read(tv.games.join(rel_dir).join("Celeste.exe")).unwrap(),
        std::fs::read(folder.join("Celeste.exe")).unwrap()
    );
}
