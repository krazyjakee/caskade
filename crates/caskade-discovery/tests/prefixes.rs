//! Windows sources inside Wine prefixes (#91) on Linux machines built at
//! test time.
#![cfg(unix)]

use crate::fixtures::write;
use std::{fs, os::unix::fs::symlink, path::Path};

use caskade_discovery::{
    ExternalId, GameRuntime, HostEnv, LaunchTarget, LauncherKind, PrefixArch, RunnerKind, ScanCtx,
    Source, SourceId,
    sources::prefixes::{Prefixes, is_prefix},
};

fn linux(root: &Path) -> std::path::PathBuf {
    write(root, "machine.toml", "schema_version = 1\nos = \"linux\"\n");
    root.join("root/home/fixture")
}

/// A Wine prefix with `system.reg` (given keys), a user and `dosdevices`.
fn prefix(dir: &Path, keys: &str) {
    write(
        dir,
        "system.reg",
        format!(
            "WINE REGISTRY Version 2\n;; All keys relative to \\\\Machine\n\n#arch=win64\n\n{keys}"
        ),
    );
    write(
        dir,
        "user.reg",
        "WINE REGISTRY Version 2\n;; All keys relative to \\\\User\\\\S-1-5-21-0-0-0-1000\n\n#arch=win64\n",
    );
    fs::create_dir_all(dir.join("drive_c/users/fixture/AppData/Roaming")).unwrap();
    fs::create_dir_all(dir.join("drive_c/users/Public")).unwrap();
    fs::create_dir_all(dir.join("drive_c/ProgramData")).unwrap();
    fs::create_dir_all(dir.join("dosdevices")).unwrap();
    symlink("../drive_c", dir.join("dosdevices/c:")).unwrap();
}

#[test]
fn gog_galaxy_in_a_lutris_prefix() {
    let temp = crate::fixtures::tempdir();
    let home = linux(temp.path());
    write(
        &home,
        ".config/lutris/games/gog-galaxy.yml",
        "game:\n  exe: 'C:\\Program Files (x86)\\GOG Galaxy\\GalaxyClient.exe'\n  prefix: /home/fixture/Games/gog-galaxy\nwine:\n  version: wine-ge-8-26\nrunner: wine\n",
    );
    let pfx = home.join("Games/gog-galaxy");
    prefix(
        &pfx,
        "[Software\\\\Wow6432Node\\\\GOG.com\\\\Games\\\\1207658930] 1700000000\n\
         \"gameName\"=\"Fixture Prefix Quest\"\n\
         \"path\"=\"C:\\\\GOG Games\\\\Prefix Quest\"\n\
         \"exe\"=\"C:\\\\GOG Games\\\\Prefix Quest\\\\quest.exe\"\n\n\
         [Software\\\\Wow6432Node\\\\GOG.com\\\\GalaxyClient\\\\paths] 1700000000\n\
         \"client\"=\"C:\\\\Program Files (x86)\\\\GOG Galaxy\"\n",
    );
    write(&pfx, "drive_c/GOG Games/Prefix Quest/quest.exe", "inert");
    write(
        &pfx,
        "drive_c/GOG Games/Prefix Quest/goggame-1207658930.info",
        r#"{"gameId": "1207658930", "name": "Prefix Quest", "playTasks": [{"isPrimary": true, "type": "FileTask", "path": "quest.exe"}]}"#,
    );
    write(
        &pfx,
        "drive_c/Program Files (x86)/GOG Galaxy/GalaxyClient.exe",
        "inert",
    );

    let env = HostEnv::fixture(temp.path()).unwrap();
    assert!(is_prefix(&env, &pfx));
    let ctx = ScanCtx::default();
    let games = Prefixes.scan(&env, &ctx).unwrap();
    let gog: Vec<_> = games
        .iter()
        .filter(|game| {
            game.ids.contains(&ExternalId::Gog {
                product_id: 1_207_658_930,
            })
        })
        .collect();
    assert_eq!(gog.len(), 1, "{games:#?}\n{:?}", ctx.take_warnings());
    let game = gog[0];
    assert_eq!(game.title, "Fixture Prefix Quest");
    assert_eq!(game.source, SourceId::new("prefixes"));
    let GameRuntime::Windows {
        prefix: Some(prefix),
        runner: Some(runner),
    } = &game.runtime
    else {
        panic!("{:?}", game.runtime);
    };
    assert!(prefix.path.ends_with("Games/gog-galaxy"));
    assert_eq!(prefix.owner, Some(SourceId::new("lutris")));
    assert_eq!(prefix.arch, Some(PrefixArch::Win64));
    assert_eq!(
        (runner.kind, runner.name.as_deref()),
        (RunnerKind::Wine, Some("wine-ge-8-26"))
    );
    assert!(
        game.install_dir
            .as_ref()
            .unwrap()
            .ends_with("drive_c/GOG Games/Prefix Quest")
    );
    let LaunchTarget::Exec { program, .. } = &game.launch[0] else {
        panic!("{:?}", game.launch);
    };
    assert!(program.ends_with("drive_c/Program Files (x86)/GOG Galaxy/GalaxyClient.exe"));

    // Galaxy itself is reported as a launcher inside the prefix.
    let launchers = Prefixes.detect(&env);
    assert!(
        launchers
            .iter()
            .any(|status| status.source == SourceId::new("gog")
                && status.kind
                    == Some(LauncherKind::Wine {
                        prefix: pfx.clone()
                    }))
    );
}

