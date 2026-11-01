//! #97: real-machine verification is opt-in. Default CI exercises the macOS
//! profile/bundle fixtures on Linux; this test never launches a game.
#![cfg(all(target_os = "macos", feature = "macos-sources"))]
use caskade_discovery::*;

#[test]
#[ignore = "requires an explicitly provisioned real Mac with Steam, Epic and an App Store game"]
fn real_mac_steam_epic_and_app_store_game_are_discovered() {
    assert_eq!(
        std::env::var("CASKADE_DISCOVERY_LIVE").as_deref(),
        Ok("1"),
        "explicit live permission required"
    );
    let env = HostEnv::live();
    let config = Config {
        sources: orchestrator::SourceSelection::only([
            SourceId::new("steam"),
            SourceId::new("epic"),
            SourceId::new("macos.apps"),
        ]),
        ..Config::default()
    };
    let report = match scan(&env, &config, &Cancel::new()) {
        Ok(report) => report,
        Err(_) => panic!("real Mac scan failed; inspect details locally"),
    };
    assert!(
        report.sources.iter().all(|s| s.error.is_none()),
        "a selected Mac source failed"
    );
    let installed: Vec<_> = report
        .groups
        .iter()
        .flat_map(|g| &g.entries)
        .filter(|game| game.state == InstallState::Installed && game.runtime == GameRuntime::Native)
        .collect();
    let steam = installed
        .iter()
        .filter(|g| g.source.as_str() == "steam")
        .count();
    let epic = installed
        .iter()
        .filter(|g| g.source.as_str() == "epic")
        .count();
    let receipts = installed
        .iter()
        .filter(|game| {
            game.source.as_str() == "macos.apps"
                && game.confidence == Confidence::Certain
                && game
                    .evidence
                    .iter()
                    .any(|e| matches!(e,Evidence::Rule{rule,..}if rule=="store.mac_app_store"))
        })
        .count();
    // Aggregate proof only: don't print titles, account ids, home paths or URIs.
    println!(
        "real_mac_discovery steam={steam} epic={epic} app_store_games={receipts} groups={}",
        report.groups.len()
    );
    assert!(
        steam > 0,
        "provision a native Steam game before running this check"
    );
    assert!(
        epic > 0,
        "provision a native Epic game before running this check"
    );
    assert!(
        receipts > 0,
        "provision a categorized Mac App Store game before running this check"
    );
}
