#![cfg(all(feature = "windows-sources", feature = "sqlite"))]
//! D2's independently authored logical inventory and classifier confusion matrix.
use caskade_discovery::{
    sources::uninstall::{self, UninstallSource},
    *,
};
use std::{collections::BTreeSet, fs, path::PathBuf};
use crate::fixtures;
const SOURCES: [&str; 13] = [
    "steam",
    "uninstall",
    "startmenu",
    "gameux",
    "epic",
    "gog",
    "ea",
    "ubisoft",
    "battlenet",
    "amazon",
    "riot",
    "rockstar",
    "xbox",
];
fn machine(id: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/machines")
        .join(id)
}
#[test]
fn composite_windows_machine_finds_each_logical_game_once() {
    let root = machine("windows-integration");
    let env = HostEnv::fixture(&root).unwrap();
    let config = Config {
        sources: SourceSelection::only(SOURCES.into_iter().map(SourceId::new)),
        ..Config::default()
    };
    let mut report = scan(&env, &config, &Cancel::new()).unwrap();
    assert!(
        report.sources.iter().all(|s| s.error.is_none()),
        "{:?}",
        report.sources
    );
    let inventory: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("expected.json")).unwrap()).unwrap();
    let expected: BTreeSet<_> = inventory["titles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    let actual: BTreeSet<_> = report
        .groups
        .iter()
        .map(|g| g.primary_entry().unwrap().title.as_str())
        .collect();
    assert_eq!(report.groups.len(), expected.len(), "{actual:?}");
    assert_eq!(actual, expected);
    for id in SOURCES {
        assert!(
            report
                .sources
                .iter()
                .any(|s| s.source.as_str() == id && s.count > 0),
            "missing source {id}"
        );
    }
    for owner in ["steam", "epic", "ea", "ubisoft", "gog", "battlenet"] {
        assert!(
            report.groups.iter().any(|group| group
                .entries
                .iter()
                .any(|e| e.source.as_str() == owner)
                && group
                    .entries
                    .iter()
                    .any(|e| e.source.as_str() == "uninstall")),
            "missing merged uninstall alias for {owner}"
        );
    }
    for owner in [
        "steam",
        "epic",
        "ea",
        "ubisoft",
        "gog",
        "battlenet",
        "amazon",
        "riot",
        "rockstar",
    ] {
        assert!(
            report
                .launchers
                .iter()
                .any(|l| l.source.as_str() == owner && l.found),
            "missing launcher {owner}"
        );
    }
    let hollow: Vec<_> = report
        .groups
        .iter()
        .filter(|g| g.entries.iter().any(|e| e.title == "Hollow Knight"))
        .collect();
    assert_eq!(hollow.len(), 1);
    let owners: BTreeSet<_> = hollow[0]
        .entries
        .iter()
        .map(|e| e.source.as_str())
        .collect();
    assert_eq!(owners, BTreeSet::from(["uninstall", "gameux", "startmenu"]));
    for source in &mut report.sources {
        source.duration_ms = 0;
    }
    fixtures::assert_snapshot("d2_windows_integration", &report, &root);
}
#[test]
fn uninstall_corpus_precision_and_recall_exceed_d2_gate() {
    let root = machine("uninstall-windows");
    let env = HostEnv::fixture(&root).unwrap();
    let labels: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("records.json")).unwrap()).unwrap();
    let labels = labels["records"].as_array().unwrap();
    assert_eq!(labels.len(), 100);
    let games = UninstallSource::new()
        .scan(&env, &ScanCtx::default())
        .unwrap();
    // Alias records are routing inputs, explicitly labelled separately from
    // standalone classifier decisions; integration above tests their ownership.
    let predicted: BTreeSet<_> = games
        .iter()
        .filter(|g| {
            !g.evidence
                .iter()
                .any(|e| matches!(e, Evidence::Rule{rule,..} if rule == uninstall::ALIAS_RULE))
        })
        .map(|g| g.title.as_str())
        .collect();
    assert!(
        predicted
            .iter()
            .all(|title| labels.iter().any(|label| label["title"]
                .as_str()
                .or_else(|| label["values"]["DisplayName"].as_str())
                == Some(*title))),
        "unlabelled classifier output"
    );
    let (mut tp, mut fp, mut tn, mut fn_, mut aliases) = (0, 0, 0, 0, 0);
    for label in labels {
        if label["expect"] == "alias" {
            aliases += 1;
            continue;
        }
        match (
            label["expect"] == "game",
            label["title"]
                .as_str()
                .or_else(|| label["values"]["DisplayName"].as_str())
                .is_some_and(|title| predicted.contains(title)),
        ) {
            (true, true) => tp += 1,
            (false, true) => fp += 1,
            (false, false) => tn += 1,
            (true, false) => fn_ += 1,
        }
    }
    let precision = f64::from(tp) / f64::from(tp + fp);
    let recall = f64::from(tp) / f64::from(tp + fn_);
    println!(
        "{}",
        serde_json::json!({"metric":"d2_uninstall_classifier","total":labels.len(),"routing_aliases":aliases,"eligible":tp+fp+tn+fn_,"true_positive":tp,"false_positive":fp,"true_negative":tn,"false_negative":fn_,"precision":precision,"recall":recall})
    );
    assert_eq!((tp, fp, tn, fn_, aliases), (24, 0, 68, 0, 8));
    assert!(precision >= 0.95 && recall >= 0.95);
}
