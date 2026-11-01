#![cfg(feature = "windows-formats")]
use caskade_installer::{
    ArtifactNode, Fidelity, Strategy,
    exec::{CancellationToken, ExecRequest, Executor},
    formats::windows::inno::{
        InnoExtractor, InnoHandler, InnoLimits, InnoProbe, inspect_path, metadata_from_artifact,
    },
    host::HostCaps,
    plan::{PlanEngine, Policy},
    probe::ProbeEngine,
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use crate::fixtures;

fn corpus() -> Vec<fixtures::Fixture> {
    fixtures::load_windows(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
        .into_iter()
        .filter(|fixture| fixture.expected.format == "inno")
        .collect()
}
fn probe(path: &Path) -> ArtifactNode {
    let mut probe = ProbeEngine::default();
    probe.register(Box::new(InnoProbe::new()));
    let node = probe.probe_path(path).unwrap().root;
    assert_eq!(
        node.format,
        caskade_installer::Format::Inno,
        "{}",
        path.display()
    );
    assert!(node.facts.contains_key("pe.metadata"));
    node
}
fn host(name: &str) -> HostCaps {
    toml::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/hosts")
                .join(format!("{name}.toml")),
        )
        .unwrap(),
    )
    .unwrap()
}
fn planner() -> PlanEngine {
    let mut planner = PlanEngine::new();
    planner.register(Box::new(InnoHandler::new())).unwrap();
    planner
}

#[test]
fn eighteen_real_installers_have_metadata_and_six_host_plan_snapshots() {
    let fixtures = corpus();
    assert_eq!(fixtures.len(), 18);
    let mut snapshot = BTreeMap::new();
    for fixture in fixtures {
        let node = probe(&fixture.input());
        let metadata = metadata_from_artifact(&node).unwrap();
        assert_eq!(metadata.app_name.as_deref(), Some("Caskade Fixture Game"));
        assert_eq!(metadata.app_version.as_deref(), Some("1.0"));
        let expected = fixture.expected.windows.as_ref().unwrap();
        assert_eq!(metadata.password_required, expected.password.is_some());
        assert_eq!(
            metadata.has_code,
            expected.features.iter().any(|f| f == "CODE")
        );
        assert_eq!(
            !metadata.registry.is_empty(),
            expected.features.iter().any(|f| f == "REGISTRY")
        );
        assert_eq!(
            !metadata.ini.is_empty(),
            expected.features.iter().any(|f| f == "INI")
        );
        assert_eq!(
            metadata.external_slices,
            expected.features.iter().any(|f| f == "SLICED")
        );
        assert_eq!(
            serde_json::from_value::<caskade_installer::formats::windows::inno::InnoMetadata>(
                serde_json::to_value(&metadata).unwrap()
            )
            .unwrap(),
            metadata
        );
        let mut plans = BTreeMap::new();
        for profile in [
            "windows",
            "linux",
            "linux-no-umu",
            "linux-exfat-root",
            "macos",
            "macos-no-wine",
        ] {
            let plan = planner()
                .plan(
                    std::slice::from_ref(&node),
                    &host(profile),
                    &Policy::default(),
                )
                .unwrap();
            if metadata.password_required {
                assert!(plan.candidates.is_empty());
                assert!(plan.selected.is_none());
            } else {
                let selected = &plan.candidates[plan.selected.unwrap() as usize];
                assert_eq!(
                    selected.strategy,
                    if profile == "windows" {
                        Strategy::RunSilent
                    } else {
                        Strategy::Extract
                    }
                );
                if selected.strategy == Strategy::Extract {
                    assert_eq!(
                        selected.fidelity,
                        if metadata.is_partial() {
                            Fidelity::Partial
                        } else {
                            Fidelity::Exact
                        }
                    );
                }
            }
            plans.insert(profile, plan);
        }
        snapshot.insert(
            fixture.expected.id,
            serde_json::json!({"metadata": metadata, "plans": plans}),
        );
    }
    insta::assert_json_snapshot!("inno_matrix", snapshot);
}

