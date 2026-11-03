use std::{fs, path::PathBuf, sync::Arc};

use caskade_discovery::{
    Cancel, Config, GameRuntime, HostEnv, InstallState, LauncherKind, PrefixArch, RunnerKind,
    ScanReport, Source, scan_with_sources,
    sources::steam::{ID, Steam},
};

use crate::fixtures as support;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn scan(machine: &std::path::Path) -> ScanReport {
    let env = HostEnv::fixture(machine).unwrap();
    let sources: [Arc<dyn Source>; 1] = [Arc::new(Steam)];
    scan_with_sources(&env, &Config::default(), &Cancel::new(), &sources).unwrap()
}

/// Fixture-relative snapshot, stored in `tests/snapshots` (outside the
/// fixture corpus).
fn snapshot(name: &str, report: &ScanReport, root: &std::path::Path) {
    insta::assert_json_snapshot!(name, support::relative_value(report, root).unwrap());
}

fn keys(report: &ScanReport) -> Vec<&str> {
    report
        .groups
        .iter()
        .map(|group| group.primary_entry().unwrap().key.as_str())
        .collect()
}

fn game<'a>(report: &'a ScanReport, key: &str) -> &'a caskade_discovery::DetectedGame {
    report
        .groups
        .iter()
        .flat_map(|group| &group.entries)
        .find(|game| game.key == key)
        .unwrap_or_else(|| panic!("no game {key}"))
}

#[test]
fn linux_native_and_flatpak_installs_share_a_library() {
    let scenario = support::Scenario::load(&fixtures(), "steam-linux-shared").unwrap();
    let report = scan(&scenario.path);
    assert_eq!(report.sources[0].error, None);
    // Proton Experimental (appinfo type), Steam Linux Runtime and the
    // Steamworks redistributables (tool list and names) are not games; the
    // shared library's games appear once.
    assert_eq!(keys(&report), ["2000010", "2000020", "2000030", "620"]);

    let kinds: Vec<_> = report.launchers.iter().map(|l| l.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            Some(LauncherKind::Native),
            Some(LauncherKind::Flatpak {
                app_id: "com.valvesoftware.Steam".into()
            })
        ]
    );
    assert_eq!(report.launchers[0].version.as_deref(), Some("1700000000"));

    assert_eq!(game(&report, "2000020").state, InstallState::Partial);
    let quest = game(&report, "2000010");
    let GameRuntime::Windows {
        prefix: Some(prefix),
        runner: Some(runner),
    } = &quest.runtime
    else {
        panic!("{:?}", quest.runtime);
    };
    assert!(prefix.path.ends_with("steamapps/compatdata/2000010/pfx"));
    assert_eq!(prefix.arch, Some(PrefixArch::Win64));
    assert_eq!(runner.kind, RunnerKind::Proton);
    assert_eq!(runner.name.as_deref(), Some("proton_experimental"));
    // Native (5 min) and Flatpak (30 min) accounts: the larger wins.
    assert_eq!(quest.playtime_secs, Some(1800));
    assert_eq!(quest.last_played_unix, Some(1_699_500_000));
    assert!(quest.icon.is_some());

    let puzzle = game(&report, "620");
    assert_eq!(puzzle.runtime, GameRuntime::Native);
    assert_eq!(puzzle.playtime_secs, Some(5400));
    assert_eq!(puzzle.last_played_unix, Some(1_699_990_000));
    assert_eq!(
        puzzle.launch[0],
        caskade_discovery::LaunchTarget::Uri {
            uri: "steam://rungameid/620".into()
        }
    );

    snapshot("steam_linux_shared", &report, &scenario.path);
}

#[test]
fn windows_install_with_three_libraries() {
    let scenario = support::Scenario::load(&fixtures(), "steam-windows-libraries").unwrap();
    let report = scan(&scenario.path);
    assert_eq!(keys(&report), ["2000040", "2000050", "620"]);
    assert_eq!(report.launchers.len(), 1);
    assert_eq!(game(&report, "2000040").state, InstallState::Updating);
    for group in &report.groups {
        let game = group.primary_entry().unwrap();
        assert_eq!(
            game.runtime,
            GameRuntime::Windows {
                prefix: None,
                runner: None
            }
        );
        assert!(
            game.install_dir
                .as_ref()
                .unwrap()
                .ends_with(format!("steamapps/common/{}", game.title))
        );
    }
    snapshot("steam_windows_libraries", &report, &scenario.path);
}

