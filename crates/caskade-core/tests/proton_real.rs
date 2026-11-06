//! What the loopback tests fake, on real Proton (#11, #14): a Windows game
//! copied to a Linux device starts there through umu-launcher and Proton,
//! and its saves go from a Windows device into the prefix and back.
//!
//! `scripts/proton-real.sh` runs these: a scratch home with its own copies of
//! umu, the Steam Runtime and Proton, an Xvfb of its own, no internet.
//!
//! What's real: the program (a Windows PE, `fixtures/saves/proof_game.rs`),
//! umu, pressure-vessel, the Steam Runtime, Proton, the prefix it builds, the
//! Caskade core on both sides, the copy, sync and launch pipelines and the
//! Windows folders the program asks Windows for. What isn't: the "Windows"
//! device is a Linux process holding a fixture Windows machine, so its
//! `%APPDATA%` is a folder standing in for one (`windows_saves` has the real
//! one, on a Windows runner); the two cores share one process and talk over
//! loopback.

use std::path::{Path, PathBuf};
use std::time::Duration;

use caskade_core::install::{JobStatus, PeerCopyTarget};
use caskade_core::model::{GameRuntime, InstallLocation};
use caskade_core::saves::versions::SaveCheck;
use caskade_discovery::{HostEnv, env::KnownFolder};
use wiremock::MockServer;

use crate::cores::{core, pair};
use crate::game_copy::finished;
use crate::proof_game::{
    self, Device, TITLE, add_game, device, last_run, play, read, version_after, write,
};

/// Everything Proton touches stays under the script's scratch home.
fn in_scratch() {
    let scratch =
        std::env::var_os("CASKADE_PROTON_SCRATCH").expect("run by scripts/proton-real.sh");
    let home = std::env::var_os("HOME").expect("a home");
    assert!(Path::new(&home).starts_with(&scratch), "not a scratch home");
}

/// A Windows machine, as a fixture: Windows paths in a folder of this test's.
fn windows_machine(dir: &Path) -> HostEnv {
    let machine = dir.join("machine");
    std::fs::create_dir_all(machine.join("root/home/player")).unwrap();
    std::fs::write(
        machine.join("machine.toml"),
        "os = 'windows'\n[env]\nUSERPROFILE = 'C:\\Users\\player'\nUSERNAME = 'player'\n",
    )
    .unwrap();
    HostEnv::fixture(&machine).unwrap()
}

/// The Desk, a Windows device, and the TV, this Linux PC with real Proton.
async fn two_devices(manifest: &MockServer) -> (Device, HostEnv, Device) {
    let desk = core("Desk").await;
    let machine = windows_machine(desk.dir.path());
    let desk = device(desk, machine.clone(), manifest).await;
    let tv = core("TV").await;
    // The real host: umu and Proton as this machine has them, not a profile.
    let host = caskade_installer::pipeline::input::host(None).unwrap();
    assert!(
        host.has_runner(caskade_installer::host::RunnerKind::Proton),
        "no umu-run in the scratch home"
    );
    let _ = tv.install.clone().with_host(host);
    let tv = device(tv, HostEnv::live(), manifest).await;
    pair(&desk.core, &tv.core).await;
    (desk, machine, tv)
}

