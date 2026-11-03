#![cfg(feature = "windows-formats")]
use crate::fixtures;
use caskade_installer::{
    events::{EventSink, JobEvent},
    exec::{CancellationToken, ExecRequest, Executor},
    formats::windows::nsis::{
        NsisExtractor, NsisHandler, NsisLimits, NsisProbe, app_path, inspect_bytes,
        metadata_from_artifact,
    },
    plan::PlanEngine,
    probe::ProbeEngine,
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
    *,
};
use std::{
    collections::BTreeMap,
    fs,
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
        .filter(|f| f.expected.format == "nsis")
        .collect()
}
fn host(name: &str) -> host::HostCaps {
    toml::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/hosts/{name}.toml")),
        )
        .unwrap(),
    )
    .unwrap()
}
fn probe(path: &Path) -> ArtifactNode {
    let mut probe = ProbeEngine::default();
    probe.register(Box::new(NsisProbe));
    let node = probe.probe_path(path).unwrap().root;
    assert_eq!(
        node.format,
        Format::Nsis,
        "{} {:?}",
        path.display(),
        node.facts
    );
    node
}
fn plan(node: &ArtifactNode, name: &str) -> PlanSet {
    let mut engine = PlanEngine::new();
    engine.register(Box::new(NsisHandler)).unwrap();
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
        job_id: "nsis-test".into(),
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
    .with_extractor(Arc::new(NsisExtractor::new(BTreeMap::from([(
        node.id.clone(),
        path.into(),
    )]))))
}

