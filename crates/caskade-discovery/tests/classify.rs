use caskade_discovery::{
    ClassifierConfig, Evidence,
    classify::{self, Metadata, Verdict},
};
use serde::Deserialize;
use std::path::PathBuf;
#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    kind: String,
    label: String,
    metadata: Metadata,
    files: Vec<String>,
    expected: Verdict,
    prohibited: bool,
}
#[test]
fn labelled_uninstall_and_folder_corpus_reports_precision_and_recall() {
    let corpus: Corpus = serde_json::from_str(include_str!("classify_corpus.json")).unwrap();
    assert!(corpus.cases.len() >= 200);
    assert!(
        corpus
            .cases
            .iter()
            .filter(|c| c.kind == "uninstall")
            .count()
            >= 100
    );
    assert!(corpus.cases.iter().filter(|c| c.kind == "folder").count() >= 100);
    let (mut true_positive, mut false_positive, mut games, mut reviewed) =
        (0usize, 0usize, 0usize, 0usize);
    let mut results = Vec::new();
    for case in corpus.cases {
        let result =
            classify::classify_inventory(&case.metadata, &case.files, &ClassifierConfig::default())
                .unwrap();
        assert_eq!(
            result.verdict, case.expected,
            "{}: score {}, evidence {:?}",
            case.id, result.score, result.evidence
        );
        if case.label == "game" {
            games += 1;
        }
        if result.verdict == Verdict::Likely {
            if case.label == "game" {
                true_positive += 1;
            } else {
                false_positive += 1;
            }
        }
        if case.label == "game" && result.verdict != Verdict::NotAGame {
            reviewed += 1;
        }
        if case.prohibited {
            assert_ne!(
                result.verdict,
                Verdict::Likely,
                "redistributable/driver/SDK: {}",
                case.id
            );
        }
        results.push(serde_json::json!({"id":case.id,"label":case.label,"score":result.score,"verdict":result.verdict}));
    }
    let precision = true_positive as f64 / (true_positive + false_positive).max(1) as f64;
    let recall = true_positive as f64 / games as f64;
    eprintln!(
        "classifier corpus: {} rows; likely precision={precision:.3}; likely recall={recall:.3}; including review recall={:.3}",
        results.len(),
        reviewed as f64 / games as f64
    );
    assert!(precision >= 0.95);
    insta::assert_json_snapshot!("labelled_corpus", results);
}
#[test]
fn weak_signals_and_multiple_sdks_cannot_promote_a_utility_to_likely() {
    let metadata = Metadata {
        publisher: Some("Valve".into()),
        ..Default::default()
    };
    let files = vec![
        "steam_api.dll",
        "steam_api64.dll",
        "EOSSDK-Win64-Shipping.dll",
        "Galaxy64.dll",
        "SDL2.dll",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let result =
        classify::classify_inventory(&metadata, &files, &ClassifierConfig::default()).unwrap();
    assert_eq!(result.score, 58);
    assert_eq!(result.verdict, Verdict::Maybe);
}
#[test]
fn conjunctions_and_category_caps_are_explainable() {
    let config = ClassifierConfig::default();
    let files = vec!["UnityPlayer.dll".to_owned()];
    assert_eq!(
        classify::classify_inventory(&Metadata::default(), &files, &config)
            .unwrap()
            .verdict,
        Verdict::NotAGame
    );
    let files = vec![
        "UnityPlayer.dll".into(),
        "Demo_Data/globalgamemanagers".into(),
        "steam_api64.dll".into(),
    ];
    let metadata = Metadata {
        title: Some("Windows SDK".into()),
        category: Some("Game".into()),
        ..Default::default()
    };
    let result = classify::classify_inventory(&metadata, &files, &config).unwrap();
    assert_eq!(result.verdict, Verdict::NotAGame);
    assert!(
        result
            .evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule {rule,..} if rule=="category.sdk"))
    );
    assert!(
        result
            .evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule {rule,..} if rule=="safety.category_cap"))
    );
}
#[test]
fn game_redist_subdirectories_do_not_exclude_the_parent_game() {
    let metadata = Metadata {
        title: Some("Original Game".into()),
        install_dir: Some(PathBuf::from("C:/Games/Original")),
        ..Default::default()
    };
    let files = vec![
        "UnityPlayer.dll".into(),
        "Game_Data/globalgamemanagers".into(),
        "_CommonRedist/DirectX/DXSETUP.exe".into(),
    ];
    assert_eq!(
        classify::classify_inventory(&metadata, &files, &ClassifierConfig::default())
            .unwrap()
            .verdict,
        Verdict::Likely
    );
}
#[test]
fn thresholds_budgets_and_known_folder_boundaries_are_explicit() {
    let metadata = Metadata {
        publisher: Some("Valve".into()),
        install_dir: Some(PathBuf::from("C:/GamesFake/Utility")),
        known_game_folders: vec![PathBuf::from("C:/Games")],
        ..Default::default()
    };
    assert_eq!(
        classify::classify_inventory(&metadata, &[], &ClassifierConfig::default())
            .unwrap()
            .score,
        15
    );
    let config = ClassifierConfig {
        likely_score: 10,
        maybe_score: 5,
        ..Default::default()
    };
    assert_eq!(
        classify::classify_inventory(&metadata, &[], &config)
            .unwrap()
            .verdict,
        Verdict::Likely
    );
    let invalid = ClassifierConfig {
        likely_score: 5,
        maybe_score: 10,
        ..Default::default()
    };
    assert!(classify::classify_inventory(&metadata, &[], &invalid).is_err());
    assert!(
        classify::classify_inventory(&metadata, &["../UnityPlayer.dll".into()], &config).is_err()
    );
}

#[test]
fn game_category_through_client_and_game_named_driver_are_preserved() {
    let metadata = Metadata {
        title: Some("Portal".into()),
        category: Some("Game".into()),
        executable: Some(PathBuf::from("steam.exe")),
        ..Default::default()
    };
    let result =
        classify::classify_inventory(&metadata, &[], &ClassifierConfig::default()).unwrap();
    assert_eq!(result.verdict, Verdict::Likely);
    assert!(
        !result
            .evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule{rule,..}if rule=="category.launcher"))
    );
    let metadata = Metadata {
        title: Some("Driver: Parallel Lines".into()),
        publisher: Some("Ubisoft".into()),
        category: Some("Game".into()),
        ..Default::default()
    };
    assert_eq!(
        classify::classify_inventory(&metadata, &[], &ClassifierConfig::default())
            .unwrap()
            .verdict,
        Verdict::Likely
    );
}
#[test]
fn unix_game_folder_matching_preserves_case() {
    let metadata = Metadata {
        install_dir: Some(PathBuf::from("/home/user/Games/Original")),
        known_game_folders: vec![PathBuf::from("/home/user/games")],
        ..Default::default()
    };
    assert_eq!(
        classify::classify_inventory(&metadata, &[], &ClassifierConfig::default())
            .unwrap()
            .score,
        0
    );
}

#[test]
fn windows_verbatim_and_unc_game_folder_paths_compare_without_prefix_mismatch() {
    for (path, parent) in [
        (r"\\?\C:\Games\Original", r"c:\games"),
        (
            r"\\?\UNC\Server\Share\Games\Original",
            r"\\server\share\games",
        ),
    ] {
        let metadata = Metadata {
            install_dir: Some(PathBuf::from(path)),
            known_game_folders: vec![PathBuf::from(parent)],
            ..Default::default()
        };
        assert_eq!(
            classify::classify_inventory(&metadata, &[], &ClassifierConfig::default())
                .unwrap()
                .score,
            10
        );
    }
}
