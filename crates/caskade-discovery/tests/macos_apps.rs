#![cfg(feature = "macos-sources")]
//! Authored macOS profile bundles; no Apple tools, receipts or private data.
use crate::fixtures::write;
use caskade_discovery::{sources::macos_apps::MacosApps, *};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
fn scenario() -> tempfile::TempDir {
    let t = crate::fixtures::tempdir();
    fs::create_dir(t.path().join("root")).unwrap();
    write(t.path(), "machine.toml", "schema_version=1\nos='macos'\n");
    t
}
fn app(root: &Path, path: &str, name: &str, id: &str, category: Option<&str>, binary: bool) {
    let mut dict = plist::Dictionary::new();
    for (k, v) in [
        ("CFBundleIdentifier", id),
        ("CFBundleName", name),
        ("CFBundleExecutable", "Game"),
        ("CFBundleIconFile", "GameIcon"),
    ] {
        dict.insert(k.into(), plist::Value::String(v.into()));
    }
    if let Some(c) = category {
        dict.insert(
            "LSApplicationCategoryType".into(),
            plist::Value::String(c.into()),
        );
    }
    let value = plist::Value::Dictionary(dict);
    let mut bytes = vec![];
    if binary {
        value.to_writer_binary(&mut bytes).unwrap();
    } else {
        value.to_writer_xml(&mut bytes).unwrap();
    }
    write(root, &format!("root/{path}/Contents/Info.plist"), bytes);
    // No executable permission needed: discovery describes a target, never runs it.
    write(
        root,
        &format!("root/{path}/Contents/MacOS/Game"),
        b"authored non-executable fixture",
    );
    write(
        root,
        &format!("root/{path}/Contents/Resources/GameIcon.icns"),
        b"fixture icon bytes",
    );
}
fn scan(root: &Path) -> Vec<DetectedGame> {
    MacosApps
        .scan(&HostEnv::fixture(root).unwrap(), &ScanCtx::default())
        .unwrap()
}
#[test]
fn xml_binary_game_categories_store_receipt_and_exact_executable_icon() {
    let t = scenario();
    app(
        t.path(),
        "Applications/Fixture.app",
        "Fixture Game",
        "org.caskade.fixture",
        Some("public.app-category.games"),
        false,
    );
    app(
        t.path(),
        "Users/fixture/Applications/Games/Café.app",
        "Café Puzzle",
        "org.caskade.puzzle",
        Some("public.app-category.puzzle-games"),
        true,
    );
    write(
        t.path(),
        "root/Applications/Fixture.app/Contents/_MASReceipt/receipt",
        b"authored marker; no actual signed receipt",
    );
    let env = HostEnv::fixture(t.path()).unwrap();
    let games = MacosApps.scan(&env, &ScanCtx::default()).unwrap();
    assert_eq!(games.len(), 2, "{games:?}");
    let game = games.iter().find(|g| g.title == "Fixture Game").unwrap();
    assert!(
        games.iter().all(
            |g| g.confidence == Confidence::Certain && matches!(g.runtime, GameRuntime::Native)
        )
    );
    assert!(
        game.evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule{rule,..}if rule=="store.mac_app_store"))
    );
    assert!(game.ids.is_empty()); // A receipt is no verified App Store/Arcade id.
    let dir = env.fs_root().join("Applications/Fixture.app");
    assert_eq!(
        game.launch,
        [LaunchTarget::Exec {
            program: dir.join("Contents/MacOS/Game"),
            args: vec![],
            cwd: Some(dir.join("Contents/MacOS")),
            env: BTreeMap::new()
        }]
    );
    assert_eq!(
        game.icon.as_deref(),
        Some(dir.join("Contents/Resources/GameIcon.icns").as_path())
    );
    assert_eq!(game.tracking.as_ref().unwrap().exe_names, ["Game"]);
    let report = scan_with_sources(
        &env,
        &Config::default(),
        &Cancel::new(),
        &[Arc::new(MacosApps)],
    )
    .unwrap();
    assert_eq!(report.groups.len(), 2);
    assert!(report.sources.iter().all(|s| s.error.is_none()));
    assert_eq!(MacosApps.watch_paths(&env).len(), 4);
}
#[test]
fn other_categories_are_classified_and_launcher_bundle_ids_are_always_excluded() {
    let t = scenario();
    app(
        t.path(),
        "Applications/Editor.app",
        "Fixture Editor",
        "org.fixture.editor",
        Some("public.app-category.developer-tools"),
        false,
    );
    app(
        t.path(),
        "Applications/Uncategorized.app",
        "Fixture Engine Game",
        "org.fixture.game",
        None,
        false,
    );
    write(
        t.path(),
        "root/Applications/Uncategorized.app/Contents/Resources/data.pck",
        b"Godot fixture",
    );
    // Heroic is itself categorized as a game upstream. Bundle-id identity wins.
    app(
        t.path(),
        "Applications/Heroic.app",
        "Heroic",
        "com.heroicgameslauncher.hgl",
        Some("public.app-category.games"),
        false,
    );
    let games = scan(t.path());
    assert_eq!(games.len(), 1, "{games:?}");
    assert_eq!(games[0].title, "Fixture Engine Game");
    assert_eq!(games[0].confidence, Confidence::Likely);
    assert!(
        games[0]
            .evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule{rule,..}if rule.contains("godot")))
    );
}
#[test]
fn only_applications_and_games_subfolders_are_walked_and_copies_stay_separate() {
    let t = scenario();
    for path in [
        "Applications/Game.app",
        "Applications/Games/Other.app",
        "Users/fixture/Applications/Copy.app",
        "Applications/OtherFolder/TooDeep.app",
        "Applications/Games/Subdir/TooDeep.app",
    ] {
        app(
            t.path(),
            path,
            "Same Game",
            "org.fixture.same",
            Some("public.app-category.games"),
            false,
        );
    }
    let games = scan(t.path());
    assert_eq!(games.len(), 3);
    let env = HostEnv::fixture(t.path()).unwrap();
    let groups = merge::merge(&env, games, &merge::MergeConfig::default()).unwrap();
    assert_eq!(groups.len(), 3);
}
#[test]
fn malformed_bundle_missing_exec_and_unsafe_paths_do_not_hide_healthy_apps() {
    let t = scenario();
    app(
        t.path(),
        "Applications/Good.app",
        "Good",
        "org.fixture.good",
        Some("public.app-category.games"),
        false,
    );
    write(
        t.path(),
        "root/Applications/Corrupt.app/Contents/Info.plist",
        b"bplist00corrupt",
    );
    app(
        t.path(),
        "Applications/Broken.app",
        "Broken",
        "org.fixture.broken",
        Some("public.app-category.games"),
        false,
    );
    fs::remove_file(
        t.path()
            .join("root/Applications/Broken.app/Contents/MacOS/Game"),
    )
    .unwrap();
    app(
        t.path(),
        "Applications/Unsafe.app",
        "Unsafe",
        "org.fixture.unsafe",
        Some("public.app-category.games"),
        false,
    );
    let path = t
        .path()
        .join("root/Applications/Unsafe.app/Contents/Info.plist");
    let mut value = plist::Value::from_file(&path).unwrap();
    value.as_dictionary_mut().unwrap().insert(
        "CFBundleExecutable".into(),
        plist::Value::String("../../Outside".into()),
    );
    value.to_file_xml(path).unwrap();
    let ctx = ScanCtx::default();
    let games = MacosApps
        .scan(&HostEnv::fixture(t.path()).unwrap(), &ctx)
        .unwrap();
    assert_eq!(games.len(), 2);
    let broken = games.iter().find(|g| g.title == "Broken").unwrap();
    assert!(matches!(broken.state, InstallState::Broken { .. }));
    assert_eq!(broken.confidence, Confidence::Maybe);
    assert!(!ctx.take_warnings().is_empty());
}
#[test]
fn empty_profile_and_cancellation_are_host_independent() {
    let t = scenario();
    let env = HostEnv::fixture(t.path()).unwrap();
    assert!(
        MacosApps
            .scan(&env, &ScanCtx::default())
            .unwrap()
            .is_empty()
    );
    assert!(MacosApps.detect(&env).is_empty());
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        MacosApps.scan(&env, &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    ));
}
#[cfg(unix)]
#[test]
fn app_and_executable_symlinks_do_not_escape_the_fixture_or_bundle() {
    let t = scenario();
    let other = crate::fixtures::tempdir();
    app(
        other.path(),
        "Applications/Outside.app",
        "Outside",
        "org.fixture.out",
        Some("public.app-category.games"),
        false,
    );
    fs::create_dir_all(t.path().join("root/Applications")).unwrap();
    std::os::unix::fs::symlink(
        other.path().join("root/Applications/Outside.app"),
        t.path().join("root/Applications/Outside.app"),
    )
    .unwrap();
    app(
        t.path(),
        "Applications/Inside.app",
        "Inside",
        "org.fixture.inside",
        Some("public.app-category.games"),
        false,
    );
    let exe = t
        .path()
        .join("root/Applications/Inside.app/Contents/MacOS/Game");
    fs::remove_file(&exe).unwrap();
    write(
        t.path(),
        "root/outside-executable",
        b"still insidefixture but outsidebundle",
    );
    std::os::unix::fs::symlink(t.path().join("root/outside-executable"), &exe).unwrap();
    assert!(scan(t.path()).is_empty());
}

#[test]
fn bundle_entry_budget_is_shared_across_all_application_roots() {
    let t = scenario();
    for name in ["A", "B"] {
        app(
            t.path(),
            &format!("Applications/{name}.app"),
            name,
            &format!("org.fixture.{name}"),
            Some("public.app-category.games"),
            false,
        );
    }
    let env = HostEnv::fixture(t.path()).unwrap();
    let config = Config {
        classifier: ClassifierConfig {
            max_entries: 1,
            ..ClassifierConfig::default()
        },
        ..Config::default()
    };
    let ctx = ScanCtx::with_config(Cancel::new(), Arc::new(config));
    let games = MacosApps.scan(&env, &ctx).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].title, "A");
    assert!(ctx.take_warnings().iter().any(|w| w.contains("budget")));
}
