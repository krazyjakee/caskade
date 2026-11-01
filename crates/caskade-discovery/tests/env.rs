//! #65: one fixture tree, three host profiles, no ambient environment.
use crate::fixtures::write;
use caskade_discovery::{
    OperatingSystem,
    env::{HostEnv, KnownFolder, Volume, VolumeKind},
    registry::{RegistryHive, RegistryKey, RegistryView},
    testing::{FakeClock, FakeDrives},
};
use std::{
    ffi::OsString,
    fs, io,
    path::Path,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

fn fixture(metadata: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("root")).unwrap();
    fs::write(temp.path().join("machine.toml"), metadata).unwrap();
    temp
}

#[test]
fn the_same_tree_is_read_as_windows_linux_and_macos() {
    let temp = fixture("schema_version=1\nos='linux'\n");
    let root = temp.path().join("root").canonicalize().unwrap();
    for (os, home, config) in [
        (
            OperatingSystem::Windows,
            "drives/c/Users/fixture",
            "drives/c/Users/fixture/AppData/Roaming",
        ),
        (
            OperatingSystem::Linux,
            "home/fixture",
            "home/fixture/.config",
        ),
        (
            OperatingSystem::Macos,
            "Users/fixture",
            "Users/fixture/Library/Application Support",
        ),
    ] {
        write(
            &root,
            &format!("{config}/game.json"),
            br#"{"title":"Fixture Game"}"#,
        );
        let env = HostEnv::fixture_for(temp.path(), os).unwrap();
        assert_eq!(env.os(), os);
        assert_eq!(env.home(), root.join(home));
        let known = env.known_folder(KnownFolder::ConfigHome).unwrap();
        assert_eq!(known, root.join(config));
        assert_eq!(
            fs::read(known.join("game.json")).unwrap(),
            br#"{"title":"Fixture Game"}"#
        );
        for folder in [
            KnownFolder::Home,
            KnownFolder::ConfigHome,
            KnownFolder::DataHome,
            KnownFolder::CacheHome,
            KnownFolder::StateHome,
            KnownFolder::Documents,
            KnownFolder::Desktop,
        ] {
            assert!(env.known_folder(folder).unwrap().starts_with(&root));
        }
        assert_eq!(
            env.fixture_root(),
            Some(temp.path().canonicalize().unwrap().as_path())
        );
        assert_eq!(env.fs_root(), root);
    }
}

#[test]
fn fixture_defaults_do_not_leak_the_process_environment_or_clock() {
    let temp = fixture("");
    let first = HostEnv::fixture(temp.path()).unwrap();
    let second = HostEnv::fixture(temp.path()).unwrap();
    assert_eq!(first.os(), OperatingSystem::Linux);
    assert_eq!(first.var("HOME"), Some("/home/fixture".into()));
    assert_eq!(first.var("PATH"), None);
    assert_eq!(first.now(), UNIX_EPOCH + Duration::from_secs(1_700_000_000));
    assert_eq!(first.now(), second.now());
    assert_eq!(first.monotonic(), Duration::ZERO);
    let key = RegistryKey::new(RegistryHive::LocalMachine, RegistryView::Native, "Software");
    assert!(!first.registry().key_exists(&key).unwrap());
    assert!(first.registry().values(&key).unwrap().is_empty());
    assert_eq!(
        first.xdg_data_dirs(),
        [
            first.fs_root().join("usr/local/share"),
            first.fs_root().join("usr/share")
        ]
    );
}