#[cfg(feature = "windows-sources")]
#[test]
fn a_retail_game_in_wine_through_its_uninstall_key() {
    let temp = crate::fixtures::tempdir();
    let home = linux(temp.path());
    let wine = home.join(".wine");
    prefix(
        &wine,
        "[Software\\\\Microsoft\\\\Windows\\\\CurrentVersion\\\\Uninstall\\\\Fixture Retail] 1700000000\n\
         \"DisplayName\"=\"Fixture Retail Racer\"\n\
         \"Publisher\"=\"Fixture Games\"\n\
         \"InstallLocation\"=\"C:\\\\Program Files (x86)\\\\Fixture Retail Racer\"\n\
         \"DisplayIcon\"=\"C:\\\\Program Files (x86)\\\\Fixture Retail Racer\\\\racer.exe\"\n\
         \"UninstallString\"=\"C:\\\\Program Files (x86)\\\\Fixture Retail Racer\\\\unins000.exe\"\n",
    );
    let game_dir = "drive_c/Program Files (x86)/Fixture Retail Racer";
    for file in [
        "racer.exe",
        "UnityPlayer.dll",
        "racer_Data/globalgamemanagers",
        "unins000.exe",
    ] {
        write(&wine, &format!("{game_dir}/{file}"), "inert");
    }
    let env = HostEnv::fixture(temp.path()).unwrap();
    let ctx = ScanCtx::default();
    let games = Prefixes.scan(&env, &ctx).unwrap();
    let racer = games
        .iter()
        .find(|game| game.title == "Fixture Retail Racer")
        .unwrap_or_else(|| panic!("{games:#?}\n{:?}", ctx.take_warnings()));
    let GameRuntime::Windows {
        prefix: Some(prefix),
        ..
    } = &racer.runtime
    else {
        panic!("{:?}", racer.runtime);
    };
    assert!(prefix.path.ends_with(".wine"));
    assert_eq!(prefix.owner, None);
    assert!(racer.install_dir.as_ref().unwrap().ends_with(game_dir));
}

