use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use caskade_installer::{
    Question, RuntimeSpec,
    discover::{DiscoveryConfig, DiscoveryHints, LaunchHint, discover, discover_with_config},
    host::HostCaps,
};
use serde_json::{Value, json};

fn hosts() -> Vec<(&'static str, HostCaps)> {
    [
        ("windows", include_str!("hosts/windows.toml")),
        ("linux", include_str!("hosts/linux.toml")),
        ("linux-no-umu", include_str!("hosts/linux-no-umu.toml")),
        (
            "linux-exfat-root",
            include_str!("hosts/linux-exfat-root.toml"),
        ),
        ("macos", include_str!("hosts/macos.toml")),
        ("macos-no-wine", include_str!("hosts/macos-no-wine.toml")),
    ]
    .into_iter()
    .map(|(name, text)| (name, toml::from_str(text).unwrap()))
    .collect()
}

fn host() -> HostCaps {
    hosts().remove(1).1
}

fn fixture(name: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin")
        .join(name);
    // These are fixed, committed, project-generated test archives. Production
    // extraction is handled by #9 through the shared safety module.
    zip::ZipArchive::new(fs::File::open(path).unwrap())
        .unwrap()
        .extract(root.path())
        .unwrap();
    root
}

fn portable_pe(gui: bool) -> Vec<u8> {
    let mut bytes = vec![0; 256];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[220..222].copy_from_slice(&(if gui { 2_u16 } else { 3 }).to_le_bytes());
    bytes
}

fn put(root: &Path, relative: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn portable_snapshot(mut value: Value) -> Value {
    fn visit(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if ["path", "working_dir", "icon"].contains(&key.as_str())
                        && let Value::String(path) = value
                    {
                        *path = path.replace('\\', "/");
                    } else {
                        visit(value);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    visit(value);
                }
            }
            _ => {}
        }
    }
    visit(&mut value);
    value
}

#[test]
fn unity_unreal_gog_rank_the_correct_exe_on_all_six_hosts() {
    let mut snapshots = BTreeMap::new();
    for (name, filename) in [
        ("unity", "unity.zip"),
        ("unreal", "unreal.zip"),
        ("gog", "gog.zip"),
    ] {
        let root = fixture(filename);
        for (profile, host) in hosts() {
            let result = discover(
                root.path(),
                &host,
                &DiscoveryHints {
                    installer_title: Some("Fixture Game".into()),
                    ..DiscoveryHints::default()
                },
            )
            .unwrap();
            assert_eq!(
                result.launch_targets[0].path,
                Path::new("FixtureGame.exe"),
                "{name}/{profile}"
            );
            assert!(result.question.is_none(), "{name}/{profile}");
            assert_eq!(result.title, "Fixture Game");
            assert!(
                result
                    .launch_targets
                    .iter()
                    .all(|target| target.path.is_relative() && target.working_dir.is_relative())
            );
            if name == "gog" {
                assert_eq!(result.ids["gog"], "1234567890");
                assert_eq!(result.ids["steam"], "480");
            }
            snapshots.insert(
                format!("{name}/{profile}"),
                portable_snapshot(serde_json::to_value(result).unwrap()),
            );
        }
    }
    insta::assert_json_snapshot!("fixture_host_rankings", snapshots);
}

#[test]
fn exact_signals_override_heuristics_and_preserve_argv() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "Game.exe", portable_pe(true));
    put(root.path(), "Alternate.exe", portable_pe(false));
    put(root.path(), ".itch.toml", b"name = 'Itch Game'\n[[actions]]\nname = 'play'\npath = 'Alternate.exe'\nargs = ['--save', 'data with spaces', 'C:\\Games\\file']\n");
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert_eq!(result.title, "Itch Game");
    assert_eq!(result.launch_targets[0].path, Path::new("Alternate.exe"));
    assert_eq!(
        result.launch_targets[0].args,
        ["--save", "data with spaces", "C:\\Games\\file"]
    );
    assert!(matches!(
        result.launch_targets[0].runtime,
        RuntimeSpec::ProtonPrefix { .. }
    ));
}

