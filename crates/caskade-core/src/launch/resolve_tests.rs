#![cfg(unix)]
use super::*;
use caskade_discovery::{Prefix, RunnerHint, SourceId};
use std::collections::BTreeMap;

/// Wine names a start folder that's on no drive `unix\\…`, where a game's
/// relative paths find nothing, and a managed prefix has no `z:` (#11).
#[tokio::test]
async fn a_managed_windows_game_starts_on_its_game_drive() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root").unwrap();
    identity.write(&root).unwrap();
    let game = root.join("Fixture");
    std::fs::create_dir_all(game.join("bin")).unwrap();
    std::fs::write(game.join("bin/game.exe"), b"inert fixture").unwrap();
    let mut entry = GameEntry::new("game", SourceId::new("fixture"), "windows");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::Managed {
        root_id: identity.root_id().into(),
        rel_dir: "Fixture".into(),
    });
    entry.runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: ".caskade/prefix".into(),
            arch: None,
            owner: None,
        }),
        runner: Some(RunnerHint {
            kind: RunnerKind::Umu,
            path: Some("/bin/true".into()),
            name: None,
        }),
    };
    entry.launch = Some(LaunchSpec::Executable {
        path: "bin/game.exe".into(),
        args: Vec::new(),
        cwd: Some("bin".into()),
        env: BTreeMap::new(),
    });
    let storage = StorageRoot::from_manifest(root.clone(), &identity);
    let prepared = resolve(
        &entry,
        Some(storage),
        None,
        Arc::new(super::super::NoPluginRunner),
    )
    .await
    .unwrap();
    let drive = game.join(".caskade/prefix/dosdevices/g:/Game/bin");
    assert_eq!(prepared.command.args, vec![r"G:\Game\bin\game.exe"]);
    assert_eq!(prepared.command.cwd.as_deref(), Some(drive.as_path()));
    assert_eq!(prepared.command.env["PWD"], drive.to_string_lossy());
}

#[tokio::test]
async fn external_wine_prefix_keeps_its_own_drive_mappings() {
    let t = tempfile::tempdir().unwrap();
    let prefix = t.path().join("prefix");
    let game = prefix.join("drive_c/Games/Fixture");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::create_dir(prefix.join("dosdevices")).unwrap();
    std::os::unix::fs::symlink(prefix.join("drive_c"), prefix.join("dosdevices/c:")).unwrap();
    std::fs::write(game.join("game.exe"), b"inert fixture").unwrap();
    let mut entry = GameEntry::new("game", SourceId::new("fixture"), "windows");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External { path: game.clone() });
    entry.runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: prefix.clone(),
            arch: None,
            owner: None,
        }),
        runner: Some(RunnerHint {
            kind: RunnerKind::Wine,
            path: Some("/bin/true".into()),
            name: None,
        }),
    };
    entry.launch = Some(LaunchSpec::Executable {
        path: game.join("game.exe"),
        args: vec!["literal arg".into()],
        cwd: Some(game),
        env: BTreeMap::new(),
    });
    let prepared = resolve(&entry, None, None, Arc::new(super::super::NoPluginRunner))
        .await
        .unwrap();
    if !cfg!(windows) {
        assert_eq!(
            prepared.command.args,
            vec![r"C:\Games\Fixture\game.exe", "literal arg"]
        );
        assert!(prepared.direct);
        assert!(
            !prepared
                .target
                .excluded_executables
                .contains(&prepared.command.program)
        );
    }
    assert!(!prefix.join("dosdevices/g:").exists());
    assert_eq!(
        std::fs::read_dir(prefix.join("dosdevices"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn external_program_outside_existing_drives_is_rejected() {
    let t = tempfile::tempdir().unwrap();
    let prefix = t.path().join("prefix");
    std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
    let program = t.path().join("game.exe");
    std::fs::write(&program, b"x").unwrap();
    assert!(external_windows_path(&prefix, &program).is_err());
}

#[tokio::test]
async fn managed_prefix_cannot_follow_a_symlink_into_another_prefix() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("root");
    let game = root.join("game");
    let outside = t.path().join("outside");
    std::fs::create_dir_all(game.join(".caskade")).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep"), b"original").unwrap();
    std::os::unix::fs::symlink(&outside, game.join(".caskade/prefix")).unwrap();
    std::fs::write(game.join("game.exe"), b"MZ").unwrap();
    caskade_installer::manifest::RootManifest::new("root")
        .unwrap()
        .write(&root)
        .unwrap();
    let mut entry = GameEntry::new("game", SourceId::new("fixture"), "managed");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::Managed {
        root_id: "root".into(),
        rel_dir: "game".into(),
    });
    entry.runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: ".caskade/prefix".into(),
            arch: None,
            owner: None,
        }),
        runner: Some(RunnerHint {
            kind: RunnerKind::Wine,
            path: None,
            name: None,
        }),
    };
    entry.launch = Some(LaunchSpec::Executable {
        path: "game.exe".into(),
        args: vec![],
        cwd: Some(".".into()),
        env: BTreeMap::new(),
    });
    let result = resolve(
        &entry,
        Some(StorageRoot {
            id: "root".into(),
            path: root,
            is_default: true,
            label: None,
        }),
        None,
        Arc::new(super::super::NoPluginRunner),
    )
    .await;
    if !cfg!(windows) {
        assert!(matches!(result, Err(LaunchError::Invalid(_))));
    }
    assert_eq!(
        std::fs::read_to_string(outside.join("keep")).unwrap(),
        "original"
    );
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 1);
}
