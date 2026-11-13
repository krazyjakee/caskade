#![cfg(feature = "windows-formats")]
use crate::fixtures;
use caskade_installer::{
    events::{EventSink, JobEvent},
    exec::{CancellationToken, ExecRequest, Executor},
    formats::windows::msi::{
        MsiError, MsiExtractor, MsiHandler, MsiLimits, MsiProbe, inspect_reader,
        metadata_from_artifact, uninstall_steps,
    },
    plan::PlanEngine,
    probe::ProbeEngine,
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
    *,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
fn corpus() -> Vec<fixtures::Fixture> {
    fixtures::load_windows(&root())
        .unwrap()
        .into_iter()
        .filter(|f| f.expected.id.starts_with("msi-"))
        .collect()
}
fn host(name: &str) -> host::HostCaps {
    caskade_installer::pipeline::input::host(Some(name)).unwrap()
}
fn probe(path: &Path) -> ArtifactNode {
    let mut probe = ProbeEngine::default();
    probe.register(Box::new(MsiProbe));
    probe.probe_path(path).unwrap().root
}
fn plan(node: &ArtifactNode, name: &str) -> PlanSet {
    let mut engine = PlanEngine::new();
    engine.register(Box::new(MsiHandler)).unwrap();
    engine
        .plan(
            std::slice::from_ref(node),
            &host(name),
            &plan::Policy::default(),
        )
        .unwrap()
}
fn job(root: &TestRoot, node: &ArtifactNode, path: &Path, plans: PlanSet) -> ExecRequest {
    ExecRequest {
        root: root.path().into(),
        job_id: "msi-test".into(),
        relative_dir: "Fixture Game".into(),
        plans,
        sources: BTreeMap::from([(node.id.clone(), path.into())]),
    }
}
fn worker(path: &Path, node: &ArtifactNode, sink: Arc<dyn EventSink>) -> Executor {
    Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink,
    )
    .with_extractor(Arc::new(MsiExtractor::new(BTreeMap::from([(
        node.id.clone(),
        path.into(),
    )]))))
}

#[test]
fn metadata_and_host_strategy_matrix() {
    let corpus = corpus();
    assert_eq!(corpus.len(), 3);
    let mut snapshots = BTreeMap::new();
    for fixture in corpus {
        let node = probe(&fixture.input());
        assert_eq!(node.format, Format::Msi);
        let metadata = metadata_from_artifact(&node).unwrap();
        assert_eq!(metadata.files.len(), 4);
        assert_eq!(metadata.is_partial(), fixture.expected.id == "msi-plus-cab");
        assert_eq!(uninstall_steps(&node).len(), 1);
        let mut hosts = BTreeMap::new();
        for name in [
            "windows",
            "linux",
            "linux-no-umu",
            "linux-exfat-root",
            "macos",
            "macos-no-wine",
        ] {
            let plans = plan(&node, name);
            assert!(plans.question.is_none());
            assert_eq!(
                plans.candidates[0].strategy,
                if name == "windows" {
                    Strategy::RunSilent
                } else {
                    Strategy::Extract
                }
            );
            hosts.insert(name, plans);
        }
        snapshots.insert(fixture.expected.id, (metadata, hosts));
    }
    insta::assert_json_snapshot!("metadata_and_host_plans", snapshots);
}