#[test]
fn desktop_exec_and_plist_executable_are_exact_signals() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "game/start.sh", b"#!/bin/sh\nexit 0\n");
    put(root.path(), "game/Game.desktop", b"[Desktop Entry]\nType=Application\nName=Desktop Game\nExec=\"./start.sh\" --save \"a b\" %f\nPath=.\n");
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert_eq!(result.title, "Desktop Game");
    assert_eq!(result.launch_targets[0].path, Path::new("game/start.sh"));
    assert_eq!(result.launch_targets[0].args, ["--save", "a b"]);
    assert_eq!(result.launch_targets[0].working_dir, Path::new("game"));

    let root = fixture("macos-app.zip");
    let result = discover(root.path(), &hosts()[4].1, &DiscoveryHints::default()).unwrap();
    assert_eq!(result.title, "Fixture Game");
    assert_eq!(result.ids["bundle"], "org.caskade.fixture-game");
    assert_eq!(
        result.launch_targets[0].path,
        Path::new("FixtureGame.app/Contents/MacOS/FixtureGame")
    );
    assert_eq!(
        result.launch_targets[0].reason,
        "Info.plist CFBundleExecutable"
    );
}

#[test]
fn metadata_title_priority_ids_and_installer_hooks() {
    let root = fixture("gog.zip");
    let hints = DiscoveryHints {
        installer_title: Some("Installer Name".into()),
        package_name: Some("Package Name".into()),
        resolver_title: Some("Resolver Name".into()),
        source_name: Some("setup_filename_1.0.exe".into()),
        source_url: Some("https://example.com/project".parse().unwrap()),
        ids: BTreeMap::from([("custom".into(), "id-1".into())]),
        launch_targets: vec![LaunchHint {
            path: "FixtureGame.exe".into(),
            args: vec!["--from-icons".into()],
            working_dir: Some(".".into()),
            reason: "Inno [Icons] hook".into(),
        }],
    };
    let result = discover(root.path(), &host(), &hints).unwrap();
    assert_eq!(result.title, "Fixture Game");
    assert_eq!(result.ids["source_url"], "https://example.com/project");
    assert_eq!(result.ids["custom"], "id-1");
    assert_eq!(result.launch_targets[0].args, ["--from-icons"]);
    assert_eq!(result.launch_targets[0].reason, "Inno [Icons] hook");
    fs::remove_file(root.path().join("goggame-1234567890.info")).unwrap();
    assert_eq!(
        discover(root.path(), &host(), &hints).unwrap().title,
        "Installer Name"
    );
}

#[test]
fn a_launch_hint_by_name_finds_the_program_inside_a_wrapping_folder() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "Port-v1.2/bin/Port.exe",
        "Port-v1.2/Tools.exe",
        "deeper/a/b/Port.exe",
    ] {
        put(root.path(), name, portable_pe(true));
    }
    let hints = DiscoveryHints {
        launch_targets: vec![LaunchHint {
            path: "port.exe".into(),
            args: vec![],
            working_dir: None,
            reason: "the port's install plan".into(),
        }],
        ..Default::default()
    };
    let result = discover(root.path(), &host(), &hints).unwrap();
    assert!(result.question.is_none(), "{:?}", result.question);
    assert_eq!(
        result.launch_targets[0].path,
        PathBuf::from("Port-v1.2/bin/Port.exe")
    );
    assert_eq!(
        result.launch_targets[0].working_dir,
        PathBuf::from("Port-v1.2/bin")
    );
}

#[test]
fn ties_require_a_launch_target_choice_and_denied_programs_are_excluded() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "One.exe",
        "Two.exe",
        "setup.exe",
        "unins000.exe",
        "UnityCrashHandler64.exe",
        "__redist/DXSETUP.exe",
        "_CommonRedist/vcredist.exe",
    ] {
        put(root.path(), name, portable_pe(true));
    }
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert_eq!(result.launch_targets.len(), 2);
    assert!(
        matches!(result.question, Some(Question::ChooseLaunchTarget { targets }) if targets.len() == 2)
    );
}

