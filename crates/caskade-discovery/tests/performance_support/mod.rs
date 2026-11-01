//! Generated-at-test-time workload. No game binaries, host data or large
//! committed machine copies. Setup is deliberately outside the measured scan.
use caskade_discovery::*;
use std::{
    fs,
    path::{Path, PathBuf},
};
pub const STEAM_GAMES: u32 = 1000;
pub const LIBRARIES: u32 = 10;
pub const FIRST_APP: u32 = 100_000_000;
pub struct Workload {
    pub _temp: tempfile::TempDir,
    pub env: HostEnv,
}
fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn overlay(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink());
        let to = destination.join(entry.file_name());
        if kind.is_dir() {
            overlay(&entry.path(), &to);
        } else if kind.is_file() && !to.exists() {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/machines")
        .join(name)
}
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
pub fn prepare(os: OperatingSystem) -> Workload {
    let temp = tempfile::tempdir().unwrap();
    let scenario = temp.path().join("machine");
    match os {
        OperatingSystem::Linux => {
            crate::fixtures::Scenario::load(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
                "linux-integration",
            )
            .unwrap()
            .materialize(&scenario)
            .unwrap();
            for extra in ["minigalaxy-linux", "faugus-linux"] {
                overlay(&fixture(extra).join("root"), &scenario.join("root"));
            }
            let bottle =
                scenario.join("root/home/fixture/.local/share/bottles/bottles/Performance");
            write(&bottle.join("bottle.yml"),b"Name: Performance\nArch: win64\nRunner: soda\nPath: Performance\nCustom_Path: false\nExternal_Programs:\n  fixture:\n    name: Performance Bottled Game\n    executable: game.exe\n    path: 'C:\\Games\\Bottled\\game.exe'\n");
            write(&bottle.parent().unwrap().parent().unwrap().join("library.yml"),b"game:\n  bottle:\n    name: Performance\n    path: Performance\n  id: fixture\n  name: Performance Bottled Game\n");
            write(
                &bottle.join("drive_c/Games/Bottled/game.exe"),
                b"original inert game",
            );
            write(
                &bottle.join("system.reg"),
                b"WINE REGISTRY Version 2\n\n#arch=win64\n",
            );
        }
        OperatingSystem::Windows => crate::fixtures::Scenario::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
            "windows-integration",
        )
        .unwrap()
        .materialize(&scenario)
        .unwrap(),
        OperatingSystem::Macos => {
            fs::create_dir_all(scenario.join("root")).unwrap();
            write(
                &scenario.join("machine.toml"),
                b"schema_version=1\nos='macos'\n",
            );
            let root = scenario.join("root");
            for setup in [
                crate::macos_support::steam,
                crate::macos_support::epic,
                crate::macos_support::heroic,
                crate::macos_support::battlenet,
                crate::macos_support::riot,
                crate::macos_support::gog,
                crate::macos_support::itch,
            ] {
                setup(&root);
            }
        }
    }
    let env = HostEnv::fixture(&scenario).unwrap();
    let native = match os {
        OperatingSystem::Linux => env.home().join(".local/share/Steam"),
        OperatingSystem::Windows => env.resolve_path(Path::new(r"C:\Steam")).unwrap(),
        OperatingSystem::Macos => env.home().join("Library/Application Support/Steam"),
    };
    let mut libraries = String::from("\"libraryfolders\"\n{\n");
    for library in 0..LIBRARIES {
        let symbolic = if library == 0 {
            match os {
                OperatingSystem::Linux => "/home/fixture/.local/share/Steam".into(),
                OperatingSystem::Windows => r"C:\Steam".into(),
                OperatingSystem::Macos => "/Users/fixture/Library/Application Support/Steam".into(),
            }
        } else {
            match os {
                OperatingSystem::Linux => format!("/home/fixture/SteamLibraries/Library{library}"),
                OperatingSystem::Windows => format!(r"D:\SteamLibraries\Library{library}"),
                OperatingSystem::Macos => format!("/Users/fixture/SteamLibraries/Library{library}"),
            }
        };
        let root = env.resolve_path(Path::new(&symbolic)).unwrap();
        fs::create_dir_all(root.join("steamapps/common")).unwrap();
        libraries.push_str(&format!(
            " \"{library}\" {{ \"path\" {} \"apps\" {{",
            quote(&symbolic)
        ));
        for index in 0..STEAM_GAMES / LIBRARIES {
            let app = FIRST_APP + library * (STEAM_GAMES / LIBRARIES) + index;
            let folder = format!("Synthetic_{app}");
            write(
                &root.join(format!("steamapps/appmanifest_{app}.acf")),
                format!(
                    "\"AppState\" {{ \"appid\" \"{app}\" \"name\" \"Synthetic game {app}\" \"StateFlags\" \"4\" \"installdir\" \"{folder}\" \"SizeOnDisk\" \"1024\" }}\n"
                ),
            );
            write(
                &root.join("steamapps/common").join(&folder).join("game.exe"),
                b"original inert synthetic game: never executed",
            );
            libraries.push_str(&format!(" \"{app}\" \"1024\""));
        }
        libraries.push_str(" } }\n");
    }
    libraries.push_str("}\n");
    write(&native.join("steamapps/libraryfolders.vdf"), libraries);
    Workload { _temp: temp, env }
}
