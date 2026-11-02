use super::*;
#[test]
fn uri_dispatch_is_one_literal_argument() {
    let uri = "steam://rungameid/620";
    let command = uri_command(uri).unwrap();
    assert_eq!(command.args.last().unwrap(), uri);
    assert!(command.env.is_empty());
    for bad in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://example.org",
        "-steam:x",
        "steam:x\n",
    ] {
        assert!(uri_command(bad).is_err(), "{bad}");
    }
}
#[test]
fn managed_paths_follow_alias_and_reject_parent_or_wrong_alias() {
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("Café Game");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("game"), b"game").unwrap();
    let aliases = caskade_installer::runtime::alias::AliasStore::new(t.path().join("aliases"));
    let alias = aliases.link("test", &real).unwrap();
    assert_eq!(
        managed_path(&real, Some(&alias), Path::new("game"), false).unwrap(),
        alias.join("game")
    );
    assert!(managed_path(&real, Some(&alias), Path::new("../other"), false).is_err());
    assert!(managed_path(&real, None, &real.join("game"), false).is_err());
    assert!(managed_path(&real, Some(t.path()), Path::new("game"), false).is_err());
}
#[cfg(unix)]
#[test]
fn managed_executable_symlink_cannot_escape_root() {
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("game");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(t.path().join("outside"), b"x").unwrap();
    std::os::unix::fs::symlink(t.path().join("outside"), real.join("exe")).unwrap();
    assert!(managed_path(&real, None, Path::new("exe"), false).is_err());
}

/// #245: a Flatpak app missing here is named before anything starts; a
/// `flatpak` that can't answer leaves the launch to go ahead.
#[cfg(unix)]
#[tokio::test]
async fn a_missing_flatpak_app_is_named_before_the_launch() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let stand_in = |name: &str, script: &str| {
        let path = t.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    let installed = stand_in("installed", "exit 0");
    let absent = stand_in(
        "absent",
        "echo 'error: org.example.Game not installed' >&2; exit 1",
    );
    assert_eq!(flatpak_missing(&installed, "org.example.Game").await, None);
    let missing = flatpak_missing(&absent, "org.example.Game").await.unwrap();
    assert!(
        missing.contains("org.example.Game isn't installed on this device"),
        "{missing}"
    );
    let no_flatpak = flatpak_missing(&t.path().join("no-such-flatpak"), "org.example.Game")
        .await
        .unwrap();
    assert!(
        no_flatpak.starts_with("Flatpak isn't installed"),
        "{no_flatpak}"
    );
}
