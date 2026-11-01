#![cfg(feature = "macos-sources")]
//! #96: native macOS locations are parsed on Linux without host data or launch.
#[cfg(feature = "sqlite")]
use caskade_discovery::sources::{gog::GogGalaxy, itch::Itch};
use caskade_discovery::{
    sources::{
        battlenet::BattleNet, epic::EpicGamesLauncher, heroic::Heroic, macos_apps::MacosApps,
        riot::RiotClient, steam::Steam,
    },
    *,
};
#[cfg(unix)]
use std::fs;
use std::{path::Path, sync::Arc};
#[path = "macos_support/mod.rs"]
mod macos_support;
use macos_support::*;
fn one(source: &dyn Source, setup: fn(&Path), key: &str) {
    let t = machine();
    setup(&t.path().join("root"));
    let env = HostEnv::fixture(t.path()).unwrap();
    let ctx = ScanCtx::default();
    let games = source.scan(&env, &ctx).unwrap();
    assert!(source.hosts().contains(OperatingSystem::Macos));
    assert_eq!(games.len(), 1, "{games:?} {:?}", ctx.take_warnings());
    assert_eq!(games[0].key, key);
    assert_eq!(games[0].runtime, GameRuntime::Native);
    assert_eq!(games[0].state, InstallState::Installed);
    assert!(source.detect(&env).iter().any(|c| c.found));
    assert!(
        games[0]
            .install_dir
            .as_ref()
            .unwrap()
            .starts_with(env.fs_root())
    );
    assert!(!source.watch_paths(&env).is_empty());
}
#[test]
fn steam_application_support_profile() {
    one(&Steam, steam, "620");
}
#[test]
fn epic_application_support_profile() {
    one(&EpicGamesLauncher, epic, "MacEpic");
}
#[test]
fn heroic_application_support_profile() {
    one(&Heroic, heroic, "legendary:MacHeroic");
}
#[test]
fn battlenet_shared_agent_profile() {
    one(&BattleNet, battlenet, "wow");
}
#[test]
fn riot_shared_metadata_profile() {
    one(&RiotClient, riot, "league_of_legends");
}
#[cfg(feature = "sqlite")]
#[test]
fn gog_shared_galaxy_database_profile() {
    one(&GogGalaxy, gog, "1207658924");
}
#[cfg(feature = "sqlite")]
#[test]
fn itch_application_support_database_profile() {
    one(&Itch, itch, "mac-cave");
}
#[test]
fn steam_generated_app_shortcuts_only_enrich_the_steam_game() {
    let t = machine();
    let root = t.path().join("root");
    steam(&root);
    app(
        &root,
        "Users/fixture/Applications/Fixture Steam.app",
        "com.valvesoftware.steam.app.620",
        "run.sh",
    );
    app(
        &root,
        "Users/fixture/Applications/Orphan.app",
        "com.valvesoftware.steam.app.999999",
        "run.sh",
    );
    let env = HostEnv::fixture(t.path()).unwrap();
    let sources: Vec<Arc<dyn Source>> = vec![Arc::new(Steam), Arc::new(MacosApps)];
    let report = scan_with_sources(&env, &Config::default(), &Cancel::new(), &sources).unwrap();
    assert_eq!(report.groups.len(), 1, "{report:?}");
    assert_eq!(report.groups[0].entries.len(), 2);
    assert_eq!(
        report.groups[0].primary_entry().unwrap().source,
        SourceId::new("steam")
    );
    assert_eq!(
        report.groups[0].primary_entry().unwrap().ids,
        [ExternalId::Steam { app_id: 620 }]
    );
}
#[cfg(feature = "sqlite")]
#[test]
fn all_seven_macos_launcher_sources_share_one_host_profile() {
    let t = machine();
    let root = t.path().join("root");
    for setup in [steam, epic, heroic, battlenet, riot, gog, itch] {
        setup(&root);
    }
    let env = HostEnv::fixture(t.path()).unwrap();
    let sources: Vec<Arc<dyn Source>> = vec![
        Arc::new(Steam),
        Arc::new(EpicGamesLauncher),
        Arc::new(GogGalaxy),
        Arc::new(Heroic),
        Arc::new(BattleNet),
        Arc::new(RiotClient),
        Arc::new(Itch),
    ];
    let config = Config {
        sources: orchestrator::SourceSelection::only(sources.iter().map(|s| s.id())),
        ..Config::default()
    };
    let report = scan(&env, &config, &Cancel::new()).unwrap();
    assert_eq!(report.groups.len(), 7, "{report:?}");
    assert_eq!(report.sources.len(), 7);
    assert!(report.sources.iter().all(|s| s.error.is_none()));
    assert!(
        report
            .groups
            .iter()
            .all(|g| g.primary_entry().unwrap().runtime == GameRuntime::Native)
    );
}
#[cfg(unix)]
#[test]
fn launcher_bundle_executable_cannot_escape_bundle() {
    let t = machine();
    let root = t.path().join("root");
    epic(&root);
    let exe = root.join("Applications/Epic Games Launcher.app/Contents/MacOS/EpicLauncherMac");
    fs::remove_file(&exe).unwrap();
    write(&root, "Outside", b"inert outsidebundle");
    std::os::unix::fs::symlink(root.join("Outside"), exe).unwrap();
    let env = HostEnv::fixture(t.path()).unwrap();
    assert_eq!(EpicGamesLauncher.detect(&env)[0].path, None);
}

#[cfg(feature = "sqlite")]
#[test]
fn two_gog_products_sharing_one_macos_galaxy_client_never_merge() {
    let t = machine();
    let root = t.path().join("root");
    gog(&root);
    rusqlite::Connection::open(root.join("Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db")).unwrap().execute_batch("INSERT INTO InstalledBaseProducts VALUES(1207659000,'/Users/fixture/Games/OtherGog');INSERT INTO LimitedDetails VALUES(1207659000,'Another GOG Game');").unwrap();
    write(&root, "Users/fixture/Games/OtherGog/game", b"inert game");
    let env = HostEnv::fixture(t.path()).unwrap();
    let report = scan_with_sources(
        &env,
        &Config::default(),
        &Cancel::new(),
        &[Arc::new(GogGalaxy)],
    )
    .unwrap();
    assert_eq!(report.groups.len(), 2, "{report:?}");
}
#[test]
fn two_riot_products_sharing_one_client_never_merge() {
    let t = machine();
    let root = t.path().join("root");
    riot(&root);
    write(
        &root,
        "Users/Shared/Riot Games/Metadata/fixture.live/fixture.live.product_settings.yaml",
        "product_install_full_path: /Users/fixture/Games/AnotherRiot\nshortcut_name: Another Riot Game\n",
    );
    write(&root, "Users/fixture/Games/AnotherRiot/game", b"inert game");
    let env = HostEnv::fixture(t.path()).unwrap();
    let report = scan_with_sources(
        &env,
        &Config::default(),
        &Cancel::new(),
        &[Arc::new(RiotClient)],
    )
    .unwrap();
    assert_eq!(report.groups.len(), 2, "{report:?}");
}
