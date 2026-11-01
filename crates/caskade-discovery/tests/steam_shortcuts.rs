use std::path::PathBuf;

use caskade_discovery::{
    Cancel, DetectedGame, Evidence, ExternalId, GameRuntime, HostEnv, LaunchTarget, PrefixArch,
    RunnerKind, ScanCtx, Source,
    sources::steam::shortcuts::{PROXY_RULE, SteamShortcuts},
};

use crate::fixtures as support;

fn scenario() -> support::Scenario {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    support::Scenario::load(&fixtures, "steam-shortcuts").unwrap()
}

/// The source's own entries; merging (which folds proxies into their
/// launcher's group or drops them) is #69's.
fn scan(scenario: &support::Scenario) -> Vec<DetectedGame> {
    let env = HostEnv::fixture(&scenario.path).unwrap();
    let ctx = ScanCtx::new(Cancel::new());
    let games = SteamShortcuts.scan(&env, &ctx).unwrap();
    assert!(ctx.take_warnings().is_empty());
    games
}

fn by_title<'a>(games: &'a [DetectedGame], title: &str) -> &'a DetectedGame {
    games
        .iter()
        .find(|game| game.title == title)
        .unwrap_or_else(|| panic!("no shortcut {title}"))
}

fn proxy(game: &DetectedGame) -> bool {
    game.evidence
        .iter()
        .any(|evidence| matches!(evidence, Evidence::Rule { rule, .. } if rule == PROXY_RULE))
}

#[test]
fn shortcuts_become_games_or_launcher_proxies() {
    let scenario = scenario();
    let report = scan(&scenario);
    let mut titles: Vec<_> = report.iter().map(|game| game.title.as_str()).collect();
    titles.sort_unstable();
    // The RetroArch shortcut is left to emulator detection.
    assert_eq!(
        titles,
        [
            "Fixture Epic Quest",
            "Fixture Epic Two",
            "Fixture Indie",
            "Fixture Legacy",
            "Fixture Puzzle via Steam",
            "Fixture Windows Port",
        ]
    );

    let indie = by_title(&report, "Fixture Indie");
    assert_eq!(indie.key, "2294968295");
    assert_eq!(
        indie.ids,
        [ExternalId::SteamShortcut {
            app_id: 2_294_968_295
        }]
    );
    assert_eq!(
        indie.launch[0],
        LaunchTarget::Uri {
            uri: format!(
                "steam://rungameid/{}",
                (2_294_968_295_u64 << 32) | 0x0200_0000
            )
        }
    );
    let LaunchTarget::Exec {
        program, args, cwd, ..
    } = &indie.launch[1]
    else {
        panic!("{:?}", indie.launch);
    };
    assert!(program.ends_with("Games/Fixture Indie/indie"));
    assert_eq!(args, &["--windowed", "--profile=Fixture One"]);
    assert!(cwd.as_ref().unwrap().ends_with("Games/Fixture Indie"));
    assert!(
        indie
            .install_dir
            .as_ref()
            .unwrap()
            .ends_with("Games/Fixture Indie")
    );
    assert!(indie.icon.as_ref().unwrap().ends_with("icon.png"));
    assert_eq!(indie.last_played_unix, Some(1_699_100_000));
    assert_eq!(indie.runtime, GameRuntime::Native);
    assert!(!proxy(indie));

    let port = by_title(&report, "Fixture Windows Port");
    let GameRuntime::Windows {
        prefix: Some(prefix),
        runner: Some(runner),
    } = &port.runtime
    else {
        panic!("{:?}", port.runtime);
    };
    assert!(prefix.path.ends_with("steamapps/compatdata/2294968294/pfx"));
    assert_eq!(prefix.arch, Some(PrefixArch::Win64));
    assert_eq!(
        (runner.kind, runner.name.as_deref()),
        (RunnerKind::Proton, Some("GE-Proton10-34"))
    );

    let epic = by_title(&report, "Fixture Epic Quest");
    assert!(proxy(epic));
    assert_eq!(epic.install_dir, None);
    assert!(epic.launch.contains(&LaunchTarget::Uri {
        uri: "heroic://launch/legendary/FixtureEpicApp".into()
    }));
    assert!(epic.launch.contains(&LaunchTarget::Flatpak {
        app_id: "com.heroicgameslauncher.hgl".into(),
        args: vec![
            "--no-gui".into(),
            "--no-sandbox".into(),
            "heroic://launch/legendary/FixtureEpicApp".into()
        ],
    }));

    let legendary = by_title(&report, "Fixture Epic Two");
    assert!(proxy(legendary));
    assert!(legendary.launch.contains(&LaunchTarget::Exec {
        program: PathBuf::from("legendary"),
        args: vec!["launch".into(), "FixtureEpicTwo".into()],
        cwd: legendary.launch.iter().find_map(|target| match target {
            LaunchTarget::Exec { cwd, .. } => cwd.clone(),
            _ => None,
        }),
        env: Default::default(),
    }));

    let via_steam = by_title(&report, "Fixture Puzzle via Steam");
    assert!(proxy(via_steam));
    assert!(via_steam.ids.contains(&ExternalId::Steam { app_id: 620 }));

    // No appid in the file: Steam's exe + name formula.
    let legacy = by_title(&report, "Fixture Legacy");
    let expected = steamlocate::Shortcut::new(
        0,
        "Fixture Legacy".into(),
        "\"/home/fixture/Games/Fixture Legacy/legacy\"".into(),
        String::new(),
    )
    .steam_id();
    assert_eq!(legacy.key, (expected >> 32).to_string());

    insta::assert_json_snapshot!(
        "steam_shortcuts",
        support::relative_value(&report, &scenario.path).unwrap()
    );
}

#[test]
fn stale_shortcuts_are_broken_and_uncertain() {
    let scenario = scenario();
    let temp = tempfile::tempdir().unwrap();
    let machine = temp.path().join("machine");
    scenario.materialize(&machine).unwrap();
    std::fs::remove_dir_all(machine.join("root/home/fixture/Games/Fixture Indie")).unwrap();
    let env = HostEnv::fixture(&machine).unwrap();
    let games = SteamShortcuts
        .scan(&env, &ScanCtx::new(Cancel::new()))
        .unwrap();
    let indie = by_title(&games, "Fixture Indie");
    assert_eq!(indie.install_dir, None);
    assert_eq!(indie.confidence, caskade_discovery::Confidence::Maybe);
    assert!(matches!(
        indie.state,
        caskade_discovery::InstallState::Broken { .. }
    ));
    // Valid shortcuts and launcher proxies keep their confidence.
    let legacy = by_title(&games, "Fixture Legacy");
    assert_eq!(legacy.confidence, caskade_discovery::Confidence::Certain);
    assert!(legacy.install_dir.is_some());
    assert_eq!(
        by_title(&games, "Fixture Epic Two").confidence,
        caskade_discovery::Confidence::Certain
    );
}
