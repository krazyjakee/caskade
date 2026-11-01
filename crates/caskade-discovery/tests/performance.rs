#![cfg(all(
    feature = "windows-sources",
    feature = "linux-sources",
    feature = "macos-sources",
    feature = "sqlite"
))]
//! Strict release performance gate; metadata generation and build are untimed.
use caskade_discovery::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
#[path = "fixtures/support/mod.rs"]
mod fixtures;
#[path = "hardening_support/mod.rs"]
mod invariants;
#[path = "macos_support/mod.rs"]
mod macos_support;
#[path = "performance_support/mod.rs"]
mod workload;
#[test]
#[ignore = "generated1000-game release performance gate; run serially with --release"]
fn thousand_games_ten_libraries_and_all_launcher_profiles_finish_under_two_seconds() {
    if cfg!(debug_assertions) {
        panic!("this measured budget requires a release build");
    }
    let mut metrics = vec![];
    let mut covered = BTreeSet::new();
    for os in [
        OperatingSystem::Linux,
        OperatingSystem::Windows,
        OperatingSystem::Macos,
    ] {
        let fixture = workload::prepare(os);
        let config = Config::default();
        let started = Instant::now();
        let report = scan(&fixture.env, &config, &Cancel::new()).unwrap();
        let elapsed = started.elapsed();
        let invariant_counts = invariants::check(&fixture.env, &report);
        assert!(invariant_counts.healthy(), "{os:?}: {invariant_counts:?}");
        let mut generated = BTreeMap::<u32, usize>::new();
        for (index, group) in report.groups.iter().enumerate() {
            for id in group.entries.iter().flat_map(|game| &game.ids) {
                if let ExternalId::Steam { app_id } = id
                    && (workload::FIRST_APP..workload::FIRST_APP + workload::STEAM_GAMES)
                        .contains(app_id)
                {
                    assert!(
                        generated
                            .insert(*app_id, index)
                            .is_none_or(|old| old == index),
                        "generated Steam identity appears in two groups"
                    );
                }
            }
        }
        assert_eq!(
            generated.len(),
            workload::STEAM_GAMES as usize,
            "{os:?}: generated games were lost"
        );
        assert!(
            report.sources.iter().all(|s| s.error.is_none()),
            "{os:?}: {:?}",
            report.sources
        );
        for source in &report.sources {
            if source.count > 0 {
                covered.insert(source.source.to_string());
            }
        }
        let metric = serde_json::json!({"profile":format!("{os:?}"),"generated_steam_games":workload::STEAM_GAMES,"generated_steam_libraries":workload::LIBRARIES,"groups":report.groups.len(),"elapsed_ms":elapsed.as_secs_f64()*1000.0,"budget_ms":2000,"source_counts":report.sources.iter().map(|s|(s.source.to_string(),s.count)).collect::<BTreeMap<_,_>>()});
        println!("{metric}");
        metrics.push(metric);
        assert!(
            elapsed < Duration::from_secs(2),
            "{os:?}: full scan took {elapsed:?}, budget is strictly below2s"
        );
    }
    for source in [
        "steam",
        "steam.shortcuts",
        "heroic",
        "legendary",
        "lutris",
        "bottles",
        "minigalaxy",
        "faugus",
        "itch",
        "epic",
        "gog",
        "ea",
        "ubisoft",
        "battlenet",
        "amazon",
        "riot",
        "rockstar",
        "xbox",
    ] {
        assert!(
            covered.contains(source),
            "performance workload lacks launcher source {source}"
        );
    }
    if let Some(directory) = std::env::var_os("CASKADE_DISCOVERY_PERF_OUTPUT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("scan-1000.json"),serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"metric":"discovery1000","finished_unix":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),"commit":std::env::var("GITHUB_SHA").ok(),"profiles":metrics,"covered_sources":covered})).unwrap()).unwrap();
    }
}
