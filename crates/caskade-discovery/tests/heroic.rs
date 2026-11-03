use std::path::PathBuf;

use caskade_discovery::{
    Cancel, DetectedGame, ExternalId, GameRuntime, HostEnv, LaunchTarget, LauncherKind, RunnerKind,
    ScanCtx, Source,
    sources::{
        heroic::{FLATPAK_ID, Heroic},
        legendary::{Legendary, RARE},
    },
};

use crate::fixtures as support;

fn scenario() -> support::Scenario {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    support::Scenario::load(&fixtures, "epic-linux").unwrap()
}

fn scan(source: &dyn Source, scenario: &support::Scenario) -> Vec<DetectedGame> {
    let env = HostEnv::fixture(&scenario.path).unwrap();
    let ctx = ScanCtx::new(Cancel::new());
    let games = source.scan(&env, &ctx).unwrap();
    assert_eq!(ctx.take_warnings(), Vec::<String>::new());
    games
}

fn by_key<'a>(games: &'a [DetectedGame], key: &str) -> &'a DetectedGame {
    games
        .iter()
        .find(|game| game.key == key)
        .unwrap_or_else(|| panic!("no game {key}"))
}

#[test]
fn heroic_native_and_flatpak_with_all_four_libraries() {
    let scenario = scenario();
    let games = scan(&Heroic, &scenario);
    let keys: Vec<_> = games.iter().map(|game| game.key.as_str()).collect();
    // DLC (Epic and GOG) and browser sideloads are not games.
    assert_eq!(
        keys,
        [
            "gog:1207658930",
            "gog:1207658940",
            "legendary:FixtureEpicApp",
            "legendary:FixtureFlatEpic",
            "nile:amzn1.adg.product.fixture-0001",
            "nile:amzn1.adg.product.fixture-0002",
            "sideload:fixtureSideload01",
            "sideload:fixtureSideload02",
        ]
    );

    let epic = by_key(&games, "legendary:FixtureEpicApp");
    assert_eq!(epic.title, "Fixture Epic Quest");
    assert_eq!(
        epic.ids,
        [ExternalId::Epic {
            namespace: "fixturens01".into(),
            catalog_item: "fixtureitem01".into(),
            app_name: "FixtureEpicApp".into(),
        }]
    );
    assert_eq!(
        epic.launch,
        [LaunchTarget::Uri {
            uri: "heroic://launch/legendary/FixtureEpicApp".into()
        }]
    );
    let GameRuntime::Windows {
        prefix: Some(prefix),
        runner: Some(runner),
    } = &epic.runtime
    else {
        panic!("{:?}", epic.runtime);
    };
    assert!(prefix.path.ends_with("Prefixes/default/Fixture Epic Quest"));
    assert_eq!(runner.kind, RunnerKind::Proton);
    assert_eq!(runner.name.as_deref(), Some("Proton - GE-Proton10-34"));
    assert_eq!(epic.playtime_secs, Some(95 * 60));
    assert_eq!(epic.last_played_unix, Some(1_700_000_000));
    assert_eq!(
        epic.tracking.as_ref().unwrap().exe_names,
        ["FixtureEpic.exe"]
    );

    let gog = by_key(&games, "gog:1207658930");
    assert_eq!(gog.title, "Fixture GOG Native");
    assert_eq!(
        gog.ids,
        [ExternalId::Gog {
            product_id: 1_207_658_930
        }]
    );
    assert_eq!(gog.runtime, GameRuntime::Native);
    // The older gog_store/library.json gives the Flatpak install's title.
    assert_eq!(
        by_key(&games, "gog:1207658940").title,
        "Fixture Flatpak GOG"
    );

    let amazon = by_key(&games, "nile:amzn1.adg.product.fixture-0001");
    assert_eq!(amazon.title, "Fixture Amazon Drive");
    assert_eq!(
        amazon.ids,
        [ExternalId::Amazon {
            id: "amzn1.adg.product.fixture-0001".into()
        }]
    );
    assert_eq!(
        by_key(&games, "nile:amzn1.adg.product.fixture-0002").title,
        "Fixture Flatpak Amazon"
    );

    let sideload = by_key(&games, "sideload:fixtureSideload01");
    assert!(
        sideload
            .install_dir
            .as_ref()
            .unwrap()
            .ends_with("Games/Sideload")
    );

    // The Flatpak install launches through `flatpak run`.
    let flat = by_key(&games, "legendary:FixtureFlatEpic");
    assert_eq!(
        flat.launch[0],
        LaunchTarget::Flatpak {
            app_id: FLATPAK_ID.into(),
            args: vec![
                "--no-gui".into(),
                "--no-sandbox".into(),
                "heroic://launch/legendary/FixtureFlatEpic".into()
            ],
        }
    );

    let env = HostEnv::fixture(&scenario.path).unwrap();
    let kinds: Vec<_> = Heroic
        .detect(&env)
        .into_iter()
        .map(|status| status.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            Some(LauncherKind::Native),
            Some(LauncherKind::Flatpak {
                app_id: FLATPAK_ID.into()
            })
        ]
    );
    insta::assert_json_snapshot!(
        "heroic",
        support::relative_value(&games, &scenario.path).unwrap()
    );
}