#[tokio::test]
async fn every_plain_inno_fixture_extracts_exact_goldens_and_reprobes_through_executor() {
    let mut count = 0;
    for fixture in corpus().into_iter().filter(|fixture| {
        fixture
            .expected
            .windows
            .as_ref()
            .unwrap()
            .password
            .is_none()
    }) {
        let root = TestRoot::new().unwrap();
        let node = probe(&fixture.input());
        let sources = BTreeMap::from([(node.id.clone(), fixture.input())]);
        let extractor = Arc::new(InnoExtractor::new(sources.clone()));
        let worker = Executor::new(
            Arc::new(FakeRuntime::default()),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .with_extractor(extractor.clone());
        let plans = planner()
            .plan(
                std::slice::from_ref(&node),
                &host("linux"),
                &Policy::default(),
            )
            .unwrap();
        let report = worker
            .execute(
                ExecRequest {
                    job_id: fixture.expected.id.clone(),
                    root: root.path().into(),
                    relative_dir: "Inno Game".into(),
                    plans,
                    sources,
                },
                CancellationToken::new(),
            )
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", fixture.expected.id));
        fixture
            .assert_tree(&report.dir)
            .unwrap_or_else(|error| panic!("{}: {error}", fixture.expected.id));
        let nodes = extractor.take_artifacts(&node.id).unwrap();
        assert!(
            nodes
                .iter()
                .any(|node| node.format == caskade_installer::Format::Pe
                    && node.path == Path::new("FixtureGame.exe"))
        );
        let hints = extractor.take_discovery_hints(&node.id).unwrap();
        let discovered =
            caskade_installer::discover::discover(&report.dir, &host("linux"), &hints).unwrap();
        assert_eq!(
            discovered.launch_targets[0].path,
            PathBuf::from("FixtureGame.exe")
        );
        if fixture.expected.id.starts_with("gog") {
            assert_eq!(hints.ids.get("gog").map(String::as_str), Some("1234567890"));
            assert_eq!(
                discovered.ids.get("gog").map(String::as_str),
                Some("1234567890")
            );
        }
        fs::remove_dir_all(&report.dir).unwrap();
        root.assert_clean();
        count += 1;
    }
    assert_eq!(count, 15);
}

#[test]
fn direct_metadata_inspection_works_for_all_versions_and_encrypted_fallback() {
    for fixture in corpus() {
        let metadata = inspect_path(&fixture.input(), InnoLimits::default()).unwrap();
        if fixture.expected.id == "inno64-password" {
            assert_eq!(metadata.parser, "innospect/0.2.0");
        } else {
            assert_eq!(metadata.parser, "inno/0.7.1");
        }
    }
}

#[test]
fn native_presets_and_policy_use_typed_steps_and_the_executor_directory_placeholder() {
    use caskade_installer::{
        Step,
        exec::INSTALL_DIR,
        plan::{ExecutionPolicy, WindowsMethod},
    };
    let node = probe(
        &corpus()
            .into_iter()
            .find(|fixture| fixture.expected.id == "inno60-basic")
            .unwrap()
            .input(),
    );
    let mut policy = Policy::default();
    policy.preset_args.language = Some("en".into());
    let plan = planner()
        .plan(std::slice::from_ref(&node), &host("windows"), &policy)
        .unwrap();
    let silent = plan
        .candidates
        .iter()
        .find(|candidate| candidate.strategy == Strategy::RunSilent)
        .unwrap();
    let Step::Run { args, expect, .. } = &silent.steps[0] else {
        panic!("RunSilent must use Run")
    };
    for flag in [
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/SP-",
        "/NOICONS",
        "/LANG=en",
    ] {
        assert!(args.iter().any(|arg| arg == flag), "missing {flag}");
    }
    assert!(args.iter().any(|arg| arg.contains(INSTALL_DIR)));
    assert!(!expect.is_empty());
    let interactive = plan
        .candidates
        .iter()
        .find(|candidate| candidate.strategy == Strategy::RunInteractive)
        .unwrap();
    let Step::RunInteractive { args, .. } = &interactive.steps[0] else {
        panic!("interactive typed step")
    };
    assert!(!args.iter().any(|arg| arg == "/VERYSILENT"));
    assert!(args.iter().any(|arg| arg.contains(INSTALL_DIR)));
    policy.execution = ExecutionPolicy::Never;
    let extraction_only = planner()
        .plan(std::slice::from_ref(&node), &host("windows"), &policy)
        .unwrap();
    assert!(
        extraction_only
            .candidates
            .iter()
            .all(|candidate| candidate.strategy == Strategy::Extract)
    );
    policy.execution = ExecutionPolicy::Allow;
    policy.windows_method = WindowsMethod::Extract;
    let plan = planner()
        .plan(std::slice::from_ref(&node), &host("windows"), &policy)
        .unwrap();
    assert_eq!(
        plan.candidates[plan.selected.unwrap() as usize].strategy,
        Strategy::Extract
    );
    let mut unknown = node;
    unknown.facts.remove("inno.metadata");
    let unsupported = planner()
        .plan(&[unknown], &host("windows"), &policy)
        .unwrap();
    assert!(unsupported.candidates.is_empty());
}
