//! #73 and #75 acceptance: Epic games reported by Heroic, the Legendary CLI
//! and BoilR-style Steam shortcuts end up in one group per game.

use std::{path::PathBuf, sync::Arc};

use caskade_discovery::{
    Cancel, Config, Evidence, ScanReport, Source, scan_with_sources,
    sources::{
        heroic::Heroic,
        legendary::Legendary,
        steam::{
            Steam,
            shortcuts::{PROXY_RULE, SteamShortcuts},
        },
    },
};

use crate::fixtures as support;

fn report() -> ScanReport {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let scenario = support::Scenario::load(&fixtures, "epic-linux").unwrap();
    let env = caskade_discovery::HostEnv::fixture(&scenario.path).unwrap();
    let sources: [Arc<dyn Source>; 4] = [
        Arc::new(Steam),
        Arc::new(Heroic),
        Arc::new(Legendary),
        Arc::new(SteamShortcuts),
    ];
    scan_with_sources(&env, &Config::default(), &Cancel::new(), &sources).unwrap()
}

/// `source:key` of every entry in the group holding `source:key`.
fn group_of(report: &ScanReport, source: &str, key: &str) -> Vec<String> {
    let group = report
        .groups
        .iter()
        .find(|group| {
            group
                .entries
                .iter()
                .any(|entry| entry.source.as_str() == source && entry.key == key)
        })
        .unwrap_or_else(|| panic!("no group with {source}:{key}"));
    let mut members: Vec<_> = group
        .entries
        .iter()
        .map(|entry| format!("{}:{}", entry.source, entry.key))
        .collect();
    members.sort();
    members
}

#[test]
fn heroic_legendary_and_shortcuts_merge_on_the_epic_app() {
    let report = report();
    // #75: a game in both Heroic and Legendary is one group; #73: the
    // BoilR/Heroic-Flatpak shortcut joins it instead of duplicating it.
    assert_eq!(
        group_of(&report, "heroic", "legendary:FixtureEpicApp"),
        [
            "heroic:legendary:FixtureEpicApp",
            "legendary:FixtureEpicApp",
            "steam.shortcuts:2294968293",
        ]
    );
    // `legendary launch <app>` shortcuts join the Legendary game.
    assert_eq!(
        group_of(&report, "legendary", "FixtureEpicTwo"),
        ["legendary:FixtureEpicTwo", "steam.shortcuts:2294968292"]
    );
    // The launcher's entry stays primary.
    let primary = report
        .groups
        .iter()
        .map(|group| group.primary_entry().unwrap())
        .find(|entry| entry.title == "Fixture Epic Quest")
        .unwrap();
    assert_eq!(primary.source.as_str(), "heroic");

    // A proxy whose launcher game is absent (steam://rungameid/620 here)
    // never becomes a game of its own.
    for group in &report.groups {
        let all_proxies = group.entries.iter().all(|entry| {
            entry.evidence.iter().any(
                |evidence| matches!(evidence, Evidence::Rule { rule, .. } if rule == PROXY_RULE),
            )
        });
        assert!(!all_proxies, "{:?}", group.entries[0].title);
    }
    // Ordinary shortcuts are still games.
    assert!(
        report
            .groups
            .iter()
            .any(|group| group.primary_entry().unwrap().title == "Fixture Indie")
    );
}