#[test]
fn legendary_cli_and_rare() {
    let scenario = scenario();
    let games = scan(&Legendary, &scenario);
    let keys: Vec<_> = games.iter().map(|game| game.key.as_str()).collect();
    assert_eq!(keys, ["FixtureEpicApp", "FixtureEpicTwo"]);
    let two = by_key(&games, "FixtureEpicTwo");
    assert_eq!(
        two.ids,
        [ExternalId::Epic {
            namespace: "fixturens02".into(),
            catalog_item: "fixtureitem02".into(),
            app_name: "FixtureEpicTwo".into(),
        }]
    );
    assert_eq!(
        two.launch,
        [LaunchTarget::Exec {
            program: PathBuf::from("legendary"),
            args: vec!["launch".into(), "FixtureEpicTwo".into()],
            cwd: None,
            env: Default::default(),
        }]
    );
    let GameRuntime::Windows {
        prefix: Some(prefix),
        runner: Some(runner),
    } = &two.runtime
    else {
        panic!("{:?}", two.runtime);
    };
    assert!(prefix.path.ends_with("Games/Legendary/Prefix"));
    assert_eq!(runner.kind, RunnerKind::Wine);

    let env = HostEnv::fixture(&scenario.path).unwrap();
    let sources: Vec<_> = Legendary
        .detect(&env)
        .into_iter()
        .map(|status| (status.source, status.found))
        .collect();
    assert_eq!(sources, [(Legendary.id(), true), (RARE, true)]);
    insta::assert_json_snapshot!(
        "legendary",
        support::relative_value(&games, &scenario.path).unwrap()
    );
}

#[test]
fn missing_and_hostile_configs() {
    let scenario = scenario();
    let temp = crate::fixtures::tempdir();
    let machine = temp.path().join("machine");
    scenario.materialize(&machine).unwrap();
    let heroic = machine.join("root/home/fixture/.config/heroic");
    std::fs::write(heroic.join("gog_store/installed.json"), "{ not json").unwrap();
    std::fs::write(
        heroic.join("nile_config/nile/installed.json"),
        r#"[{"id": "../escape", "path": "/x"}]"#,
    )
    .unwrap();
    let env = HostEnv::fixture(&machine).unwrap();
    let ctx = ScanCtx::new(Cancel::new());
    let games = Heroic.scan(&env, &ctx).unwrap();
    assert_eq!(ctx.take_warnings().len(), 2);
    assert!(
        games
            .iter()
            .any(|game| game.key == "legendary:FixtureEpicApp")
    );
    assert!(!games.iter().any(|game| game.key == "gog:1207658930"));

    let empty = temp.path().join("empty");
    std::fs::create_dir_all(empty.join("root")).unwrap();
    std::fs::write(
        empty.join("machine.toml"),
        "schema_version = 1\nos = \"linux\"\n",
    )
    .unwrap();
    let env = HostEnv::fixture(&empty).unwrap();
    assert!(!Heroic.detect(&env)[0].found);
    assert!(!Legendary.detect(&env)[0].found);
    assert!(Heroic.scan(&env, &ScanCtx::default()).unwrap().is_empty());
}

#[test]
fn windows_config_locations() {
    let temp = crate::fixtures::tempdir();
    let machine = temp.path();
    std::fs::write(
        machine.join("machine.toml"),
        "schema_version = 1\nos = \"windows\"\n[env]\nUSERPROFILE = 'C:\\Users\\fixture'\n",
    )
    .unwrap();
    let user = machine.join("root/drives/c/Users/fixture");
    let legendary = user.join(".config/legendary");
    let heroic = user.join("AppData/Roaming/heroic/legendaryConfig/legendary");
    for dir in [&legendary, &heroic] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("installed.json"),
            r#"{"WinApp": {"app_name": "WinApp", "title": "Windows Fixture", "platform": "Windows",
                "install_path": "C:\\Games\\Windows Fixture", "executable": "win.exe", "is_dlc": false}}"#,
        )
        .unwrap();
    }
    std::fs::create_dir_all(machine.join("root/drives/c/Games/Windows Fixture")).unwrap();
    let env = HostEnv::fixture(machine).unwrap();
    let ctx = ScanCtx::default();
    let windows = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    let games = Legendary.scan(&env, &ctx).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].runtime, windows);
    assert!(
        games[0]
            .install_dir
            .as_ref()
            .unwrap()
            .ends_with("drives/c/Games/Windows Fixture")
    );
    let games = Heroic.scan(&env, &ctx).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].key, "legendary:WinApp");
    assert_eq!(games[0].runtime, windows);
    assert_eq!(ctx.take_warnings(), Vec::<String>::new());
}
