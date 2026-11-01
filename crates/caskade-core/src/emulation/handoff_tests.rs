use super::*;
use crate::emulation::RomLocation;

const PLATFORMS: &str = r#"
[[platform]]
id = "nintendo_snes"
name = "Super Nintendo Entertainment System"
short = "SNES"
extensions = ["sfc", "smc", "zip"]

[[platform]]
id = "nintendo_gba"
name = "Game Boy Advance"
short = "GBA"
extensions = ["gba", "zip"]

[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd"]

[[region]]
id = "usa"
name = "USA"
codes = ["USA", "U"]
"#;

fn catalog() -> Catalog {
    Catalog::load([("platforms.toml", PLATFORMS)]).unwrap()
}

#[test]
fn installer_platforms_map_to_catalog_platforms() {
    let catalog = catalog();
    let files = |name: &str| vec![PathBuf::from(name)];
    assert_eq!(
        platform_for(&catalog, "nintendo_snes", &files("a.sfc")).as_deref(),
        Some("nintendo_snes")
    );
    assert_eq!(
        platform_for(&catalog, "nintendo_game_boy_advance", &files("a.gba")).as_deref(),
        Some("nintendo_gba")
    );
    // An id Caskade doesn't know falls back to a unique extension.
    assert_eq!(
        platform_for(&catalog, "unknown", &files("Game.CHD")).as_deref(),
        Some("sony_playstation")
    );
    assert_eq!(platform_for(&catalog, "unknown", &files("game.zip")), None);
    assert_eq!(platform_for(&catalog, "unknown", &files("game.bin")), None);
    assert_eq!(platform_for(&catalog, "unknown", &[]), None);
}

#[test]
fn discs_become_one_game_in_disc_order_keyed_on_the_first_rom() {
    let temp = tempfile::tempdir().unwrap();
    for (name, size) in [
        ("Final Fantasy VII (USA) (Disc 2).chd", 20),
        ("Final Fantasy VII (USA) (Disc 1).chd", 10),
    ] {
        std::fs::write(temp.path().join(name), vec![0u8; size]).unwrap();
    }
    let files = vec![
        PathBuf::from("Final Fantasy VII (USA) (Disc 2).chd"),
        PathBuf::from("Final Fantasy VII (USA) (Disc 1).chd"),
    ];
    let catalog = catalog();
    let (game, entry) = rom_entry("sony_playstation", temp.path(), &files, Some(&catalog));
    assert_eq!(game.title, "Final Fantasy VII");
    assert_eq!(entry.source_id, HANDOFF_SOURCE);
    assert_eq!(entry.state, InstallState::Installed);
    assert_eq!(entry.size, Some(30));
    let Some(LaunchSpec::Emulator {
        platform_id,
        target,
        roms,
        args,
    }) = &entry.launch
    else {
        panic!("{:?}", entry.launch);
    };
    assert_eq!(platform_id, "sony_playstation");
    assert_eq!(*target, EmulatorTarget::PlatformDefault);
    assert_eq!(*args, ArgsOverride::Default);
    let names: Vec<_> = roms.iter().map(|rom| rom.name.as_deref()).collect();
    assert_eq!(names, [Some("Disc 1"), Some("Disc 2")]);
    assert_eq!(
        roms[0].location,
        RomLocation::Absolute {
            path: temp.path().join("Final Fantasy VII (USA) (Disc 1).chd")
        }
    );
    // The first file handed over keys the entry, so a reinstall updates it.
    assert_eq!(
        entry.external_id,
        temp.path()
            .join("Final Fantasy VII (USA) (Disc 2).chd")
            .to_string_lossy()
    );
    let (_, again) = rom_entry("sony_playstation", temp.path(), &files, Some(&catalog));
    assert_eq!(again.external_id, entry.external_id);
}
