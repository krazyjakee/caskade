//! Opt-in D1 live evidence. Never prints game names, paths, ids or diagnostics.
#![cfg(all(target_os = "linux", feature = "linux-sources", feature = "sqlite"))]
use caskade_discovery::*;
use std::{collections::BTreeSet, time::Instant};

#[test]
#[ignore = "reads real launcher metadata; requires CASKADE_DISCOVERY_LIVE=1"]
fn live_linux_discovery_counts() {
    assert_eq!(
        std::env::var("CASKADE_DISCOVERY_LIVE").ok().as_deref(),
        Some("1"),
        "live scans require explicit opt-in"
    );
    let started = Instant::now();
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
    let result = scan(&HostEnv::live(), &config, &Cancel::new());
    assert!(
        result.is_ok(),
        "live scan failed; private diagnostics intentionally suppressed"
    );
    let report = result.unwrap();
    let safe = serde_json::json!({
        "elapsed_ms":started.elapsed().as_millis(),"groups":report.groups.len(),
        "launchers":report.launchers.iter().map(|launcher|serde_json::json!({"source":launcher.source,"found":launcher.found,"kind":match &launcher.kind{Some(LauncherKind::Native)=>Some("native"),Some(LauncherKind::Flatpak{..})=>Some("flatpak"),Some(LauncherKind::Snap{..})=>Some("snap"),Some(LauncherKind::Wine{..})=>Some("wine"),None=>None}})).collect::<Vec<_>>(),
        "sources":report.sources.iter().map(|source|serde_json::json!({"source":source.source,"duration_ms":source.duration_ms,"count":source.count,"warnings":source.warnings.len(),"failed":source.error.is_some()})).collect::<Vec<_>>()
    });
    eprintln!("{safe}");
    assert!(
        report.sources.iter().all(|s| s.error.is_none()),
        "a source failed; aggregate counters above contain no private diagnostics"
    );
    let mut ids = BTreeSet::new();
    for group in &report.groups {
        assert!(group.primary_entry().is_some(), "invalid empty group");
        let group_ids: BTreeSet<_> = group
            .entries
            .iter()
            .flat_map(|e| &e.ids)
            .filter_map(|id| match id {
                ExternalId::Steam { .. } | ExternalId::Gog { .. } => Some(id.to_string()),
                _ => None,
            })
            .collect();
        for id in group_ids {
            assert!(
                ids.insert(id),
                "duplicate global store identity across groups (value suppressed)"
            );
        }
    }
}
