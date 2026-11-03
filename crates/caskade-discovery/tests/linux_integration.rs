#![cfg(all(feature = "linux-sources", feature = "sqlite"))]
//! D1's authored inventory is independent of the scan and contains only
//! original synthetic metadata, including intentional duplicate interfaces.
use crate::fixtures;
use caskade_discovery::*;
use serde::Deserialize;
use std::{collections::BTreeSet, fs, path::PathBuf};
#[derive(Deserialize)]
struct Inventory {
    groups: Vec<Vec<String>>,
    excluded_desktop_keys: Vec<String>,
}
#[test]
fn composite_linux_machine_finds_every_game_once_without_tools_or_runtimes() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/machines/linux-integration");
    let env = HostEnv::fixture(&root).unwrap();
    let config = Config {
        sources: SourceSelection::only(
            [
                "steam",
                "steam.shortcuts",
                "heroic",
                "legendary",
                "lutris",
                "desktop",
                "flatpak",
                "snap",
            ]
            .into_iter()
            .map(SourceId::new),
        ),
        ..Config::default()
    };
    let mut report = scan(&env, &config, &Cancel::new()).unwrap();
    assert!(
        report.sources.iter().all(|s| s.error.is_none()),
        "{report:?}"
    );
    assert!(
        report.sources.iter().all(|s| s.warnings.is_empty()),
        "{report:?}"
    );
    let inventory: Inventory =
        serde_json::from_slice(&fs::read(root.join("expected.json")).unwrap()).unwrap();
    let expected: BTreeSet<BTreeSet<String>> = inventory
        .groups
        .into_iter()
        .map(|g| g.into_iter().collect())
        .collect();
    let mut all = BTreeSet::new();
    let actual: BTreeSet<BTreeSet<String>> = report
        .groups
        .iter()
        .map(|group| {
            assert!(group.primary_entry().is_some());
            group
                .entries
                .iter()
                .map(|game| {
                    let key = format!("{}:{}", game.source, game.key);
                    assert!(all.insert(key.clone()), "duplicate source record: {key}");
                    assert!(
                        !inventory
                            .excluded_desktop_keys
                            .iter()
                            .any(|k| game.source.as_str() == "desktop" && game.key == *k)
                    );
                    key
                })
                .collect()
        })
        .collect();
    assert_eq!(report.groups.len(), 20);
    assert_eq!(actual, expected);
    // Both Steam installations and Flatpak Heroic are detected, even when
    // they share physical games with other launchers and desktop proxies.
    assert!(
        report
            .launchers
            .iter()
            .any(|l| l.source.as_str() == "steam" && matches!(l.kind, Some(LauncherKind::Native)))
    );
    assert!(
        report.launchers.iter().any(|l| l.source.as_str() == "steam"
            && matches!(l.kind, Some(LauncherKind::Flatpak { .. })))
    );
    assert!(
        report
            .launchers
            .iter()
            .any(|l| l.source.as_str() == "heroic"
                && matches!(l.kind, Some(LauncherKind::Flatpak { .. })))
    );
    assert!(
        report
            .sources
            .iter()
            .any(|s| s.source.as_str() == "steam" && s.count == 4)
    );
    // Source durations are measurements, not fixture data. Preserve all
    // counts, states, alternatives and evidence while stabilizing this field.
    for source in &mut report.sources {
        source.duration_ms = 0;
    }
    fixtures::assert_snapshot("d1_linux_integration", &report, &root);
}