#[test]
fn windows_known_folders_env_expansion_and_both_program_files_are_mapped() {
    let temp = fixture(
        r#"
os='windows'
[env]
USERPROFILE='C:\Users\Player'
appdata='D:\Roaming'
LOCALAPPDATA='D:\Local'
ProgramData='D:\Shared'
'ProgramFiles(x86)'='E:\Programs32'
ProgramFiles='E:\Programs64'
TOKEN='value with spaces'
[known_folders]
documents='%USERPROFILE%\Saved Games'
"#,
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    for (folder, path) in [
        (KnownFolder::Home, "drives/c/Users/Player"),
        (KnownFolder::AppData, "drives/d/Roaming"),
        (KnownFolder::LocalAppData, "drives/d/Local"),
        (KnownFolder::ProgramData, "drives/d/Shared"),
        (KnownFolder::ProgramFiles, "drives/e/Programs64"),
        (KnownFolder::ProgramFilesX86, "drives/e/Programs32"),
        (KnownFolder::Documents, "drives/c/Users/Player/Saved Games"),
        (
            KnownFolder::StartMenu,
            "drives/d/Roaming/Microsoft/Windows/Start Menu/Programs",
        ),
    ] {
        assert_eq!(env.known_folder(folder).unwrap(), env.fs_root().join(path));
    }
    assert_eq!(env.var("ApPdAtA"), Some(r"D:\Roaming".into()));
    assert_eq!(
        env.expand_vars("%token%/$TOKEN/${TOKEN}/%MISSING%/$MISSING"),
        OsString::from("value with spaces/value with spaces/value with spaces/%MISSING%/$MISSING")
    );
    assert_eq!(
        env.expand_path(r"%APPDATA%\Launcher\library.json").unwrap(),
        env.fs_root().join("drives/d/Roaming/Launcher/library.json")
    );
    assert_eq!(env.expand_path("~/game").unwrap(), env.home().join("game"));
}

#[test]
fn unix_xdg_paths_have_profile_separators_and_relative_overrides_are_ignored() {
    let temp = fixture(
        r#"
os='linux'
[env]
HOME='/home/player'
XDG_CONFIG_HOME='/custom/config'
XDG_DATA_HOME='relative-data'
XDG_DATA_DIRS='/opt/game-data:/usr/share::relative'
"#,
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert_eq!(
        env.known_folder(KnownFolder::ConfigHome).unwrap(),
        env.fs_root().join("custom/config")
    );
    assert_eq!(
        env.known_folder(KnownFolder::DataHome).unwrap(),
        env.fs_root().join("home/player/.local/share")
    );
    assert_eq!(
        env.xdg_data_dirs(),
        [
            env.fs_root().join("opt/game-data"),
            env.fs_root().join("usr/share")
        ]
    );
    assert_eq!(
        env.expand_path("${HOME}/Games").unwrap(),
        env.fs_root().join("home/player/Games")
    );
    assert_eq!(
        env.expand_vars("${UNCLOSED/$!/%UNKNOWN"),
        OsString::from("${UNCLOSED/$!/%UNKNOWN")
    );
}

#[test]
fn windows_case_separators_verbatim_and_unc_lookup_work_on_linux_too() {
    let temp = fixture("os='windows'");
    let root = temp.path().join("root");
    write(&root, "drives/c/Games/Café Game/config.INI", b"game");
    write(&root, "unc/Server/Share/Game/data.bin", b"network fixture");
    let env = HostEnv::fixture(temp.path()).unwrap();
    for path in [
        r"C:\GAMES\café game\CONFIG.ini",
        "c:/games/Café Game/config.ini",
        r"\\?\C:\Games\Café Game\config.ini",
    ] {
        let mapped = env.resolve_path(Path::new(path)).unwrap();
        assert_eq!(fs::read(&mapped).unwrap(), b"game");
        assert_eq!(env.resolve_path(&mapped).unwrap(), mapped);
    }
    for path in [
        r"\\server\share\game\DATA.bin",
        r"\\?\UNC\server\share\GAME\data.bin",
    ] {
        assert_eq!(
            fs::read(env.map_windows_path(path).unwrap()).unwrap(),
            b"network fixture"
        );
    }
}

#[test]
fn explicit_volume_case_semantics_override_the_profile_default() {
    let temp = fixture(
        r#"
os='windows'
[[volumes]]
path='D:\'
kind='removable'
case_sensitive=true
filesystem='testfs'
"#,
    );
    write(
        &temp.path().join("root"),
        "drives/d/Games/Case.txt",
        b"case",
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let volumes = env.volumes().unwrap();
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].kind, VolumeKind::Removable);
    assert_eq!(volumes[0].root, env.fs_root().join("drives/d"));
    assert_eq!(volumes[0].filesystem.as_deref(), Some("testfs"));
    match env.map_windows_path(r"D:\games\case.txt") {
        Ok(path) => assert!(!path.exists()),
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::NotFound),
    }
    assert!(
        env.map_windows_path(r"D:\Games\Case.txt")
            .unwrap()
            .is_file()
    );
}

