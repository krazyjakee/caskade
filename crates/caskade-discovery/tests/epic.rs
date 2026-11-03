//! Epic Games Launcher (#83) against a Windows machine built at test time.

use crate::fixtures::write;
use std::{fs, path::Path};

use caskade_discovery::{
    ExternalId, GameRuntime, HostEnv, InstallState, LaunchTarget, ScanCtx, Source,
    sources::epic::{EpicGamesLauncher, ID},
};

fn manifest(app: &str, extra: &str) -> String {
    format!(
        r#"{{
  "FormatVersion": 0,
  "bIsIncompleteInstall": false,
  "AppName": "{app}",
  "DisplayName": "Display {app}",
  "CatalogNamespace": "ns{app}",
  "CatalogItemId": "item{app}",
  "MainGameAppName": "{app}",
  "AppCategories": ["public", "games", "applications"],
  "InstallLocation": "C:\\Games\\{app}",
  "LaunchExecutable": "Binaries\\Win64\\{app}.exe",
  "ProcessNames": ["{app}-Shipping.exe"],
  "InstallSize": 4096{extra}
}}"#
    )
}

/// A Windows machine with the launcher, a game, a DLC, an incomplete
/// install, a stale manifest and a game whose folder is gone.
fn machine(root: &Path) {
    write(
        root,
        "machine.toml",
        "schema_version = 1\nos = \"windows\"\n",
    );
    write(
        root,
        "registry/hklm32.reg",
        "Windows Registry Editor Version 5.00\n\n[HKEY_LOCAL_MACHINE\\SOFTWARE\\Epic Games\\EpicGamesLauncher]\n\"AppDataPath\"=\"C:\\\\ProgramData\\\\Epic\\\\EpicGamesLauncher\\\\Data\\\\\"\n",
    );
    let drive = root.join("root/drives/c");
    let manifests = "ProgramData/Epic/EpicGamesLauncher/Data/Manifests";
    write(
        &drive,
        &format!("{manifests}/A1.item"),
        manifest("Quest", ""),
    );
    write(&drive, "Games/Quest/Binaries/Win64/Quest.exe", "inert");
    write(
        &drive,
        &format!("{manifests}/B2.item"),
        manifest("QuestSoundtrack", r#", "MainGameAppName": "Quest""#)
            .replace(r#""MainGameAppName": "QuestSoundtrack","#, ""),
    );
    write(
        &drive,
        &format!("{manifests}/C3.item"),
        manifest("Downloading", "").replace(
            r#""bIsIncompleteInstall": false"#,
            r#""bIsIncompleteInstall": true"#,
        ),
    );
    write(
        &drive,
        &format!("{manifests}/D4.item"),
        manifest("Stale", ""),
    );
    write(
        &drive,
        &format!("{manifests}/E5.item"),
        manifest("Moved", ""),
    );
    write(&drive, &format!("{manifests}/F6.item"), "{ not json");
    write(
        &drive,
        "ProgramData/Epic/UnrealEngineLauncher/LauncherInstalled.dat",
        r#"{"InstallationList": [
            {"InstallLocation": "C:\\Games\\Quest", "NamespaceId": "nsQuest", "ItemId": "itemQuest", "ArtifactId": "Quest", "AppName": "Quest"},
            {"InstallLocation": "C:\\Games\\Moved", "NamespaceId": "nsMoved", "ItemId": "itemMoved", "ArtifactId": "Moved", "AppName": "Moved"}
        ]}"#,
    );
    write(
        &drive,
        "Program Files (x86)/Epic Games/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe",
        "inert",
    );
}

#[test]
fn games_without_dlc_incomplete_or_stale_installs() {
    let temp = crate::fixtures::tempdir();
    machine(temp.path());
    let env = HostEnv::fixture(temp.path()).unwrap();
    let ctx = ScanCtx::default();
    let games = EpicGamesLauncher.scan(&env, &ctx).unwrap();
    let keys: Vec<_> = games.iter().map(|game| game.key.as_str()).collect();
    assert_eq!(keys, ["Moved", "Quest"]);
    let warnings = ctx.take_warnings();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("stale manifest for Stale"))
    );

    let quest = &games[1];
    assert_eq!(quest.title, "Display Quest");
    assert_eq!(
        quest.ids,
        [ExternalId::Epic {
            namespace: "nsQuest".into(),
            catalog_item: "itemQuest".into(),
            app_name: "Quest".into(),
        }]
    );
    assert_eq!(
        quest.launch,
        [LaunchTarget::Uri {
            uri: "com.epicgames.launcher://apps/nsQuest%3AitemQuest%3AQuest?action=launch&silent=true"
                .into()
        }]
    );
    assert_eq!(quest.state, InstallState::Installed);
    assert_eq!(quest.size, Some(4096));
    assert!(
        quest
            .install_dir
            .as_ref()
            .unwrap()
            .ends_with("drives/c/Games/Quest")
    );
    assert_eq!(
        quest.runtime,
        GameRuntime::Windows {
            prefix: None,
            runner: None
        }
    );
    assert_eq!(
        quest.tracking.as_ref().unwrap().exe_names,
        ["Quest.exe", "Quest-Shipping.exe"]
    );
    // Listed by the launcher but its folder is gone.
    assert!(matches!(games[0].state, InstallState::Broken { .. }));

    let launchers = EpicGamesLauncher.detect(&env);
    assert!(launchers[0].found);
    assert!(
        launchers[0]
            .path
            .as_ref()
            .unwrap()
            .ends_with("EpicGamesLauncher.exe")
    );
    assert!(
        launchers[0]
            .data_dir
            .as_ref()
            .unwrap()
            .ends_with("EpicGamesLauncher/Data")
    );
    let watched: Vec<_> = EpicGamesLauncher
        .watch_paths(&env)
        .into_iter()
        .map(|path| path.path)
        .collect();
    assert!(watched[0].ends_with("Data/Manifests"));
}

#[test]
fn default_data_dir_without_registry() {
    let temp = crate::fixtures::tempdir();
    machine(temp.path());
    fs::remove_file(temp.path().join("registry/hklm32.reg")).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    let games = EpicGamesLauncher.scan(&env, &ScanCtx::default()).unwrap();
    assert_eq!(games.len(), 2);
}

#[test]
fn no_launcher() {
    let temp = crate::fixtures::tempdir();
    write(
        temp.path(),
        "machine.toml",
        "schema_version = 1\nos = \"windows\"\n",
    );
    fs::create_dir_all(temp.path().join("root")).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    let launchers = EpicGamesLauncher.detect(&env);
    assert_eq!(
        (launchers[0].source.clone(), launchers[0].found),
        (ID, false)
    );
    assert!(
        EpicGamesLauncher
            .scan(&env, &ScanCtx::default())
            .unwrap()
            .is_empty()
    );
}