/// Copy to this computer on the TV, as the Desk's game arrived by sync.
/// Returns the game here, its release and its folder.
async fn copy_to_tv(desk: &Device, tv: &Device, desk_entry: &str) -> (String, String, PathBuf) {
    let tv_core = &tv.core;
    let mut game = None;
    let found = &mut game;
    let store = tv_core.store.clone();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while found.is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the game reached the TV's library"
        );
        for id in store.sync_snapshot().await.unwrap().games {
            if store.game(&id).await.unwrap().unwrap().title == TITLE {
                *found = Some(id);
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let game = game.unwrap();
    let job = tv_core
        .install
        .copy_from_device(
            PeerCopyTarget {
                device_id: desk.core.id().await,
                device_name: "Desk".into(),
                entry_id: desk_entry.to_owned(),
                game_id: game.clone(),
                title: TITLE.into(),
            },
            None,
        )
        .await
        .unwrap();
    let done = finished(&tv_core.install, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    let entries = tv_core.store.entries_for_game(&game).await.unwrap();
    let copy = entries
        .iter()
        .find(|entry| entry.source_id.as_str() == "caskade-storage")
        .expect("a managed release");
    assert!(
        matches!(copy.runtime, GameRuntime::Windows { .. }),
        "{:?}",
        copy.runtime
    );
    let Some(InstallLocation::Managed { rel_dir, .. }) = &copy.location else {
        panic!("not managed: {:?}", copy.location)
    };
    let folder = tv_core.games.join(rel_dir);
    eprintln!("copied to {}", folder.display());
    (game, copy.id.clone(), folder)
}

/// Every file under `dir`, relative, without following links.
fn tree(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(&path, base, found);
            } else {
                found.push(path.strip_prefix(base).unwrap().display().to_string());
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, dir, &mut found);
    found.sort();
    found
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "real Proton: run by scripts/proton-real.sh"]
async fn a_windows_game_copied_here_starts_through_proton() {
    in_scratch();
    let manifest = proof_game::manifest("CaskadeProof").await;
    let (desk, _, tv) = two_devices(&manifest).await;
    let (desk_entry, source) = add_game(&desk, "CaskadeProof").await;
    let (_a, _b) = (desk.sync.start(), tv.sync.start());
    let (game, entry, folder) = copy_to_tv(&desk, &tv, &desk_entry).await;
    assert_eq!(
        std::fs::read(folder.join("Content/level1.pak")).unwrap(),
        std::fs::read(source.join("Content/level1.pak")).unwrap()
    );
    assert!(!folder.join("ran.log").exists(), "never run yet");
    assert!(!folder.join(".caskade/prefix").exists(), "no prefix yet");

    let started = std::time::Instant::now();
    let session = play(&tv, &entry).await;
    eprintln!(
        "first play through Proton took {:?}, session {} ms\n{}",
        started.elapsed(),
        session.duration_ms,
        last_run(&folder)
    );

    // The program ran, on Wine, from Caskade's drive, in the game's prefix.
    let run = last_run(&folder);
    assert!(run.starts_with("run wine="), "{run}");
    assert!(!run.contains("wine=none"), "not under Wine: {run}");
    assert!(run.contains(r"exe=G:\Game\Caskade Proof.exe"), "{run}");
    assert!(run.contains(r"cwd=G:\Game"), "{run}");
    assert!(run.contains("data=6291456"), "{run}");
    assert!(
        run.contains(r"appdata=C:\users\steamuser\AppData\Roaming\CaskadeProof"),
        "{run}"
    );
    // And this is Proton's prefix, isolated as a managed game's is.
    let prefix = folder.join(".caskade/prefix");
    assert!(read(prefix.join("version")).contains("UMU-Proton"));
    assert_eq!(
        read(prefix.join("drive_c/users/steamuser/AppData/Roaming/CaskadeProof/progress.txt")),
        "level=1 by=proton"
    );
    assert_eq!(
        std::fs::read_dir(prefix.join("dosdevices/z:"))
            .unwrap()
            .count(),
        0
    );
    let sessions = tv.core.store.sessions_for_game(&game, 10).await.unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert!(sessions[0].ended_at_ms.is_some());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "real Proton: run by scripts/proton-real.sh"]
async fn a_windows_save_reaches_the_prefix_and_comes_back() {
    in_scratch();
    let manifest = proof_game::manifest("CaskadeProof").await;
    let (desk, machine, tv) = two_devices(&manifest).await;
    let (desk_entry, _) = add_game(&desk, "CaskadeProof").await;
    let (_a, _b) = (desk.sync.start(), tv.sync.start());
    let (game, entry, folder) = copy_to_tv(&desk, &tv, &desk_entry).await;

    // Played on Windows: saves where Windows keeps them, kept as a version.
    let app_data = machine
        .known_folder(KnownFolder::AppData)
        .unwrap()
        .join("CaskadeProof");
    let documents = machine
        .known_folder(KnownFolder::Documents)
        .unwrap()
        .join("CaskadeProof");
    let local = machine
        .known_folder(KnownFolder::LocalAppData)
        .unwrap()
        .join("CaskadeProof");
    let saved_games = machine.home().join("Saved Games/CaskadeProof");
    write(app_data.join("progress.txt"), "level=7 by=windows");
    write(documents.join("slot1.sav"), "slot level=7 by=windows");
    write(local.join("settings.ini"), "[game]\nby=windows\n");
    write(saved_games.join("trophies.dat"), "trophies=7 by=windows");
    let played = desk
        .versions
        .capture(
            &desk_entry,
            None,
            caskade_core::saves::versions::VersionReason::Session,
        )
        .await
        .unwrap()
        .expect("a version on the Desk");
    assert_eq!(played.file_count, 4);

    // Play on the TV: its prefix doesn't exist yet, and the game starts with
    // the Desk's save all the same.
    let check = tv.versions.before_play(&entry).await.unwrap();
    assert!(check.checked, "{check:?}");
    assert!(
        matches!(&check.check, SaveCheck::Restored { version } if version.id == played.id),
        "{check:?}"
    );
    assert!(check.warnings.is_empty(), "{check:?}");
    let session = play(&tv, &entry).await;
    eprintln!("first play on the TV:\n{}", last_run(&folder));
    let run = last_run(&folder);
    assert!(!run.contains("wine=none"), "{run}");
    assert!(run.contains("progress=[level=7 by=windows]"), "{run}");
    assert!(run.contains("slot=[slot level=7 by=windows]"), "{run}");
    assert!(run.contains("settings=[[game]\nby=windows\n]"), "{run}");
    assert!(run.contains("trophies=[trophies=7 by=windows]"), "{run}");
    assert!(run.contains("wrote level=8 by=proton"), "{run}");

    // Caskade kept what the game wrote in the prefix, one file each, named
    // by the folders Windows has.
    let from_proton = version_after(&tv, &game, &session).await;
    assert_eq!(from_proton.parents, vec![played.id.clone()]);
    assert_eq!(from_proton.file_count, 4, "{from_proton:?}");

    // Back on the Desk: Windows paths, and nothing else.
    let before = tree(machine.home());
    let check = desk.versions.before_play(&desk_entry).await.unwrap();
    assert!(
        matches!(&check.check, SaveCheck::Restored { version } if version.id == from_proton.id),
        "{check:?}"
    );
    assert!(check.warnings.is_empty(), "{check:?}");
    assert_eq!(read(app_data.join("progress.txt")), "level=8 by=proton");
    assert_eq!(read(documents.join("slot1.sav")), "slot level=8 by=proton");
    assert_eq!(read(local.join("settings.ini")), "[game]\nby=proton\n");
    assert_eq!(
        read(saved_games.join("trophies.dat")),
        "trophies=8 by=proton"
    );
    assert_eq!(tree(machine.home()), before, "files beside the saves");

    // Windows again, then Proton reads that in the prefix it has now.
    write(app_data.join("progress.txt"), "level=20 by=windows");
    let again = desk
        .versions
        .capture(
            &desk_entry,
            None,
            caskade_core::saves::versions::VersionReason::Session,
        )
        .await
        .unwrap()
        .expect("a version on the Desk");
    let check = tv.versions.before_play(&entry).await.unwrap();
    assert!(
        matches!(&check.check, SaveCheck::Restored { version } if version.id == again.id),
        "{check:?}"
    );
    play(&tv, &entry).await;
    eprintln!("second play on the TV:\n{}", last_run(&folder));
    let run = last_run(&folder);
    assert!(run.contains("progress=[level=20 by=windows]"), "{run}");
    assert!(run.contains("wrote level=21 by=proton"), "{run}");
}
