#![cfg(all(feature = "linux-sources", feature = "sqlite"))]
use crate::fixtures;
use caskade_discovery::{
    sources::{heroic::Heroic, lutris::Lutris, steam::Steam},
    *,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
fn machine() -> PathBuf {
    corpus().join("machines/lutris-linux")
}
fn scan(root: &Path) -> Vec<DetectedGame> {
    Lutris
        .scan(
            &HostEnv::fixture(root).unwrap(),
            &ScanCtx::new(Cancel::new()),
        )
        .unwrap()
}
#[test]
fn native_flatpak_old_new_yaml_prefixes_and_store_ids() {
    let env = HostEnv::fixture(machine()).unwrap();
    assert_eq!(Lutris.detect(&env).len(), 2);
    let games = scan(&machine());
    assert_eq!(games.len(), 5);
    let puzzle = games.iter().find(|g| g.key == "native:1").unwrap();
    assert!(puzzle.ids.contains(&ExternalId::Steam { app_id: 620 }));
    assert_eq!(puzzle.playtime_secs, Some(5400));
    assert_eq!(puzzle.last_played_unix, Some(1699999000));
    assert_eq!(
        puzzle.launch[0],
        LaunchTarget::Uri {
            uri: "lutris:rungameid/1".into()
        }
    );
    let LaunchTarget::Exec {
        program, args, cwd, ..
    } = &puzzle.launch[1]
    else {
        panic!("missing YAML executable")
    };
    assert!(program.ends_with("Games/Fixture Puzzle/game"));
    assert_eq!(
        args,
        &["--name", "Fixture Puzzle", "--literal", "$(no-shell)"]
    );
    assert_eq!(cwd, &puzzle.install_dir);
    let adventure = games.iter().find(|g| g.key == "native:2").unwrap();
    let GameRuntime::Windows {
        prefix: Some(prefix),
        runner: Some(runner),
    } = &adventure.runtime
    else {
        panic!("no Windows prefix")
    };
    assert!(prefix.path.ends_with("Games/prefixes/fixture"));
    assert_eq!(prefix.owner, Some(SourceId::new("lutris")));
    assert_eq!(prefix.arch, Some(PrefixArch::Win64));
    assert_eq!(runner.name.as_deref(), Some("wine-ge-fixture"));
    let LaunchTarget::Exec { program, args, .. } = &adventure.launch[1] else {
        panic!("missing game exe")
    };
    assert!(program.ends_with("drive_c/Game/adventure.exe"));
    assert_eq!(args, &["--name", "Fixture Adventure", ""]);
    let flatpak = games.iter().find(|g| g.key == "flatpak:1").unwrap();
    assert_eq!(flatpak.playtime_secs, Some(10800));
    assert_eq!(
        flatpak.launch[0],
        LaunchTarget::Flatpak {
            app_id: "net.lutris.Lutris".into(),
            args: vec!["lutris:rungameid/1".into()]
        }
    );
    assert_eq!(
        games
            .iter()
            .find(|g| g.key == "native:4")
            .unwrap()
            .playtime_secs,
        None
    );
    let watch = Lutris.watch_paths(&env);
    assert_eq!(
        watch.iter().filter(|p| p.path.ends_with("pga.db")).count(),
        2
    );
    fixtures::assert_snapshot("lutris_native_flatpak", &games, &machine());
}
#[test]
fn steam_and_gog_entries_merge_into_the_store_groups() {
    let env = HostEnv::fixture(machine()).unwrap();
    let sources: Vec<Arc<dyn Source>> = vec![Arc::new(Steam), Arc::new(Heroic), Arc::new(Lutris)];
    let report = scan_with_sources(&env, &Config::default(), &Cancel::new(), &sources).unwrap();
    assert!(
        report.sources.iter().all(|s| s.error.is_none()),
        "{report:?}"
    );
    assert_eq!(report.groups.len(), 4, "{report:?}");
    let steam = report
        .groups
        .iter()
        .find(|g| {
            g.entries
                .iter()
                .any(|e| e.ids.contains(&ExternalId::Steam { app_id: 620 }))
        })
        .unwrap();
    assert!(steam.entries.iter().any(|e| e.source.as_str() == "steam"));
    assert!(steam.entries.iter().any(|e| e.source.as_str() == "lutris"));
    let gog = report
        .groups
        .iter()
        .find(|g| {
            g.entries.iter().any(|e| {
                e.ids.contains(&ExternalId::Gog {
                    product_id: 1207658924,
                })
            })
        })
        .unwrap();
    assert!(gog.entries.iter().any(|e| e.source.as_str() == "heroic"));
    assert_eq!(
        gog.entries
            .iter()
            .filter(|e| e.source.as_str() == "lutris")
            .count(),
        2
    );
}
#[test]
fn locked_wal_database_is_read_without_modifying_original() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("machine");
    fixtures::Scenario::load(&corpus(), "lutris-linux")
        .unwrap()
        .materialize(&root)
        .unwrap();
    let path = root.join("root/home/fixture/.local/share/lutris/pga.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; BEGIN IMMEDIATE; UPDATE games SET name='Uncommitted name' WHERE id=1;").unwrap();
    let before = fs::read(&path).unwrap();
    let wal = fs::read(path.with_file_name("pga.db-wal")).unwrap();
    let games = scan(&root);
    assert_eq!(
        games.iter().find(|g| g.key == "native:1").unwrap().title,
        "Fixture Puzzle"
    );
    assert_eq!(before, fs::read(&path).unwrap());
    assert_eq!(wal, fs::read(path.with_file_name("pga.db-wal")).unwrap());
}
#[test]
fn malformed_database_and_yaml_do_not_hide_other_valid_installs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("machine");
    fixtures::Scenario::load(&corpus(), "lutris-linux")
        .unwrap()
        .materialize(&root)
        .unwrap();
    fs::write(
        root.join("root/home/fixture/.var/app/net.lutris.Lutris/data/lutris/pga.db"),
        "not SQLite",
    )
    .unwrap();
    fs::write(
        root.join("root/home/fixture/.config/lutris/games/puzzle-config.yml"),
        "game: [malformed",
    )
    .unwrap();
    let ctx = ScanCtx::new(Cancel::new());
    let games = Lutris
        .scan(&HostEnv::fixture(&root).unwrap(), &ctx)
        .unwrap();
    assert_eq!(games.len(), 3);
    assert!(ctx.take_warnings().len() >= 2);
    assert_eq!(
        games
            .iter()
            .find(|g| g.key == "native:1")
            .unwrap()
            .launch
            .len(),
        1
    );
    let cancel = Cancel::new();
    cancel.cancel();
    assert_eq!(
        Lutris.scan(&HostEnv::fixture(&root).unwrap(), &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    );
}

