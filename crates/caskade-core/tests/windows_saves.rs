//! The Windows half of a save going to another device and back (#14), on a
//! real Windows runner: the real program (`fixtures/saves/proof_game.rs`) started
//! through the launch pipeline asks Windows for its folders, and Caskade
//! keeps its saves from the real `%APPDATA%`, `%LOCALAPPDATA%`, Documents and
//! Saved Games, then restores another device's into them. The other device
//! is a fixture Windows machine: a version names its files by those folders,
//! whichever device made it, and `proton_real` has a Proton prefix doing the
//! same (a fixture Linux machine can't run on Windows, whose paths have
//! drive letters). The ci.yml job `windows-elevation` runs this.
//!
//! The save folders are named for the run, and go when it ends.

use std::path::PathBuf;
use std::time::Duration;

use caskade_core::model::{
    Game, GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec,
};
use caskade_core::saves::versions::{SaveCheck, VersionReason};
use caskade_discovery::{HostEnv, env::KnownFolder};

use crate::cores::{core, pair};
use crate::proof_game::{
    self, TITLE, add_game, device, last_run, play, read, version_after, write,
};

/// A folder under each of the real Windows folders, gone at the end.
struct RealFolders(Vec<PathBuf>);

impl Drop for RealFolders {
    fn drop(&mut self) {
        for folder in &self.0 {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

fn env_dir(name: &str) -> PathBuf {
    std::env::var_os(name).expect(name).into()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "writes in the real %APPDATA%: CI's Windows runner"]
async fn a_save_in_the_real_windows_folders_leaves_and_comes_back() {
    let name = format!("CaskadeProof-{}", uuid::Uuid::new_v4().simple());
    let profile = env_dir("USERPROFILE");
    let app_data = env_dir("APPDATA").join(&name);
    let local = env_dir("LOCALAPPDATA").join(&name);
    let documents = profile.join("Documents").join(&name);
    let saved_games = profile.join("Saved Games").join(&name);
    let _gone = RealFolders(vec![
        app_data.clone(),
        local.clone(),
        documents.clone(),
        saved_games.clone(),
    ]);

    let manifest = proof_game::manifest(&name).await;
    let desk = device(core("Desk").await, HostEnv::live(), &manifest).await;
    let deck_core = core("Deck").await;

    // The other device, and its copy of the game.
    let machine = deck_core.dir.path().join("machine");
    std::fs::create_dir_all(machine.join("root/home/player")).unwrap();
    write(
        machine.join("machine.toml"),
        "os = 'windows'\n[env]\nUSERPROFILE = 'C:\\Users\\player'\nUSERNAME = 'player'\n",
    );
    let env = HostEnv::fixture(&machine).unwrap();
    let base = env.map_windows_path("C:\\Games\\Proof").unwrap();
    std::fs::create_dir_all(&base).unwrap();
    let other_app_data = env.known_folder(KnownFolder::AppData).unwrap().join(&name);
    let other_local = env
        .known_folder(KnownFolder::LocalAppData)
        .unwrap()
        .join(&name);
    let other_documents = env
        .known_folder(KnownFolder::Documents)
        .unwrap()
        .join(&name);
    let other_saved_games = env.home().join("Saved Games").join(&name);
    let game = deck_core
        .store
        .upsert_game(&Game::new(TITLE))
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, "manual".to_owned().into(), "proof");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External { path: base });
    entry.launch = Some(LaunchSpec::Executable {
        path: "Caskade Proof.exe".into(),
        args: Vec::new(),
        cwd: None,
        env: Default::default(),
    });
    entry.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    let deck_entry = deck_core.store.upsert_entry(&entry).await.unwrap().id;
    let deck = device(deck_core, env, &manifest).await;
    pair(&desk.core, &deck.core).await;

    // Played on real Windows: the program itself, through the launch
    // pipeline, and the version `launch.after` keeps.
    let (entry, folder) = add_game(&desk, &name).await;
    let game = desk
        .core
        .store
        .get_entry(&entry)
        .await
        .unwrap()
        .unwrap()
        .game_id;
    let session = play(&desk, &entry).await;
    let run = last_run(&folder);
    eprintln!("first play on Windows:\n{run}");
    // `CASKADE_PROOF_WINE` rehearses this binary under Wine, away from CI.
    let by = match std::env::var_os("CASKADE_PROOF_WINE") {
        Some(_) => "proton",
        None => "windows",
    };
    assert_eq!(run.contains("wine=none"), by == "windows", "{run}");
    assert_eq!(
        read(app_data.join("progress.txt")),
        format!("level=1 by={by}")
    );
    assert_eq!(
        read(documents.join("slot1.sav")),
        format!("slot level=1 by={by}")
    );
    assert_eq!(
        read(local.join("settings.ini")),
        format!("[game]\nby={by}\n")
    );
    assert_eq!(
        read(saved_games.join("trophies.dat")),
        format!("trophies=1 by={by}")
    );
    let played = version_after(&desk, &game, &session).await;
    assert_eq!(played.file_count, 4, "{played:?}");

    // It reaches the other device.
    let (_a, _b) = (desk.sync.start(), deck.sync.start());
    let check = deck.versions.before_play(&deck_entry).await.unwrap();
    assert!(
        matches!(&check.check, SaveCheck::Restored { version } if version.content_hash == played.content_hash),
        "{check:?}"
    );
    assert!(check.warnings.is_empty(), "{check:?}");
    for (path, text) in [
        (
            other_app_data.join("progress.txt"),
            format!("level=1 by={by}"),
        ),
        (
            other_documents.join("slot1.sav"),
            format!("slot level=1 by={by}"),
        ),
        (
            other_local.join("settings.ini"),
            format!("[game]\nby={by}\n"),
        ),
        (
            other_saved_games.join("trophies.dat"),
            format!("trophies=1 by={by}"),
        ),
    ] {
        assert_eq!(read(&path), text, "{}", path.display());
    }

    // Played there, and back in the real folders.
    write(other_app_data.join("progress.txt"), "level=50 by=elsewhere");
    write(
        other_documents.join("slot1.sav"),
        "slot level=50 by=elsewhere",
    );
    let from_there = deck
        .versions
        .capture(&deck_entry, None, VersionReason::Session)
        .await
        .unwrap()
        .expect("a version on the Deck");
    let mut check = desk.versions.before_play(&entry).await.unwrap();
    for _ in 0..50 {
        if matches!(&check.check, SaveCheck::Restored { .. }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        check = desk.versions.before_play(&entry).await.unwrap();
    }
    assert!(
        matches!(&check.check, SaveCheck::Restored { version } if version.id == from_there.id),
        "{check:?}"
    );
    assert!(check.warnings.is_empty(), "{check:?}");
    assert_eq!(read(app_data.join("progress.txt")), "level=50 by=elsewhere");
    assert_eq!(
        read(documents.join("slot1.sav")),
        "slot level=50 by=elsewhere"
    );

    // The real game reads it from there.
    play(&desk, &entry).await;
    let run = last_run(&folder);
    eprintln!("second play on Windows:\n{run}");
    assert!(run.contains("progress=[level=50 by=elsewhere]"), "{run}");
    assert!(run.contains("slot=[slot level=50 by=elsewhere]"), "{run}");
    assert!(run.contains(&format!("wrote level=51 by={by}")), "{run}");
}