#[test]
fn the_prefix_env_maps_drives_folders_and_case() {
    let temp = crate::fixtures::tempdir();
    let home = linux(temp.path());
    let wine = home.join(".wine");
    prefix(&wine, "");
    write(&wine, "drive_c/Games/MixedCase/Game.EXE", "inert");
    write(&home, "Shared/data.bin", "inert");
    symlink("/home/fixture/Shared", wine.join("dosdevices/d:")).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    let windows = env.wine_prefix(&wine).unwrap();
    assert_eq!(windows.os(), caskade_discovery::OperatingSystem::Windows);
    assert!(windows.home().ends_with("drive_c/users/fixture"));
    let mapped = windows
        .resolve_path(Path::new(r"C:\games\mixedcase\game.exe"))
        .unwrap();
    assert!(
        mapped.ends_with("drive_c/Games/MixedCase/Game.EXE"),
        "{mapped:?}"
    );
    let shared = windows.resolve_path(Path::new(r"D:\data.bin")).unwrap();
    assert!(shared.ends_with("home/fixture/Shared/data.bin"));
    assert!(windows.resolve_path(Path::new(r"C:\..\..\etc")).is_err());
    assert!(windows.resolve_path(Path::new(r"Q:\nothing")).is_err());
    // An absolute target is a host path: rebased into the fixture.
    symlink("/etc", wine.join("dosdevices/e:")).unwrap();
    // A relative link that climbs out of the fixture is not mapped.
    symlink(
        "../../../../../../../../../../../../etc",
        wine.join("dosdevices/f:"),
    )
    .unwrap();
    let windows = env.wine_prefix(&wine).unwrap();
    let rebased = windows.resolve_path(Path::new(r"E:\passwd")).unwrap();
    assert!(rebased.starts_with(temp.path()), "{rebased:?}");
    assert!(windows.resolve_path(Path::new(r"F:\passwd")).is_err());
}

#[test]
fn crossover_and_whisky_bottles_on_macos() {
    let temp = crate::fixtures::tempdir();
    write(
        temp.path(),
        "machine.toml",
        "schema_version = 1\nos = \"macos\"\n",
    );
    let home = temp.path().join("root/Users/fixture");
    let gog = |id: u64, name: &str| {
        format!(
            "[Software\\\\Wow6432Node\\\\GOG.com\\\\Games\\\\{id}] 1700000000\n\
             \"gameName\"=\"{name}\"\n\
             \"path\"=\"C:\\\\GOG Games\\\\{name}\"\n"
        )
    };
    let crossover = home.join("Library/Application Support/CrossOver/Bottles/Games");
    prefix(&crossover, &gog(1_207_658_950, "Fixture Crossover Quest"));
    write(
        &crossover,
        "drive_c/GOG Games/Fixture Crossover Quest/game.exe",
        "inert",
    );
    let whisky = home.join("Library/Containers/com.isaacmarovitz.Whisky/Bottles/5F6A");
    prefix(&whisky, &gog(1_207_658_951, "Fixture Whisky Quest"));
    write(
        &whisky,
        "drive_c/GOG Games/Fixture Whisky Quest/game.exe",
        "inert",
    );

    let env = HostEnv::fixture(temp.path()).unwrap();
    let mut found: Vec<_> = Prefixes
        .scan(&env, &ScanCtx::default())
        .unwrap()
        .into_iter()
        .map(|game| {
            let GameRuntime::Windows {
                prefix: Some(prefix),
                runner,
            } = game.runtime
            else {
                panic!("{:?}", game.runtime);
            };
            (game.title, prefix.path, runner.map(|runner| runner.kind))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].0, "Fixture Crossover Quest");
    assert!(found[0].1.ends_with("CrossOver/Bottles/Games"));
    assert_eq!(found[0].2, Some(RunnerKind::CrossOver));
    assert_eq!(found[1].0, "Fixture Whisky Quest");
    assert!(
        found[1]
            .1
            .ends_with("com.isaacmarovitz.Whisky/Bottles/5F6A")
    );
    assert_eq!(found[1].2, Some(RunnerKind::Wine));
}