#[test]
fn stale_advertised_directory_is_broken_without_hiding_valid_installs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("machine");
    fixtures::Scenario::load(&corpus(), "lutris-linux")
        .unwrap()
        .materialize(&root)
        .unwrap();
    let before = scan(&root);
    let puzzle = before.iter().find(|g| g.key == "native:1").unwrap();
    assert_eq!(puzzle.state, InstallState::Installed);
    assert_eq!(puzzle.confidence, Confidence::Certain);
    fs::remove_dir_all(puzzle.install_dir.as_ref().unwrap()).unwrap();
    let after = scan(&root);
    assert_eq!(after.len(), before.len());
    for game in &after {
        let original = before.iter().find(|g| g.key == game.key).unwrap();
        if game.install_dir == puzzle.install_dir {
            assert!(matches!(game.state, InstallState::Broken { .. }));
            assert_eq!(game.confidence, Confidence::Maybe);
            assert_eq!(game.install_dir, original.install_dir);
            assert_eq!(game.launch, original.launch);
        } else {
            assert_eq!(game, original);
        }
    }
    // A regular file at an advertised directory must also fail closed.
    fs::write(puzzle.install_dir.as_ref().unwrap(), b"not a directory").unwrap();
    let after = scan(&root);
    let stale = after.iter().find(|g| g.key == "native:1").unwrap();
    assert!(matches!(stale.state, InstallState::Broken { .. }));
    assert_eq!(stale.confidence, Confidence::Maybe);
}