#[test]
fn fixture_paths_cannot_escape_roots_or_volumes() {
    let temp = fixture("os='windows'");
    let env = HostEnv::fixture(temp.path()).unwrap();
    for path in [
        r"C:\..\outside",
        r"\\server\share\..\outside",
        r"C:relative",
        r"\\.\PhysicalDrive0",
        r"C:\data:stream",
        "C:\\nul\0tail",
    ] {
        assert!(env.map_windows_path(path).is_err(), "{path}");
    }
    let linux = HostEnv::fixture_for(temp.path(), OperatingSystem::Linux).unwrap();
    assert!(linux.resolve_path(Path::new("/../../outside")).is_err());
    assert!(
        linux
            .resolve_path(&linux.fs_root().join("../outside"))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn fixture_symlinks_can_stay_inside_but_never_reach_host_data() {
    let temp = fixture("os='linux'");
    let root = temp.path().join("root");
    write(&root, "data/game.txt", b"inside");
    let outside = temp.path().join("outside");
    fs::write(&outside, b"host sentinel").unwrap();
    std::os::unix::fs::symlink("data", root.join("inside")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert_eq!(
        fs::read(env.resolve_path(Path::new("/inside/game.txt")).unwrap()).unwrap(),
        b"inside"
    );
    assert_eq!(
        env.resolve_path(Path::new("/escape")).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(fs::read(&outside).unwrap(), b"host sentinel");
}

#[test]
fn fake_clock_wall_corrections_do_not_change_monotonic_budget_time() {
    let temp = fixture("now_unix_secs=1234");
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(100)));
    let env = HostEnv::fixture(temp.path())
        .unwrap()
        .with_clock(clock.clone());
    let other = env.clone();
    clock.advance(Duration::from_millis(25));
    assert_eq!(env.now(), UNIX_EPOCH + Duration::from_millis(100025));
    assert_eq!(other.monotonic(), Duration::from_millis(25));
    clock.set_wall_time(UNIX_EPOCH);
    assert_eq!(env.monotonic(), Duration::from_millis(25));
}

#[test]
fn drive_enumeration_and_failures_are_injected() {
    let temp = fixture("");
    let env = HostEnv::fixture(temp.path()).unwrap();
    let drives = vec![Volume {
        root: env.fs_root().join("media/games"),
        kind: VolumeKind::Removable,
        case_sensitive: true,
        filesystem: Some("ext4".into()),
    }];
    assert_eq!(
        env.clone()
            .with_drives(Arc::new(FakeDrives::new(drives.clone())))
            .volumes()
            .unwrap(),
        drives
    );
    let failure = env.with_drives(Arc::new(FakeDrives::failing(
        io::ErrorKind::PermissionDenied,
        "injected",
    )));
    assert_eq!(
        failure.volumes().unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn unknown_schemas_and_oversize_metadata_fail_without_writing() {
    let temp = fixture("schema_version=2");
    assert_eq!(
        HostEnv::fixture(temp.path()).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    fs::write(
        temp.path().join("machine.toml"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    assert_eq!(
        HostEnv::fixture(temp.path()).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(fs::read_dir(temp.path().join("root")).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn fixture_metadata_and_filesystem_root_cannot_be_symlinks_outside_the_scenario() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("machine.toml"), "os='windows'").unwrap();
    let metadata = fixture("");
    fs::remove_file(metadata.path().join("machine.toml")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("machine.toml"),
        metadata.path().join("machine.toml"),
    )
    .unwrap();
    assert_eq!(
        HostEnv::fixture(metadata.path()).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    let root = fixture("");
    fs::remove_dir(root.path().join("root")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("root")).unwrap();
    assert_eq!(
        HostEnv::fixture(root.path()).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        fs::read_to_string(outside.path().join("machine.toml")).unwrap(),
        "os='windows'"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn ambiguous_case_insensitive_fixture_names_are_reported_instead_of_guessed() {
    let temp = fixture("os='windows'");
    write(&temp.path().join("root"), "drives/c/Games/Foo", b"first");
    write(&temp.path().join("root"), "drives/c/Games/foo", b"second");
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert_eq!(
        env.map_windows_path(r"C:\Games\FOO").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}