#[test]
fn gui_subsystem_and_launcher_penalty_affect_ranking() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "GameGui.exe", portable_pe(true));
    put(root.path(), "GameConsole.exe", portable_pe(false));
    put(root.path(), "Launcher.exe", portable_pe(true));
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert_eq!(result.launch_targets[0].path, Path::new("GameGui.exe"));
    assert_eq!(
        result.launch_targets.last().unwrap().path,
        Path::new("Launcher.exe")
    );
}

#[test]
fn elf_architecture_and_start_scripts_are_checked() {
    let root = tempfile::tempdir().unwrap();
    for (name, machine) in [("game.x86_64", 62_u16), ("wrong-arm", 183)] {
        let mut elf = vec![0; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[5] = 1;
        elf[18..20].copy_from_slice(&machine.to_le_bytes());
        put(root.path(), name, elf);
    }
    put(root.path(), "start.sh", b"#!/bin/sh\nexec ./game.x86_64\n");
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert_eq!(result.launch_targets[0].path, Path::new("start.sh"));
    assert!(
        !result
            .launch_targets
            .iter()
            .any(|target| target.path == Path::new("wrong-arm"))
    );
}

#[test]
fn malformed_metadata_and_missing_targets_produce_warnings() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "Game.exe", portable_pe(true));
    put(root.path(), "goggame-bad.info", b"{bad json");
    put(
        root.path(),
        ".itch.toml",
        b"[[actions]]\npath = '../../outside.exe'\n",
    );
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert_eq!(result.launch_targets.len(), 1);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.code == "invalid_launch_metadata")
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.code == "missing_launch_target")
    );
}

#[test]
fn metadata_reads_tree_size_and_tie_configuration_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "steam_appid.txt", b"1234567890");
    let result = discover_with_config(
        root.path(),
        &host(),
        &DiscoveryHints::default(),
        DiscoveryConfig {
            max_metadata_bytes: 4,
            ..DiscoveryConfig::default()
        },
    )
    .unwrap();
    assert!(!result.ids.contains_key("steam"));
    assert!(!result.warnings.is_empty());
    assert!(
        discover_with_config(
            root.path(),
            &host(),
            &DiscoveryHints::default(),
            DiscoveryConfig {
                max_entries: 0,
                ..DiscoveryConfig::default()
            }
        )
        .is_err()
    );
    assert!(
        discover_with_config(
            root.path(),
            &host(),
            &DiscoveryHints::default(),
            DiscoveryConfig {
                tie_threshold: f32::NAN,
                ..DiscoveryConfig::default()
            }
        )
        .is_err()
    );
}

#[test]
fn public_discovery_data_roundtrips_and_reflects() {
    let root = fixture("unity.zip");
    let hints = DiscoveryHints::default();
    let result = discover(root.path(), &host(), &hints).unwrap();
    assert_eq!(
        serde_json::from_value::<caskade_installer::discover::Discovery>(
            serde_json::to_value(&result).unwrap()
        )
        .unwrap(),
        result
    );
    assert_eq!(
        serde_json::to_value(hints).unwrap()["launch_targets"],
        json!([])
    );
    let _ = specta::Types::default()
        .register::<caskade_installer::discover::Discovery>()
        .register::<DiscoveryHints>()
        .register::<DiscoveryConfig>();
}

#[cfg(unix)]
#[test]
fn links_outside_the_game_are_not_scanned_or_read() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "Outside.exe", portable_pe(true));
    put(
        outside.path(),
        "goggame-evil.info",
        br#"{"name":"Outside","gameId":"evil"}"#,
    );
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("goggame-evil.info"),
        root.path().join("goggame-evil.info"),
    )
    .unwrap();
    let result = discover(root.path(), &host(), &DiscoveryHints::default()).unwrap();
    assert!(result.launch_targets.is_empty());
    assert!(!result.ids.contains_key("gog"));
}