#[test]
fn six_variants_have_typed_paths_effects_and_six_host_snapshots() {
    let fixtures = corpus();
    assert_eq!(fixtures.len(), 6);
    let mut snapshots = BTreeMap::new();
    for fixture in fixtures {
        let node = probe(&fixture.input());
        let metadata = metadata_from_artifact(&node).unwrap();
        assert_eq!(metadata.files.len(), 4);
        assert_eq!(metadata.shortcuts.len(), 1);
        assert!(metadata.shortcuts[0].link.contains("SMPROGRAMS"));
        assert_eq!(
            metadata.shortcuts[0].path.as_deref(),
            Some(Path::new("FixtureGame.exe"))
        );
        assert!(metadata.is_partial());
        assert!(
            !metadata.custom_instructions.is_empty(),
            "INI write is omitted"
        );
        assert_eq!(
            metadata.version,
            if fixture.expected.id.starts_with("nsis251") {
                "V2"
            } else {
                "V3"
            }
        );
        assert_eq!(
            metadata.encoding,
            if fixture.expected.id.contains("unicode") || fixture.expected.id == "nsis-indie" {
                "Unicode"
            } else {
                "ANSI"
            }
        );
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
async fn every_nsis_variant_extracts_exact_goldens_and_suppresses_start_menu_writes() {
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
        assert_eq!(report.candidate.fidelity, Fidelity::Partial);
        assert!(!report.dir.join("config.ini").exists());
        assert!(
            !fixtures::snapshot_tree(&report.dir)
                .unwrap()
                .iter()
                .any(|entry| entry.path.ends_with(".lnk"))
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
async fn cancellation_at_every_event_in_solid_and_nonsolid_modes_is_clean() {
    for fixture in corpus().into_iter().filter(|f| {
        matches!(
            f.expected.id.as_str(),
            "nsis251-ansi-solid" | "nsis3-unicode-nonsolid"
        )
    }) {
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
                "{} event {index}",
                fixture.expected.id
            );
            root.assert_clean();
        }
    }
}
#[tokio::test]
async fn silent_and_interactive_argv_keep_the_unquoted_directory_last() {
    let fixture = corpus().remove(0);
    let node = probe(&fixture.input());
    for strategy in [Strategy::RunSilent, Strategy::RunInteractive] {
        let root = TestRoot::new().unwrap();
        let runtime = Arc::new(FakeRuntime::new([FakeScript {
            actions: vec![
                FakeAction::WriteFile {
                    path: "FixtureGame.exe".into(),
                    contents: b"fake".to_vec(),
                },
                FakeAction::Exit { code: 0 },
            ],
        }]));
        let mut plans = plan(&node, "windows");
        let candidate = plans
            .candidates
            .iter()
            .find(|c| c.strategy == strategy)
            .unwrap()
            .clone();
        plans.candidates = vec![candidate];
        plans.selected = Some(0);
        let mut request = job(&root, &node, Path::new("/picked/空白 setup.exe"), plans);
        request.relative_dir = "空白 Game Folder".into();
        let executor = Executor::new(
            runtime.clone(),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        );
        let report = executor
            .execute(request, CancellationToken::new())
            .await
            .unwrap();
        let mut args = vec![];
        if strategy == Strategy::RunSilent {
            args.push("/S".to_owned());
        }
        args.push(format!("/D={}", report.dir.display()));
        assert_eq!(runtime.processes()[0].command.args, args);
        assert_eq!(
            runtime.processes()[0].command.program,
            Path::new("/picked/空白 setup.exe")
        );
        fs::remove_dir_all(report.dir).unwrap();
        root.assert_clean();
    }
}
#[test]
fn app_paths_preserve_unicode_and_reject_traversal_without_rewriting_it() {
    use nsis::strings::{NsisString, StringSegment};
    use std::borrow::Cow;
    let path = |tail: &str| NsisString {
        segments: vec![
            StringSegment::Variable {
                index: 21,
                name: Cow::Borrowed("$INSTDIR"),
            },
            StringSegment::Literal(tail.into()),
        ],
    };
    assert_eq!(
        app_path(&path("\\空白 Game\\game.exe")).unwrap().unwrap(),
        Path::new("空白 Game/game.exe")
    );
    for value in [
        "\\..\\outside",
        "\\C:escape",
        "\\safe\\..\\escape",
        "\\CON.txt",
    ] {
        assert!(app_path(&path(value)).is_err(), "{value}");
    }
    assert!(app_path(&path("suffix\\outside")).unwrap().is_none());
    let outside = NsisString {
        segments: vec![
            StringSegment::Variable {
                index: 26,
                name: Cow::Borrowed("$PLUGINSDIR"),
            },
            StringSegment::Literal("\\plugin.dll".into()),
        ],
    };
    assert!(app_path(&outside).unwrap().is_none());
    assert!(
        app_path(&NsisString {
            segments: vec![StringSegment::Literal("/absolute/path".into())]
        })
        .is_err()
    );
}
#[test]
fn budgets_truncations_and_random_mutations_stay_bounded() {
    let fixture = corpus().remove(0);
    let bytes = fs::read(fixture.input()).unwrap();
    for limits in [
        NsisLimits {
            max_input_bytes: 16,
            ..Default::default()
        },
        NsisLimits {
            max_decompressed_bytes: 16,
            ..Default::default()
        },
        NsisLimits {
            max_instructions: 1,
            ..Default::default()
        },
        NsisLimits {
            max_metadata_bytes: 16,
            ..Default::default()
        },
        NsisLimits {
            max_path_updates: 0,
            ..Default::default()
        },
    ] {
        assert!(inspect_bytes(&bytes, limits).is_err());
    }
    for length in [0, 1, 32, 128, 512, 1024] {
        assert!(inspect_bytes(&bytes[..length], NsisLimits::default()).is_err());
    }
    let mut state = 19_u64;
    for _ in 0..64 {
        let mut data = bytes.clone();
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let offset = (state as usize) % data.len();
        data[offset] ^= (state >> 32) as u8;
        let _ = inspect_bytes(&data, NsisLimits::default());
    }
}

#[test]
fn real_registry_exec_shortcut_and_uninstaller_metadata_is_preserved() {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("nsis-support/effects.exe");
    let provenance: serde_json::Value =
        serde_json::from_str(include_str!("nsis-support/provenance.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        provenance["binary_sha256"]
    );
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(include_bytes!("nsis-support/effects.nsi"))
        ),
        provenance["source_sha256"]
    );
    let metadata = inspect_bytes(bytes, NsisLimits::default()).unwrap();
    assert!(!metadata.uninstaller);
    assert_eq!(
        metadata.files[0].path.as_deref(),
        Some(Path::new("空白 Game/game.txt"))
    );
    assert_eq!(
        metadata
            .effects
            .iter()
            .filter(|e| e.kind == "registry_write")
            .count(),
        2
    );
    assert_eq!(
        metadata
            .effects
            .iter()
            .filter(|e| e.kind == "registry_delete")
            .count(),
        1
    );
    assert_eq!(
        metadata
            .effects
            .iter()
            .filter(|e| e.kind == "execute")
            .count(),
        2
    );
    assert!(
        metadata
            .effects
            .iter()
            .any(|e| e.fields.get("data").is_some_and(|data| data == "$INSTDIR"))
    );
    assert_eq!(metadata.uninstallers, vec![PathBuf::from("uninstall.exe")]);
    assert_eq!(
        metadata.shortcuts[0].path.as_deref(),
        Some(Path::new("空白 Game/game.exe"))
    );
    assert_eq!(metadata.shortcuts[0].parameters, "--safe");
    insta::assert_json_snapshot!("declared_effects", metadata);
}
#[tokio::test]
async fn native_uninstaller_uses_a_confined_path_and_a_final_unquoted_directory_argument() {
    let node = probe(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/nsis-support/effects.exe"));
    let plans = plan(&node, "linux");
    assert!(
        caskade_installer::formats::windows::uninstall_steps(
            std::slice::from_ref(&node),
            &plans.candidates[0]
        )
        .is_empty()
    );
    let candidate = plans
        .candidates
        .iter()
        .find(|c| c.strategy == Strategy::RunSilent)
        .unwrap();
    let recorded = caskade_installer::formats::windows::uninstall_steps(
        std::slice::from_ref(&node),
        candidate,
    );
    assert!(matches!(
        &recorded[0],
        Step::Run {
            runtime: RuntimeSpec::ProtonPrefix { .. },
            ..
        }
    ));
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join("空白 Game Folder")).unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::Exit { code: 0 }],
    }]));
    let executor = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    );
    executor
        .execute_steps_in_place(
            root.path(),
            Path::new("空白 Game Folder"),
            "nsis-uninstall",
            caskade_installer::formats::windows::nsis::uninstall_steps(&node),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        runtime.processes()[0].command.program,
        root.path().join("空白 Game Folder/uninstall.exe")
    );
    assert_eq!(
        runtime.processes()[0].command.args,
        vec![
            "/S".to_owned(),
            format!("_?={}", root.path().join("空白 Game Folder").display())
        ]
    );
    fs::remove_dir(root.path().join("空白 Game Folder")).unwrap();
    root.assert_clean();
}

#[test]
fn hostile_declared_header_lengths_are_rejected_before_upstream_exact_allocation() {
    let bytes = fs::read(corpus().remove(0).input()).unwrap();
    let overlay = nsis::addressmap::PeOverlay::from_bytes(&bytes).unwrap();
    let (offset, _) = nsis::header::scan_for_first_header(overlay.overlay()).unwrap();
    let offset = overlay.overlay_offset() + offset + 20;
    for length in [i32::MAX, -1, 0] {
        let mut modified = bytes.clone();
        modified[offset..offset + 4].copy_from_slice(&length.to_le_bytes());
        // The upstream locator may decline a nonsensical signed length before
        // our allocation cap runs; both are valid pre-allocation rejections.
        let error = inspect_bytes(&modified, NsisLimits::default()).unwrap_err();
        assert!(
            !error.to_string().contains("upstream NSIS parser rejected"),
            "{length}: {error}"
        );
    }
}
