//! Pure report checks and explicitly opted-in live aggregate evidence (#103).
use caskade_discovery::*;
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
};
#[path = "hardening_support/mod.rs"]
mod invariants;
#[test]
fn invariant_checker_detects_missing_paths_duplicate_ids_and_tool_groups() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("root")).unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    let mut one = DetectedGame::new(SourceId::new("test"), "one", "Original Game");
    one.ids.push(ExternalId::Steam {
        app_id: 100_000_000,
    });
    one.install_dir = Some(env.fs_root().join("absent"));
    let mut two = one.clone();
    two.key = "two".into();
    two.title = "Steam".into();
    let report = ScanReport {
        groups: vec![
            GameGroup {
                primary: 0,
                entries: vec![one],
            },
            GameGroup {
                primary: 0,
                entries: vec![two],
            },
        ],
        ..Default::default()
    };
    let counters = invariants::check(&env, &report);
    assert_eq!(counters.missing_certain_dirs, 2);
    assert_eq!(counters.duplicate_external_ids, 1);
    assert_eq!(counters.launcher_or_redist_groups, 1);
    assert!(!counters.healthy());
}
#[test]
#[ignore = "reads real launcher metadata; aggregate-only, requires CASKADE_DISCOVERY_LIVE=1"]
fn live_full_scan_obeys_discovery_invariants_without_exporting_userdata() {
    assert_eq!(
        std::env::var("CASKADE_DISCOVERY_LIVE").as_deref(),
        Ok("1"),
        "explicit live opt-in required"
    );
    let env = HostEnv::live();
    let started = std::time::Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| {
        scan(&env, &Config::default(), &Cancel::new())
    }));
    assert!(
        result.is_ok(),
        "discovery panicked; details intentionally suppressed"
    );
    let result = result.unwrap();
    assert!(
        result.is_ok(),
        "discovery failed; details intentionally suppressed"
    );
    let report = result.unwrap();
    let counters = invariants::check(&env, &report);
    let metric = serde_json::json!({"schema_version":1,"metric":"discovery_live_invariants","elapsed_ms":started.elapsed().as_millis(),"invariants":counters,"source_counts":report.sources.iter().map(|s|(s.source.to_string(),s.count)).collect::<BTreeMap<_,_>>()});
    println!("{metric}");
    if let Some(directory) = std::env::var_os("CASKADE_DISCOVERY_LIVE_OUTPUT") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("live-invariants.json"),
            serde_json::to_vec_pretty(&metric).unwrap(),
        )
        .unwrap();
    }
    assert!(
        counters.healthy(),
        "live invariant violation; only aggregate counters emitted"
    );
}