#[test]
fn no_steam_reports_a_missing_launcher() {
    let temp = crate::fixtures::tempdir();
    fs::write(
        temp.path().join("machine.toml"),
        "schema_version = 1\nos = \"linux\"\n",
    )
    .unwrap();
    fs::create_dir(temp.path().join("root")).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    let launchers = Steam.detect(&env);
    assert_eq!(launchers.len(), 1);
    assert!(!launchers[0].found);
    assert_eq!(launchers[0].source, ID);
    assert!(scan(temp.path()).groups.is_empty());
}

#[test]
fn hostile_manifests_become_warnings() {
    let scenario = support::Scenario::load(&fixtures(), "steam-linux-shared").unwrap();
    let temp = crate::fixtures::tempdir();
    let machine = temp.path().join("machine");
    scenario.materialize(&machine).unwrap();
    let steamapps = machine.join("root/home/fixture/.local/share/Steam/steamapps");
    fs::write(
        steamapps.join("appmanifest_9.acf"),
        "\"AppState\" { \"appid\" ",
    )
    .unwrap();
    fs::write(
        steamapps.join("appmanifest_10.acf"),
        "\"AppState\" { \"appid\" \"11\" }",
    )
    .unwrap();
    fs::write(steamapps.join("appmanifest_12.acf"), vec![b'{'; 100_000]).unwrap();
    fs::write(steamapps.join("appmanifest_x.acf"), "not an id").unwrap();
    let report = scan(&machine);
    assert_eq!(report.sources[0].error, None);
    assert_eq!(keys(&report), ["2000010", "2000020", "2000030", "620"]);
    assert_eq!(
        report.sources[0].warnings.len(),
        3,
        "{:?}",
        report.sources[0].warnings
    );
}

#[test]
fn watch_paths_cover_libraries_and_user_config() {
    let scenario = support::Scenario::load(&fixtures(), "steam-linux-shared").unwrap();
    let env = HostEnv::fixture(&scenario.path).unwrap();
    let paths = Steam.watch_paths(&env);
    let relative: Vec<_> = support::relative_value(&paths, &scenario.path)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path["path"].as_str().unwrap().to_owned())
        .collect();
    for expected in [
        "$FIXTURE/root/home/fixture/.local/share/Steam/steamapps",
        "$FIXTURE/root/home/fixture/.local/share/Steam/steamapps/libraryfolders.vdf",
        "$FIXTURE/root/home/fixture/.local/share/Steam/userdata/424242/config/localconfig.vdf",
        "$FIXTURE/root/mnt/games/SteamLibrary/steamapps",
        "$FIXTURE/root/home/fixture/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps",
    ] {
        assert!(
            relative.iter().any(|path| path == expected),
            "{expected} not in {relative:#?}"
        );
    }
}

#[test]
fn a_manifest_without_its_folder_is_broken_and_not_certain() {
    let scenario = support::Scenario::load(&fixtures(), "steam-linux-shared").unwrap();
    let temp = crate::fixtures::tempdir();
    let machine = temp.path().join("machine");
    scenario.materialize(&machine).unwrap();
    fs::remove_dir_all(
        machine.join("root/home/fixture/.local/share/Steam/steamapps/common/Fixture Puzzle"),
    )
    .unwrap();
    let report = scan(&machine);
    let puzzle = game(&report, "620");
    assert_eq!(
        puzzle.state,
        InstallState::Broken {
            reason: "install folder missing".into()
        }
    );
    assert_eq!(puzzle.confidence, caskade_discovery::Confidence::Likely);
    assert_eq!(puzzle.install_dir, None);
    // A download that has not created its folder yet stays partial.
    let downloading = game(&report, "2000020");
    assert_eq!(downloading.state, InstallState::Partial);
    assert_eq!(downloading.install_dir, None);
    // Valid installs are untouched.
    assert_eq!(
        game(&report, "2000030").confidence,
        caskade_discovery::Confidence::Certain
    );
}
