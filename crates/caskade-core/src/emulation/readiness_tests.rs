//! BIOS status, launch readiness and per-game settings (#122).
use super::*;
use crate::{
    bus::Bus,
    db::LibraryStore,
    model::{Game, GameEntry, LaunchSpec},
};
use md5::{Digest, Md5};
use std::path::{Path, PathBuf};

const BIOS: &[u8] = b"synthetic PS1 BIOS for tests";

fn md5(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn catalog() -> Catalog {
    let platforms = format!(
        r#"
[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue"]
bios = [
  {{ file = "scph5501.bin", md5 = "{}", required = true }},
  {{ file = "dc/optional.bin", required = false }},
]
"#,
        md5(BIOS)
    );
    Catalog::load([("platforms.toml", platforms.as_str())]).unwrap()
}

/// A portable emulator folder with one custom PS1 profile.
fn emulator(dir: &Path) -> Emulator {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("emu"), b"").unwrap();
    Emulator {
        id: "emu".into(),
        name: "Emu".into(),
        definition: None,
        install: EmulatorInstall::Folder {
            dir: dir.to_path_buf(),
            executable: "emu".into(),
        },
        profiles: vec![EmulatorProfile::Custom {
            id: "psx".into(),
            name: "PSX".into(),
            executable: None,
            args: vec!["{RomPath}".into()],
            working_dir: None,
            platforms: vec!["sony_playstation".into()],
            extensions: vec!["cue".into()],
            env: vec![],
            tracking: Tracking::Default,
            hooks: ProfileHooks::default(),
        }],
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    store: LibraryStore,
    service: EmulationService,
    emulator_dir: PathBuf,
    rom: PathBuf,
    entry: GameEntry,
}

async fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let service = EmulationService::new(store.clone(), Bus::default(), catalog());
    let rom = temp.path().join("ROMs/Game (USA).cue");
    std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
    std::fs::write(&rom, b"FILE").unwrap();
    let game = store.upsert_game(&Game::new("Game")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, crate::install::HANDOFF_SOURCE, "game");
    entry.launch = Some(LaunchSpec::Emulator {
        platform_id: "sony_playstation".into(),
        target: EmulatorTarget::PlatformDefault,
        roms: vec![GameRom {
            location: RomLocation::Absolute { path: rom.clone() },
            name: Some("Disc 1".into()),
            size: Some(4),
        }],
        args: ArgsOverride::Default,
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    Fixture {
        emulator_dir: temp.path().join("Emu"),
        _temp: temp,
        store,
        service,
        rom,
        entry,
    }
}

#[test]
fn bios_files_are_found_checked_and_looked_up_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let hashes = BiosHashes::default();
    let bios = Bios {
        file: "scph5501.bin".into(),
        md5: Some(md5(BIOS)),
        region: None,
        required: true,
    };
    let (first, second) = (temp.path().join("a"), temp.path().join("b"));
    let dirs = [first.clone(), second.clone()];
    assert_eq!(hashes.check(&bios, &dirs), (BiosState::Missing, None));
    std::fs::create_dir_all(&first).unwrap();
    std::fs::write(first.join("scph5501.bin"), b"another dump").unwrap();
    assert_eq!(hashes.check(&bios, &dirs).0, BiosState::WrongHash);
    // A matching dump further down the list wins over a wrong one.
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(second.join("scph5501.bin"), BIOS).unwrap();
    assert_eq!(
        hashes.check(&bios, &dirs),
        (BiosState::Found, Some(second.join("scph5501.bin")))
    );
    // Without a hash, any file of that name counts.
    let unhashed = Bios { md5: None, ..bios };
    assert_eq!(hashes.check(&unhashed, &dirs).0, BiosState::Found);

    // An override replaces the emulator's folders; the definition's `bios`
    // variable replaces `system`/`bios` next to the emulator.
    let context = VarContext {
        emulator_dir: Some(PathBuf::from("/emu")),
        ..VarContext::default()
    };
    let os = OperatingSystem::Linux;
    let kind = InstallKind::Folder;
    assert_eq!(
        bios_dirs(None, &Vars::default(), os, kind, &context),
        [PathBuf::from("/emu/system"), PathBuf::from("/emu/bios")]
    );
    assert_eq!(
        bios_dirs(Some(Path::new("/mine")), &Vars::default(), os, kind, &context),
        [PathBuf::from("/mine")]
    );
    let mut vars = Vars::default();
    vars.shared.insert("bios".into(), "{EmulatorDir}/firmware".into());
    assert_eq!(
        bios_dirs(None, &vars, os, kind, &context),
        [PathBuf::from("/emu/firmware")]
    );
}

#[tokio::test]
async fn readiness_walks_from_no_emulator_to_ready() {
    let f = fixture().await;
    let game = f.service.game(&f.entry.id).await.unwrap();
    assert_eq!(game.readiness, Readiness::NoEmulator);
    assert_eq!(game.message.as_deref(), Some("No emulator for PS1 — configure"));
    assert_eq!(game.discs[0].name.as_deref(), Some("Disc 1"));
    assert!(game.discs[0].exists);
    // Without an emulator the BIOS list is known, its state isn't.
    assert_eq!(game.bios[0].state, None);
    assert_eq!(f.service.overview().await.unwrap().bios_issues, Some(0));

    f.store.upsert_emulator(&emulator(&f.emulator_dir)).await.unwrap();
    f.service
        .set_platform_settings(PlatformSettings {
            platform: "sony_playstation".into(),
            default: Some(ProfileRef {
                emulator_id: "emu".into(),
                profile_id: "psx".into(),
            }),
            bios_dir: None,
        })
        .await
        .unwrap();
    let game = f.service.game(&f.entry.id).await.unwrap();
    assert_eq!(
        game.readiness,
        Readiness::MissingBios {
            files: vec!["scph5501.bin".into()]
        }
    );
    assert_eq!(game.profile.as_ref().unwrap().profile_id, "psx");
    // The optional file being absent is not a problem.
    let optional = game.bios.iter().find(|b| b.file == "dc/optional.bin").unwrap();
    assert_eq!(optional.state, Some(BiosState::Missing));
    assert_eq!(f.service.overview().await.unwrap().bios_issues, Some(1));
    let platform = f.service.platforms().await.unwrap().remove(0);
    assert_eq!(platform.platform.bios[0].state, Some(BiosState::Missing));

    // The emulator's own system folder has it.
    std::fs::create_dir_all(f.emulator_dir.join("system")).unwrap();
    std::fs::write(f.emulator_dir.join("system/scph5501.bin"), BIOS).unwrap();
    let game = f.service.game(&f.entry.id).await.unwrap();
    assert_eq!(game.readiness, Readiness::Ready);
    assert_eq!(game.message, None);
    assert_eq!(f.service.readiness(&f.entry).await.unwrap(), Readiness::Ready);
    assert_eq!(f.service.overview().await.unwrap().bios_issues, Some(0));

    // A platform BIOS folder override wins over the emulator's folder.
    let elsewhere = f.emulator_dir.with_file_name("BIOS");
    std::fs::create_dir_all(&elsewhere).unwrap();
    f.service
        .set_platform_settings(PlatformSettings {
            platform: "sony_playstation".into(),
            default: Some(ProfileRef {
                emulator_id: "emu".into(),
                profile_id: "psx".into(),
            }),
            bios_dir: Some(elsewhere.clone()),
        })
        .await
        .unwrap();
    let game = f.service.game(&f.entry.id).await.unwrap();
    assert!(matches!(game.readiness, Readiness::MissingBios { .. }));
    std::fs::write(elsewhere.join("scph5501.bin"), b"bad dump").unwrap();
    let game = f.service.game(&f.entry.id).await.unwrap();
    assert_eq!(game.bios[0].state, Some(BiosState::WrongHash));
    std::fs::write(elsewhere.join("scph5501.bin"), BIOS).unwrap();
    assert_eq!(f.service.game(&f.entry.id).await.unwrap().readiness, Readiness::Ready);

    // The emulator going away, then the ROM going away.
    std::fs::remove_file(f.emulator_dir.join("emu")).unwrap();
    let game = f.service.game(&f.entry.id).await.unwrap();
    assert_eq!(game.readiness, Readiness::NotInstalled);
    assert_eq!(game.message.as_deref(), Some("Emu isn't installed any more — configure"));
    std::fs::remove_file(&f.rom).unwrap();
    assert!(matches!(
        f.service.game(&f.entry.id).await.unwrap().readiness,
        Readiness::RomMissing { .. }
    ));
}

#[tokio::test]
async fn games_can_be_pinned_asked_and_given_arguments() {
    let f = fixture().await;
    f.store.upsert_emulator(&emulator(&f.emulator_dir)).await.unwrap();
    let pinned = EmulatorTarget::Profile {
        emulator_id: "emu".into(),
        profile_id: "psx".into(),
    };
    let args = ArgsOverride::Append {
        args: vec!["--fullscreen".into(), "{RomDir}".into()],
    };
    let game = f
        .service
        .set_game(&f.entry.id, pinned.clone(), args.clone())
        .await
        .unwrap();
    assert_eq!(game.target, pinned);
    assert_eq!(game.args, args);
    // No BIOS yet, but the pin itself resolves.
    assert!(matches!(game.readiness, Readiness::MissingBios { .. }));
    let stored = f.store.get_entry(&f.entry.id).await.unwrap().unwrap();
    assert!(matches!(
        stored.launch,
        Some(LaunchSpec::Emulator { target, .. }) if target == pinned
    ));

    let asked = f
        .service
        .set_game(&f.entry.id, EmulatorTarget::AskEmulator, ArgsOverride::Default)
        .await
        .unwrap();
    assert_eq!(asked.readiness, Readiness::Ask);
    assert_eq!(asked.profile, None);

    let unknown = EmulatorTarget::Profile {
        emulator_id: "emu".into(),
        profile_id: "nope".into(),
    };
    assert!(f.service.set_game(&f.entry.id, unknown, ArgsOverride::Default).await.is_err());
    let bad_args = ArgsOverride::Replace {
        args: vec!["{cores}".into()],
        replaced: vec![],
    };
    assert!(
        f.service
            .set_game(&f.entry.id, EmulatorTarget::PlatformDefault, bad_args)
            .await
            .is_err()
    );
    // Pinned to something deleted.
    f.service
        .set_game(&f.entry.id, pinned, ArgsOverride::Default)
        .await
        .unwrap();
    f.service.delete_emulator("emu").await.unwrap();
    assert_eq!(
        f.service.game(&f.entry.id).await.unwrap().readiness,
        Readiness::ProfileGone
    );
}

#[test]
fn scanner_rom_paths_stay_in_their_folder() {
    let folder = Path::new("/roms/psx");
    assert_eq!(
        RomLocation::scanner_path(folder, Path::new("Game/Game.cue")),
        Some(PathBuf::from("/roms/psx/Game/Game.cue"))
    );
    for escaping in ["../other/Game.cue", "Game/../../Game.cue", "/etc/Game.cue"] {
        assert_eq!(
            RomLocation::scanner_path(folder, Path::new(escaping)),
            None,
            "{escaping}"
        );
    }
}

#[tokio::test]
async fn a_rom_path_escaping_its_scanner_is_never_ready() {
    let f = fixture().await;
    f.store
        .upsert_emulator(&emulator(&f.emulator_dir))
        .await
        .unwrap();
    std::fs::create_dir_all(f.emulator_dir.join("system")).unwrap();
    std::fs::write(f.emulator_dir.join("system/scph5501.bin"), BIOS).unwrap();
    let pinned = EmulatorTarget::Profile {
        emulator_id: "emu".into(),
        profile_id: "psx".into(),
    };
    f.service
        .set_game(&f.entry.id, pinned, ArgsOverride::Default)
        .await
        .unwrap();
    let ready = f.store.get_entry(&f.entry.id).await.unwrap().unwrap();
    assert_eq!(f.service.readiness(&ready).await.unwrap(), Readiness::Ready);

    // A scanner one level below the ROM, so `..` reaches a file that exists.
    let folder = f.rom.parent().unwrap().join("Scanned");
    std::fs::create_dir_all(&folder).unwrap();
    f.store
        .upsert_scanner(&RomScanner {
            id: "psx-roms".into(),
            name: "PS1".into(),
            folder,
            platform: Some("sony_playstation".into()),
            profile: ProfileRef {
                emulator_id: "emu".into(),
                profile_id: "psx".into(),
            },
            launch_with: EmulatorTarget::PlatformDefault,
            recursive: true,
            scan_archives: false,
            skip_cloud_placeholders: false,
            exclusions: vec![],
            in_library_update: false,
        })
        .await
        .unwrap();
    // The store refuses such paths, so this entry is built in memory.
    for path in [PathBuf::from("../Game (USA).cue"), f.rom.clone()] {
        let mut entry = ready.clone();
        let Some(LaunchSpec::Emulator { roms, .. }) = &mut entry.launch else {
            unreachable!()
        };
        roms[0].location = RomLocation::Scanner {
            scanner_id: "psx-roms".into(),
            path: path.clone(),
        };
        let error = f.service.readiness(&entry).await.unwrap_err();
        assert!(
            error.to_string().contains("escapes its scanner folder"),
            "{path:?}: {error}"
        );
    }
}
