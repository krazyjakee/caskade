//! GOG Galaxy and offline installs (#84) on Windows machines built at test
//! time; the Galaxy database is generated with SQLite.
#![cfg(feature = "sqlite")]

use crate::fixtures::write;
use std::{fs, path::Path, sync::Arc};

use caskade_discovery::{
    Cancel, Config, ExternalId, HostEnv, InstallState, LaunchTarget, ScanCtx, Source,
    scan_with_sources,
    sources::{gog::GogGalaxy, heroic::Heroic},
};

const REG: &str = "Windows Registry Editor Version 5.00\n\n";

fn info(id: u64, root: Option<u64>, task: &str) -> String {
    let root = root.map_or(String::new(), |root| format!(r#""rootGameId": "{root}","#));
    format!(r#"{{"gameId": "{id}", {root} "name": "Info {id}", "playTasks": [{task}]}}"#)
}

/// Galaxy with three installed games, one DLC, titles and playtime.
fn galaxy_machine(root: &Path) {
    write(
        root,
        "machine.toml",
        "schema_version = 1\nos = \"windows\"\n",
    );
    write(
        root,
        "registry/hklm32.reg",
        format!(
            "{REG}[HKEY_LOCAL_MACHINE\\SOFTWARE\\GOG.com\\GalaxyClient\\paths]\n\"client\"=\"C:\\\\Program Files (x86)\\\\GOG Galaxy\"\n\n\
             [HKEY_LOCAL_MACHINE\\SOFTWARE\\GOG.com\\Games\\1207658930]\n\"gameID\"=\"1207658930\"\n\"gameName\"=\"Fixture Galaxy One\"\n\"path\"=\"C:\\\\GOG Games\\\\Galaxy One\"\n\"exe\"=\"C:\\\\GOG Games\\\\Galaxy One\\\\bin\\\\one.exe\"\n\n\
             [HKEY_LOCAL_MACHINE\\SOFTWARE\\GOG.com\\Games\\1207658999]\n\"gameID\"=\"1207658999\"\n\"gameName\"=\"Fixture Galaxy One - Expansion\"\n\"path\"=\"C:\\\\GOG Games\\\\Galaxy One\"\n\"dependsOn\"=\"1207658930\"\n"
        ),
    );
    let c = root.join("root/drives/c");
    let d = root.join("root/drives/d");
    write(
        &c,
        "Program Files (x86)/GOG Galaxy/GalaxyClient.exe",
        "inert",
    );
    write(
        &c,
        "GOG Games/Galaxy One/goggame-1207658930.info",
        info(
            1_207_658_930,
            Some(1_207_658_930),
            r#"{"isPrimary": true, "type": "FileTask", "path": "bin\\one.exe", "arguments": "-fullscreen \"-profile=A B\"", "workingDir": "bin", "name": "Play"}"#,
        ),
    );
    write(
        &c,
        "GOG Games/Galaxy One/goggame-1207658998.info",
        info(1_207_658_998, Some(1_207_658_930), ""),
    );
    write(
        &c,
        "GOG Games/Galaxy Two/goggame-1207658931.info",
        info(
            1_207_658_931,
            None,
            r#"{"isPrimary": false, "type": "FileTask", "path": "manual.pdf"}, {"isPrimary": true, "type": "FileTask", "path": "two.exe"}"#,
        ),
    );
    fs::create_dir_all(d.join("GOG/Galaxy Three")).unwrap();
    let db = c.join("ProgramData/GOG.com/Galaxy/storage/galaxy-2.0.db");
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE InstalledBaseProducts (productId INTEGER PRIMARY KEY, installationPath TEXT, installationDate TEXT);
            CREATE TABLE LimitedDetails (id INTEGER PRIMARY KEY, productId INTEGER, title TEXT);
            CREATE TABLE GameTimes (releaseKey TEXT, minutesInGame INTEGER);
            CREATE TABLE LastPlayedDates (gameReleaseKey TEXT, lastPlayedDate TEXT);
            INSERT INTO InstalledBaseProducts VALUES
                (1207658930, 'C:\GOG Games\Galaxy One', '2023-01-01'),
                (1207658931, 'C:\GOG Games\Galaxy Two', '2023-01-01'),
                (1207658932, 'D:\GOG\Galaxy Three', '2023-01-01'),
                (1207658998, 'C:\GOG Games\Galaxy One', '2023-01-01');
            INSERT INTO LimitedDetails (productId, title) VALUES
                (1207658930, 'Ignored DB Title'), (1207658931, 'Fixture Galaxy Two'),
                (1207658932, 'Fixture Galaxy Three');
            INSERT INTO GameTimes VALUES ('gog_1207658931', 120);
            INSERT INTO LastPlayedDates VALUES ('gog_1207658931', '2023-11-14 22:13:20');
            "#,
        )
        .unwrap();
}

#[test]
fn galaxy_with_three_games() {
    let temp = crate::fixtures::tempdir();
    galaxy_machine(temp.path());
    let env = HostEnv::fixture(temp.path()).unwrap();
    let ctx = ScanCtx::default();
    let games = GogGalaxy.scan(&env, &ctx).unwrap();
    assert_eq!(ctx.take_warnings(), Vec::<String>::new());
    let titles: Vec<_> = games
        .iter()
        .map(|game| (game.key.as_str(), game.title.as_str()))
        .collect();
    // The expansion (`dependsOn`) and the DLC (`rootGameId`) are not games;
    // the registry name wins over the database title.
    assert_eq!(
        titles,
        [
            ("1207658930", "Fixture Galaxy One"),
            ("1207658931", "Fixture Galaxy Two"),
            ("1207658932", "Fixture Galaxy Three"),
        ]
    );
    let one = &games[0];
    assert_eq!(
        one.ids,
        [ExternalId::Gog {
            product_id: 1_207_658_930
        }]
    );
    let LaunchTarget::Exec { program, args, .. } = &one.launch[0] else {
        panic!("{:?}", one.launch);
    };
    assert!(program.ends_with("GOG Galaxy/GalaxyClient.exe"));
    assert_eq!(
        args[..2],
        [
            "/command=runGame".to_owned(),
            "/gameId=1207658930".to_owned()
        ]
    );
    let LaunchTarget::Exec {
        program, args, cwd, ..
    } = &one.launch[1]
    else {
        panic!("{:?}", one.launch);
    };
    assert!(program.ends_with("Galaxy One/bin/one.exe"));
    assert_eq!(args, &["-fullscreen", "-profile=A B"]);
    assert!(cwd.as_ref().unwrap().ends_with("Galaxy One/bin"));
    assert_eq!(one.tracking.as_ref().unwrap().exe_names, ["one.exe"]);

    let two = &games[1];
    assert_eq!(two.playtime_secs, Some(7200));
    assert_eq!(two.last_played_unix, Some(1_700_000_000));
    // Not the first task: the primary one.
    assert!(
        matches!(&two.launch[1], LaunchTarget::Exec { program, .. } if program.ends_with("two.exe"))
    );
    // No play tasks and no registry exe: Galaxy only.
    let three = &games[2];
    assert_eq!(three.launch.len(), 1);
    assert_eq!(three.state, InstallState::Installed);
    assert!(
        three
            .install_dir
            .as_ref()
            .unwrap()
            .ends_with("drives/d/GOG/Galaxy Three")
    );

    let launchers = GogGalaxy.detect(&env);
    assert!(launchers[0].found);
    assert!(
        launchers[0]
            .data_dir
            .as_ref()
            .unwrap()
            .ends_with("Galaxy/storage")
    );
    assert!(
        GogGalaxy.watch_paths(&env)[0]
            .path
            .ends_with("galaxy-2.0.db")
    );
}

#[test]
fn offline_install_without_galaxy() {
    let temp = crate::fixtures::tempdir();
    let root = temp.path();
    write(
        root,
        "machine.toml",
        "schema_version = 1\nos = \"windows\"\n",
    );
    write(
        root,
        "registry/hklm64.reg",
        format!(
            "{REG}[HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games\\1207658940]\n\"gameName\"=\"Fixture Offline\"\n\"path\"=\"C:\\\\Games\\\\Offline\"\n\"launchCommand\"=\"\\\"C:\\\\Games\\\\Offline\\\\offline.exe\\\" -windowed\"\n"
        ),
    );
    write(
        &root.join("root/drives/c"),
        "Games/Offline/offline.exe",
        "inert",
    );
    let env = HostEnv::fixture(root).unwrap();
    assert!(!GogGalaxy.detect(&env)[0].found);
    let games = GogGalaxy.scan(&env, &ScanCtx::default()).unwrap();
    assert_eq!(games.len(), 1);
    let LaunchTarget::Exec { program, args, .. } = &games[0].launch[0] else {
        panic!("{:?}", games[0].launch);
    };
    assert!(program.ends_with("Games/Offline/offline.exe"));
    assert_eq!(args, &["-windowed"]);
    assert_eq!(games[0].launch.len(), 1);
}

#[test]
fn a_gog_game_also_in_heroic_merges_on_the_gog_id() {
    let temp = crate::fixtures::tempdir();
    galaxy_machine(temp.path());
    write(
        &temp
            .path()
            .join("root/drives/c/Users/fixture/AppData/Roaming/heroic"),
        "gog_store/installed.json",
        r#"{"installed": [{"appName": "1207658931", "platform": "windows", "install_path": "C:\\Heroic\\Galaxy Two"}]}"#,
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let sources: [Arc<dyn Source>; 2] = [Arc::new(Heroic), Arc::new(GogGalaxy)];
    let report = scan_with_sources(&env, &Config::default(), &Cancel::new(), &sources).unwrap();
    let group = report
        .groups
        .iter()
        .find(|group| group.entries.iter().any(|entry| entry.key == "1207658931"))
        .unwrap();
    let mut members: Vec<_> = group
        .entries
        .iter()
        .map(|entry| format!("{}:{}", entry.source, entry.key))
        .collect();
    members.sort();
    assert_eq!(members, ["gog:1207658931", "heroic:gog:1207658931"]);
    assert_eq!(report.groups.len(), 3);
}
