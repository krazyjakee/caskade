//! Bottles (#92) on a Linux machine built at test time.
#![cfg(unix)]

use crate::fixtures::write;
use std::{fs, os::unix::fs::symlink};

use caskade_discovery::{
    Confidence, GameRuntime, HostEnv, LaunchTarget, ScanCtx, Source, SourceId,
    sources::{bottles::Bottles, prefixes::Prefixes},
};

#[test]
fn a_game_and_a_non_game_program_in_one_bottle() {
    let temp = crate::fixtures::tempdir();
    write(
        temp.path(),
        "machine.toml",
        "schema_version = 1\nos = \"linux\"\n",
    );
    let data = temp.path().join("root/home/fixture/.local/share/bottles");
    let bottle = data.join("bottles/Gaming");
    write(
        &bottle,
        "bottle.yml",
        r"Name: Gaming
Arch: win64
Runner: soda-9.0-1
Path: Gaming
Custom_Path: false
External_Programs:
  1111-aaaa:
    executable: game.exe
    name: Fixture Bottled Game
    path: /home/fixture/.local/share/bottles/bottles/Gaming/drive_c/Games/Bottled/game.exe
  2222-bbbb:
    executable: editor.exe
    name: Fixture Text Editor
    path: 'C:\Tools\Editor\editor.exe'
  3333-cccc:
    executable: pick.exe
    name: Fixture Library Pick
    path: 'C:\Games\Picked\pick.exe'
",
    );
    write(
        &data,
        "library.yml",
        "abc:\n  bottle:\n    name: Gaming\n    path: Gaming\n  id: 3333-cccc\n  name: Fixture Library Pick\n",
    );
    for file in [
        "game.exe",
        "UnityPlayer.dll",
        "game_Data/globalgamemanagers",
    ] {
        write(&bottle, &format!("drive_c/Games/Bottled/{file}"), "inert");
    }
    write(&bottle, "drive_c/Tools/Editor/editor.exe", "inert");
    write(&bottle, "drive_c/Tools/Editor/README.txt", "inert");
    write(&bottle, "drive_c/Games/Picked/pick.exe", "inert");
    // Each bottle is a Wine prefix.
    write(
        &bottle,
        "system.reg",
        "WINE REGISTRY Version 2\n\n#arch=win64\n",
    );
    fs::create_dir_all(bottle.join("dosdevices")).unwrap();
    symlink("../drive_c", bottle.join("dosdevices/c:")).unwrap();

    let env = HostEnv::fixture(temp.path()).unwrap();
    let ctx = ScanCtx::default();
    let games = Bottles.scan(&env, &ctx).unwrap();
    let titles: Vec<_> = games
        .iter()
        .map(|game| (game.title.as_str(), game.confidence))
        .collect();
    // The editor has no game evidence; the library pick needs none.
    assert_eq!(
        titles,
        [
            ("Fixture Bottled Game", Confidence::Likely),
            ("Fixture Library Pick", Confidence::Certain)
        ],
        "{:?}",
        ctx.take_warnings()
    );
    let game = &games[0];
    assert_eq!(game.key, "Gaming/1111-aaaa");
    assert_eq!(
        game.launch,
        [LaunchTarget::Exec {
            program: "bottles-cli".into(),
            args: vec![
                "run".into(),
                "-b".into(),
                "Gaming".into(),
                "-p".into(),
                "Fixture Bottled Game".into()
            ],
            cwd: None,
            env: Default::default(),
        }]
    );
    let GameRuntime::Windows {
        prefix: Some(prefix),
        runner: Some(runner),
    } = &game.runtime
    else {
        panic!("{:?}", game.runtime);
    };
    assert!(prefix.path.ends_with("bottles/Gaming"));
    assert_eq!(prefix.owner, Some(SourceId::new("bottles")));
    assert_eq!(runner.name.as_deref(), Some("soda-9.0-1"));
    assert!(
        games[1]
            .install_dir
            .as_ref()
            .unwrap()
            .ends_with("drive_c/Games/Picked")
    );
    assert!(Bottles.detect(&env)[0].found);

    // The bottle is handed to the prefix scan.
    assert!(Prefixes.scan(&env, &ScanCtx::default()).is_ok());
    assert!(caskade_discovery::sources::prefixes::is_prefix(
        &env, &bottle
    ));
}
