#![cfg(feature = "windows-sources")]
use caskade_discovery::{sources::startmenu::StartMenuSource, *};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use crate::fixtures as support;
fn scenario() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/machines/startmenu-windows")
}
fn scan(env: &HostEnv) -> Vec<DetectedGame> {
    StartMenuSource::new()
        .scan(env, &ScanCtx::default())
        .unwrap()
}
#[test]
fn portable_shortcuts_preserve_target_arguments_working_directory_icon_and_confidence() {
    let env = HostEnv::fixture(scenario()).unwrap();
    let games = scan(&env);
    let game = games.iter().find(|g| g.title == "Portable Café").unwrap();
    assert_eq!(game.confidence, Confidence::Likely);
    let dir = env
        .resolve_path(Path::new(r"C:\Portable\Portable Café"))
        .unwrap();
    assert_eq!(game.install_dir, Some(dir.clone()));
    assert_eq!(game.icon, Some(dir.join("icon.ico")));
    assert_eq!(
        game.launch,
        [LaunchTarget::Exec {
            program: dir.join("game.exe"),
            args: vec!["--profile".into(), "Café user".into(), "&".into()],
            cwd: Some(dir.join("saves")),
            env: Default::default()
        }]
    );
    let environment = games.iter().find(|g| g.title == "Environment").unwrap();
    assert_eq!(environment.confidence, Confidence::Likely);
    assert!(
        matches!(&environment.launch[0],LaunchTarget::Exec{program,args,cwd:Some(cwd),..}if program==&env.home().join("Games/Environment/game.exe")&&args==&["--environment"]&&cwd==&env.home().join("Games/Environment"))
    );
    let relative = games.iter().find(|g| g.title == "Relative Café").unwrap();
    assert!(
        matches!(&relative.launch[0],LaunchTarget::Exec{program,args,..}if program==&dir.join("game.exe")&&args==&["--relative"])
    );
    assert_eq!(StartMenuSource::new().watch_paths(&env).len(), 2);
    assert!(
        StartMenuSource::new()
            .watch_paths(&env)
            .iter()
            .all(|p| p.recursive && p.path.starts_with(env.fs_root()))
    );
}
#[test]
fn actual_steam_epic_sources_merge_url_shortcuts_and_orphan_proxies_drop() {
    let env = HostEnv::fixture(scenario()).unwrap();
    let report = scan_with_sources(
        &env,
        &Config::default(),
        &Cancel::new(),
        &[
            Arc::new(sources::steam::Steam),
            Arc::new(sources::epic::EpicGamesLauncher),
            Arc::new(StartMenuSource::new()),
        ],
    )
    .unwrap();
    assert!(
        report.sources.iter().all(|r| r.error.is_none()),
        "{:?}",
        report.sources
    );
    assert_eq!(report.groups.len(), 4, "{:?}", report.groups);
    let steam: Vec<_> = report
        .groups
        .iter()
        .filter(|g| {
            g.entries
                .iter()
                .any(|e| e.ids.contains(&ExternalId::Steam { app_id: 620 }))
        })
        .collect();
    assert_eq!(steam.len(), 1);
    assert_eq!(steam[0].entries.len(), 3);
    assert_eq!(
        steam[0].primary_entry().unwrap().source,
        SourceId::new("steam")
    );
    let epic: Vec<_> = report
        .groups
        .iter()
        .filter(|g| {
            g.entries.iter().any(
                |e| matches!(&e.ids[..],[ExternalId::Epic{app_name,..}]if app_name=="FixtureEpic"),
            )
        })
        .collect();
    assert_eq!(epic.len(), 1);
    assert_eq!(epic[0].entries.len(), 2);
    assert_eq!(
        epic[0].primary_entry().unwrap().source,
        SourceId::new("epic")
    );
    assert!(!report.groups.iter().any(|g| {
        g.entries
            .iter()
            .any(|e| e.ids.contains(&ExternalId::Steam { app_id: 999999 }))
    }));
    let portable = report
        .groups
        .iter()
        .find(|g| g.entries.iter().any(|e| e.title == "Portable Café"))
        .unwrap();
    assert_eq!(portable.entries.len(), 3);
}
#[test]
fn non_games_broken_and_corrupt_shortcuts_are_excluded_with_clean_warnings() {
    let env = HostEnv::fixture(scenario()).unwrap();
    let ctx = ScanCtx::default();
    let games = StartMenuSource::new().scan(&env, &ctx).unwrap();
    let titles: std::collections::BTreeSet<_> = games.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Portable Café",
            "Portable Duplicate",
            "Relative Café",
            "Environment",
            "Portal 2",
            "Portal 2 command",
            "Epic Adventure",
            "Absent Steam game"
        ]
        .into_iter()
        .collect()
    );
    let warnings = ctx.take_warnings();
    assert!(warnings.iter().any(|w| w.contains("Corrupt.lnk")));
    assert!(warnings.iter().any(|w| w.contains("Missing.lnk")));
}
fn materialized() -> (tempfile::TempDir, HostEnv) {
    let t = tempfile::tempdir().unwrap();
    let dest = t.path().join("machine");
    support::Scenario::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        "startmenu-windows",
    )
    .unwrap()
    .materialize(&dest)
    .unwrap();
    let env = HostEnv::fixture(dest).unwrap();
    (t, env)
}
#[test]
fn empty_fixture_cancellation_and_directory_count_budgets_do_not_scan_host() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("root")).unwrap();
    fs::write(
        t.path().join("machine.toml"),
        "schema_version=1\nos='windows'\n",
    )
    .unwrap();
    let env = HostEnv::fixture(t.path()).unwrap();
    assert!(scan(&env).is_empty());
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        StartMenuSource::new().scan(&env, &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    ));
    let env = HostEnv::fixture(scenario()).unwrap();
    let mut config = Config::default();
    config.classifier.max_entries = 2;
    assert!(
        StartMenuSource::new()
            .scan(&env, &ScanCtx::with_config(Cancel::new(), Arc::new(config)))
            .is_err()
    );
}
#[test]
fn oversized_link_is_a_warning_and_healthy_siblings_survive() {
    let (_temp, env) = materialized();
    let root = env.known_folder(env::KnownFolder::StartMenu).unwrap();
    fs::write(root.join("Oversized.lnk"), vec![0; 1024 * 1024 + 1]).unwrap();
    let ctx = ScanCtx::default();
    let games = StartMenuSource::new().scan(&env, &ctx).unwrap();
    assert!(games.iter().any(|g| g.title == "Environment"));
    assert!(
        ctx.take_warnings()
            .iter()
            .any(|w| w.contains("Oversized.lnk"))
    );
}
#[cfg(unix)]
#[test]
fn symlinks_to_external_shortcuts_or_directories_never_leave_fixture() {
    use std::os::unix::fs::symlink;
    let (_temp, env) = materialized();
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("sentinel");
    fs::write(&sentinel, b"untouched").unwrap();
    fs::write(
        outside.path().join("External.url"),
        b"[InternetShortcut]\nURL=steam://rungameid/620\n",
    )
    .unwrap();
    let root = env.known_folder(env::KnownFolder::StartMenu).unwrap();
    symlink(outside.path(), root.join("Escaping")).unwrap();
    symlink(
        outside.path().join("External.url"),
        root.join("External.url"),
    )
    .unwrap();
    let games = scan(&env);
    assert!(!games.iter().any(|g| g.title == "External"));
    assert_eq!(fs::read(sentinel).unwrap(), b"untouched");
}

#[test]
fn public_builtin_scan_routes_startmenu_and_drops_launcher_only_shortcuts() {
    let env = HostEnv::fixture(scenario()).unwrap();
    let config = Config {
        sources: SourceSelection::only([SourceId::new("startmenu")]),
        ..Default::default()
    };
    let report = caskade_discovery::scan(&env, &config, &Cancel::new()).unwrap();
    assert_eq!(report.groups.len(), 2);
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].source, SourceId::new("startmenu"));
    assert!(report.sources[0].error.is_none());
}
