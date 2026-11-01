use caskade_discovery::{
    GameRuntime, HostEnv, Prefix,
    identity::{PathContext, canonical_path, canonical_path_with},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn fixture(os: &str) -> (tempfile::TempDir, HostEnv) {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("root")).unwrap();
    fs::write(temp.path().join("machine.toml"),format!("schema_version = 1\nos = {os:?}\n[env]\nHOME = '/home/test'\nUSERPROFILE = 'C:\\Users\\test'\n")).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    (temp, env)
}
fn path(env: &HostEnv, value: &str) -> caskade_discovery::identity::CanonicalPath {
    canonical_path(env, Path::new(value), &GameRuntime::Native).unwrap()
}
#[test]
fn windows_case_unicode_verbatim_file_uri_and_unc() {
    let (_temp, env) = fixture("windows");
    fs::create_dir_all(env.fs_root().join("drives/c/Games/Café")).unwrap();
    let main = path(&env, r"C:\Games\Café");
    for spelling in [
        r"c:/GAMES/CAFÉ",
        r"\\?\C:\Games\Café",
        "file:///C:/Games/Caf%C3%A9",
    ] {
        assert!(main.same(&path(&env, spelling)), "{spelling}");
    }
    assert!(path(&env, r"\\server\share\Game").same(&path(&env, r"\\?\UNC\server\share\game")));
}
#[test]
fn linux_case_sensitivity_and_component_containment() {
    let (_temp, env) = fixture("linux");
    let a = path(&env, "/games/Foo");
    assert!(!a.same(&path(&env, "/games/foo")));
    assert!(!a.contains(&path(&env, "/games/Foobar/game")));
    assert!(!a.contains(&path(&env, "/games/Foo 2/game")));
    assert!(a.contains(&path(&env, "/games/Foo/bin/game")));
    assert!(a.same(&path(&env, "/games/Other/../Foo/./")));
}
#[test]
fn case_policy_comes_from_longest_matching_volume() {
    let (_temp, env) = fixture("linux");
    let special = env.fs_root().join("insensitive");
    let root = env.fs_root().to_path_buf();
    let env = env.with_drives(std::sync::Arc::new(
        caskade_discovery::testing::FakeDrives::new(vec![
            caskade_discovery::env::Volume {
                root,
                kind: caskade_discovery::env::VolumeKind::Fixed,
                case_sensitive: true,
                filesystem: None,
            },
            caskade_discovery::env::Volume {
                root: special,
                kind: caskade_discovery::env::VolumeKind::Removable,
                case_sensitive: false,
                filesystem: Some("exfat".into()),
            },
        ]),
    ));
    assert!(path(&env, "/insensitive/CAFÉ").same(&path(&env, "/insensitive/café")));
    assert!(!path(&env, "/other/CAFÉ").same(&path(&env, "/other/café")));
}
#[test]
fn flatpak_virtual_xdg_and_run_host_mapping() {
    let (_temp, env) = fixture("linux");
    let context = PathContext {
        flatpak_id: Some("com.heroicgameslauncher.hgl".into()),
        ..Default::default()
    };
    let virtual_path =
        canonical_path_with(&env, Path::new("~/.config/heroic/game"), &context).unwrap();
    let actual = path(
        &env,
        "/home/test/.var/app/com.heroicgameslauncher.hgl/config/heroic/game",
    );
    assert!(virtual_path.same(&actual));
    assert!(path(&env, "/run/host/home/test/Games/Foo").same(&path(&env, "/home/test/Games/Foo")));
    assert!(
        canonical_path_with(
            &env,
            Path::new("/run/host/home/test/.config/heroic/game"),
            &context
        )
        .unwrap()
        .same(&path(&env, "/home/test/.config/heroic/game"))
    );
    // Distinct sandbox storage is not the host's ~/.config directory.
    assert!(!actual.same(&path(&env, "/home/test/.config/heroic/game")));
}
#[test]
fn wine_c_and_z_map_to_host_without_guessing_other_drives() {
    let (_temp, env) = fixture("linux");
    let prefix = PathBuf::from("/home/test/prefix");
    fs::create_dir_all(env.resolve_path(&prefix).unwrap().join("drive_c/Games/Foo")).unwrap();
    let runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: prefix,
            arch: None,
            owner: None,
        }),
        runner: None,
    };
    let c = canonical_path(&env, Path::new(r"C:\Games\Foo"), &runtime).unwrap();
    assert!(c.same(&path(&env, "/home/test/prefix/drive_c/Games/Foo")));
    assert!(
        canonical_path(&env, Path::new(r"Z:\home\test\Games\Foo"), &runtime)
            .unwrap()
            .same(&path(&env, "/home/test/Games/Foo"))
    );
    assert!(canonical_path(&env, Path::new(r"E:\Unknown"), &runtime).is_err());
    assert!(canonical_path(&env, Path::new(r"C:relative"), &runtime).is_err());
}
#[cfg(unix)]
#[test]
fn links_missing_leaves_prefix_drives_and_fixture_escape() {
    let (_temp, env) = fixture("linux");
    let target = env.fs_root().join("games/Foo");
    fs::create_dir_all(&target).unwrap();
    std::os::unix::fs::symlink("games/Foo", env.fs_root().join("alias")).unwrap();
    assert!(path(&env, "/alias/missing").same(&path(&env, "/games/Foo/missing")));
    let prefix = env.fs_root().join("prefix");
    fs::create_dir_all(prefix.join("dosdevices")).unwrap();
    std::os::unix::fs::symlink("../../games", prefix.join("dosdevices/d:")).unwrap();
    let ctx = PathContext {
        prefix: Some(PathBuf::from("/prefix")),
        ..Default::default()
    };
    assert!(
        canonical_path_with(&env, Path::new(r"D:\Foo"), &ctx)
            .unwrap()
            .same(&path(&env, "/games/Foo"))
    );
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), env.fs_root().join("escape")).unwrap();
    assert!(canonical_path(&env, Path::new("/escape/payload"), &GameRuntime::Native).is_err());
}
#[cfg(unix)]
#[test]
fn non_unicode_names_remain_distinct() {
    use std::os::unix::ffi::OsStringExt;
    let (_temp, env) = fixture("linux");
    let a = env
        .fs_root()
        .join(std::ffi::OsString::from_vec(b"game\xff".to_vec()));
    let b = env
        .fs_root()
        .join(std::ffi::OsString::from_vec(b"game\xfe".to_vec()));
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    assert!(
        !canonical_path(&env, &a, &GameRuntime::Native)
            .unwrap()
            .same(&canonical_path(&env, &b, &GameRuntime::Native).unwrap())
    );
}
