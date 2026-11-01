use super::*;
use crate::{
    bus::Bus,
    db::LibraryStore,
    emulation::{
        EmulationService, Emulator, EmulatorInstall, EmulatorProfile, ProfileHooks, Tracking,
    },
};
use std::fs;

const PLATFORMS: &str = r#"
[[platform]]
id = "nintendo_snes"
name = "Super Nintendo Entertainment System"
short = "SNES"
extensions = ["sfc", "smc"]
ids = { esde = "snes", libretro = "Nintendo - Super Nintendo Entertainment System" }

[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd"]
ids = { esde = "psx" }

[[platform]]
id = "sega_genesis"
name = "Sega Genesis"
short = "MD"
extensions = ["md", "gen"]
ids = { esde = "megadrive" }
"#;

/// A PlayStation and Genesis emulator; nothing runs the SNES.
async fn service(temp: &Path) -> (EmulationService, ScannerService) {
    let store = LibraryStore::in_memory().await.unwrap();
    let catalog = Catalog::load([("platforms.toml", PLATFORMS)]).unwrap();
    let emulation = EmulationService::new(store, Bus::default(), catalog);
    let dir = temp.join("emu");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("emu"), b"").unwrap();
    let profile = |id: &str, platform: &str, extension: &str| EmulatorProfile::Custom {
        id: id.into(),
        name: id.to_uppercase(),
        executable: None,
        args: vec!["{RomPath}".into()],
        working_dir: None,
        platforms: vec![platform.into()],
        extensions: vec![extension.into()],
        env: vec![],
        tracking: Tracking::Default,
        hooks: ProfileHooks::default(),
    };
    emulation
        .save_emulator(Emulator {
            id: "emu".into(),
            name: "Emu".into(),
            definition: None,
            install: EmulatorInstall::Folder {
                dir,
                executable: "emu".into(),
            },
            profiles: vec![
                profile("psx", "sony_playstation", "chd"),
                profile("md", "sega_genesis", "md"),
            ],
        })
        .await
        .unwrap();
    let scanners = emulation.scanners();
    (emulation, scanners)
}

#[tokio::test]
async fn platform_folders_get_scanners_and_their_platforms_a_default() {
    let temp = tempfile::tempdir().unwrap();
    let (emulation, scanners) = service(temp.path()).await;
    let roms = temp.path().join("ROMs");
    for dir in ["psx", "SNES", "Genesis", "Music", ".thumbnails"] {
        fs::create_dir_all(roms.join(dir)).unwrap();
    }
    fs::write(roms.join("readme.txt"), b"").unwrap();

    let setup = scanners.setup_folder(&roms).await.unwrap();

    let made: Vec<_> = setup
        .added
        .iter()
        .map(|scanner| {
            (
                scanner.folder.file_name().unwrap().to_str().unwrap(),
                scanner.platform.as_deref().unwrap(),
                scanner.profile.profile_id.as_str(),
            )
        })
        .collect();
    assert_eq!(
        made,
        [
            ("Genesis", "sega_genesis", "md"),
            ("psx", "sony_playstation", "psx")
        ]
    );
    assert!(setup.added.iter().all(|scanner| scanner.in_library_update
        && scanner.launch_with == EmulatorTarget::PlatformDefault));
    assert_eq!(setup.no_emulator, ["Super Nintendo Entertainment System"]);
    assert_eq!(setup.unknown, ["Music"]);
    let platforms = emulation.platforms().await.unwrap();
    let default = |id: &str| {
        platforms
            .iter()
            .find(|view| view.platform.id == id)
            .and_then(|view| view.settings.default.clone())
            .map(|profile| profile.profile_id)
    };
    assert_eq!(default("sony_playstation").as_deref(), Some("psx"));
    assert_eq!(default("nintendo_snes"), None);

    // Again: the folders are covered already.
    let again = scanners.setup_folder(&roms).await.unwrap();
    assert!(again.added.is_empty());
    assert_eq!(again.existing, 2);
}

#[tokio::test]
async fn a_folder_named_after_a_platform_is_set_up_by_itself() {
    let temp = tempfile::tempdir().unwrap();
    let (_, scanners) = service(temp.path()).await;
    let roms = temp.path().join("PS1");
    fs::create_dir_all(roms.join("Some Game (USA)")).unwrap();

    let setup = scanners.setup_folder(&roms).await.unwrap();

    assert_eq!(setup.added.len(), 1);
    assert_eq!(setup.added[0].folder, roms);
    assert!(setup.unknown.is_empty());
}

#[test]
fn folder_names_match_loosely() {
    let catalog = Catalog::load([("platforms.toml", PLATFORMS)]).unwrap();
    let id = |name: &str| platform_named(&catalog, name).map(|platform| platform.id.as_str());
    assert_eq!(
        id("Nintendo - Super Nintendo Entertainment System"),
        Some("nintendo_snes")
    );
    assert_eq!(
        id("super-nintendo-entertainment-system"),
        Some("nintendo_snes")
    );
    assert_eq!(id("MegaDrive"), Some("sega_genesis"));
    assert_eq!(id("playstation"), Some("sony_playstation"));
    assert_eq!(id("ps2"), None);
    assert_eq!(id("---"), None);
}