#[tokio::test]
async fn embedded_external_and_custom_action_cabinets_match_exact_golden_trees() {
    for fixture in corpus() {
        let root = TestRoot::new().unwrap();
        let path = fixture.input();
        let node = probe(&path);
        let report = worker(&path, &node, Arc::new(RecordingSink::default()))
            .execute(
                job(&root, &node, &path, plan(&node, "linux")),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        fixture.assert_tree(&report.dir).unwrap();
        assert_eq!(
            report.candidate.fidelity,
            if fixture.expected.id == "msi-plus-cab" {
                Fidelity::Partial
            } else {
                Fidelity::Exact
            }
        );
        fs::remove_dir_all(report.dir).unwrap();
        root.assert_clean();
    }
}

struct CancelAt {
    index: usize,
    count: AtomicUsize,
    token: CancellationToken,
}
impl EventSink for CancelAt {
    fn emit(&self, _: JobEvent) {
        if self.count.fetch_add(1, Ordering::SeqCst) == self.index {
            self.token.cancel();
        }
    }
}
#[tokio::test]
async fn cancellation_at_each_msi_event_cleans_staging() {
    let fixture = corpus()
        .into_iter()
        .find(|f| f.expected.id == "msi-embedded")
        .unwrap();
    let path = fixture.input();
    let node = probe(&path);
    let root = TestRoot::new().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let report = worker(&path, &node, sink.clone())
        .execute(
            job(&root, &node, &path, plan(&node, "linux")),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    fs::remove_dir_all(report.dir).unwrap();
    assert!(sink.events().len() > 15);
    for index in 0..sink.events().len() {
        let token = CancellationToken::new();
        let sink = Arc::new(CancelAt {
            index,
            count: AtomicUsize::new(0),
            token: token.clone(),
        });
        assert!(
            matches!(
                worker(&path, &node, sink)
                    .execute(job(&root, &node, &path, plan(&node, "linux")), token)
                    .await,
                Err(exec::ExecError::Cancelled)
            ),
            "event {index}"
        );
        root.assert_clean();
    }
}

#[tokio::test]
async fn native_silent_interactive_and_product_code_uninstall_have_exact_argv() {
    let fixture = corpus().remove(0);
    let node = probe(&fixture.input());
    for strategy in [Strategy::RunSilent, Strategy::RunInteractive] {
        let mut plans = plan(&node, "windows");
        let candidate = plans
            .candidates
            .iter()
            .find(|c| c.strategy == strategy)
            .unwrap()
            .clone();
        plans.candidates = vec![candidate];
        plans.selected = Some(0);
        let root = TestRoot::new().unwrap();
        // Absolute on every host; the fake runtime never opens it.
        let source = std::env::temp_dir().join("picked").join("空白 setup.msi");
        let runtime = Arc::new(FakeRuntime::new([
            FakeScript {
                actions: vec![
                    FakeAction::WriteFile {
                        path: "FixtureGame.exe".into(),
                        contents: b"fake".to_vec(),
                    },
                    FakeAction::Exit { code: 0 },
                ],
            },
            FakeScript {
                actions: vec![FakeAction::Exit { code: 0 }],
            },
        ]));
        let executor = Executor::new(
            runtime.clone(),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        );
        let report = executor
            .execute(job(&root, &node, &source, plans), CancellationToken::new())
            .await
            .unwrap();
        let mut expected = vec!["/i".to_owned(), source.to_str().unwrap().to_owned()];
        if strategy == Strategy::RunSilent {
            expected.push("/qn".into());
        }
        expected.extend([
            "/norestart".to_owned(),
            format!("INSTALLDIR={}", report.dir.display()),
            format!("TARGETDIR={}", report.dir.display()),
        ]);
        assert_eq!(runtime.processes()[0].command.program, Path::new("msiexec"));
        assert_eq!(runtime.processes()[0].command.args, expected);
        executor
            .execute_steps_in_place(
                root.path(),
                &report.relative_dir,
                "uninstall-msi",
                uninstall_steps(&node),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let code = metadata_from_artifact(&node).unwrap().properties["ProductCode"].clone();
        assert_eq!(
            runtime.processes()[1].command.args,
            vec!["/x", &code, "/qn", "/norestart"]
        );
        fs::remove_dir_all(report.dir).unwrap();
        root.assert_clean();
    }
}

#[test]
fn hostile_string_pool_length_is_rejected_before_upstream_allocation() {
    let fixture = corpus().remove(0);
    let bytes = fs::read(fixture.input()).unwrap();
    let mut compound = cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
    let name = "\u{4840}\u{3f3f}\u{4577}\u{446c}\u{3e6a}\u{44b2}\u{482f}";
    {
        let mut stream = compound.open_stream(name).unwrap();
        stream.seek(SeekFrom::Start(4)).unwrap();
        stream.write_all(&[0, 0, 1, 0, 255, 255, 255, 255]).unwrap();
    }
    let bytes = compound.into_inner().into_inner();
    assert!(matches!(
        inspect_reader(Cursor::new(bytes), MsiLimits::default()),
        Err(MsiError::Limit)
    ));
}
#[test]
fn metadata_budgets_and_truncations_are_bounded() {
    let fixture = corpus().remove(0);
    let bytes = fs::read(fixture.input()).unwrap();
    for limits in [
        MsiLimits {
            max_read_bytes: 512,
            ..Default::default()
        },
        MsiLimits {
            max_rows: 1,
            ..Default::default()
        },
        MsiLimits {
            max_metadata_bytes: 256,
            ..Default::default()
        },
        MsiLimits {
            max_input_bytes: 512,
            ..Default::default()
        },
    ] {
        assert!(inspect_reader(Cursor::new(&bytes), limits).is_err());
    }
    for length in [0, 1, 8, 30, 64, 511, 512, 1024, bytes.len() / 2] {
        assert!(inspect_reader(Cursor::new(&bytes[..length]), MsiLimits::default()).is_err());
    }
    let mut state = 17_u64;
    for _ in 0..64 {
        let mut data = bytes.clone();
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let offset = (state as usize) % 512;
        data[offset] ^= (state >> 32) as u8;
        let _ = inspect_reader(Cursor::new(data), MsiLimits::default());
    }
}

#[tokio::test]
async fn missing_external_cabinet_is_an_explicit_clean_failure() {
    let fixture = corpus()
        .into_iter()
        .find(|f| f.expected.id == "msi-external")
        .unwrap();
    let source = tempfile::tempdir().unwrap();
    let path = source.path().join("setup.msi");
    fs::copy(fixture.input(), &path).unwrap();
    let node = probe(&path);
    let root = TestRoot::new().unwrap();
    let mut plans = plan(&node, "linux");
    plans.candidates.retain(|c| c.strategy == Strategy::Extract);
    assert!(
        worker(&path, &node, Arc::new(RecordingSink::default()))
            .execute(job(&root, &node, &path, plans), CancellationToken::new())
            .await
            .is_err()
    );
    root.assert_clean();
}

#[test]
fn recorded_uninstall_uses_only_the_executed_artifact_and_preserves_runtime() {
    let fixture = corpus().remove(0);
    let node = probe(&fixture.input());
    let plans = plan(&node, "linux");
    let helper = caskade_installer::formats::windows::uninstall_steps;
    assert!(helper(std::slice::from_ref(&node), &plans.candidates[0]).is_empty());
    let native = plans
        .candidates
        .iter()
        .find(|c| c.strategy == Strategy::RunSilent)
        .unwrap();
    let steps = helper(std::slice::from_ref(&node), native);
    assert!(matches!(
        &steps[0],
        Step::Run {
            runtime: RuntimeSpec::ProtonPrefix { .. },
            ..
        }
    ));
    let mut sibling = node.clone();
    sibling.id = ArtifactId("sibling".into());
    assert_eq!(helper(&[node, sibling], native).len(), 1);
}

fn modified(table: &str, column: &str, key: &str, id: &str, value: &str) -> Vec<u8> {
    let fixture = corpus().remove(0);
    let bytes = fs::read(fixture.input()).unwrap();
    let mut package = msi::Package::open(Cursor::new(bytes)).unwrap();
    package
        .update_rows(
            msi::Update::table(table)
                .set(column, msi::Value::from(value))
                .with(msi::Expr::col(key).eq(msi::Expr::string(id))),
        )
        .unwrap();
    package.into_inner().unwrap().into_inner()
}
#[test]
fn hostile_directory_media_and_product_code_tables_are_rejected() {
    for (table, column, key, id, value) in [
        ("File", "FileName", "File", "F0", "../escape.exe"),
        ("File", "FileName", "File", "F0", "C:escape.exe"),
        ("Directory", "Directory_Parent", "Directory", "D1", "D1"),
        (
            "Property",
            "Value",
            "Property",
            "ProductCode",
            "{not-a-guid}",
        ),
    ] {
        assert!(
            inspect_reader(
                Cursor::new(modified(table, column, key, id, value)),
                MsiLimits::default()
            )
            .is_err(),
            "{table}/{column}"
        );
    }
    let bytes = fs::read(corpus().remove(0).input()).unwrap();
    let mut package = msi::Package::open(Cursor::new(bytes)).unwrap();
    package
        .update_rows(msi::Update::table("Media").set("Cabinet", msi::Value::from("../outside.cab")))
        .unwrap();
    assert!(inspect_reader(package.into_inner().unwrap(), MsiLimits::default()).is_err());
}

#[test]
fn fuzzed_null_bootstrap_cells_fail_before_upstream_can_panic() {
    let bytes = include_bytes!("regressions/28-msi-columns-unwrap.msi");
    assert!(matches!(
        inspect_reader(Cursor::new(bytes), MsiLimits::default()),
        Err(MsiError::Invalid(_))
    ));
}

#[test]
fn all_bootstrap_tables_reject_null_required_string_cells() {
    for name in [
        "\u{4840}\u{3f7f}\u{4164}\u{422f}\u{4836}",
        "\u{4840}\u{3b3f}\u{43f2}\u{4438}\u{45b1}",
        "\u{4840}\u{3fff}\u{43e4}\u{41ec}\u{45e4}\u{44ac}\u{4831}",
    ] {
        let mut compound =
            cfb::CompoundFile::open(Cursor::new(fs::read(corpus().remove(0).input()).unwrap()))
                .unwrap();
        if !compound.is_stream(name) {
            compound
                .create_stream(name)
                .unwrap()
                .write_all(&[0; 24])
                .unwrap();
        }
        {
            let mut stream = compound.open_stream(name).unwrap();
            stream.write_all(&[0, 0]).unwrap();
        }
        let error = inspect_reader(compound.into_inner(), MsiLimits::default()).unwrap_err();
        assert!(
            error.to_string().contains("bootstrap table cell"),
            "{error}"
        );
    }
}
