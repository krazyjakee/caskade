#![cfg(feature = "windows-formats")]
use caskade_installer::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, PayloadPlatform, Program,
    Requirements, Role, RuntimeSpec, SourcePath, Step, Strategy,
    exec::{CancellationToken, ExecRequest, Executor},
    formats::{
        archive::{ArchiveOptions, NoopControl},
        windows::inno::{
            InnoError, InnoExtractor, InnoLimits, app_path, extract_path, inspect_bytes,
            inspect_path, split_arguments, uninstall_steps,
        },
    },
    plan::PlanSet,
    safety::{Limits, SafeDir, open_staging},
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
fn input(id: &str) -> PathBuf {
    fixtures().join("bin/windows").join(id).join("setup.exe")
}
fn worker(extractor: Arc<InnoExtractor>) -> Executor {
    Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor)
}
fn request(root: &TestRoot, source: PathBuf, extractor: &mut Arc<InnoExtractor>) -> ExecRequest {
    let id = ArtifactId("input".into());
    let sources = BTreeMap::from([(id.clone(), source)]);
    *extractor = Arc::new(InnoExtractor::new(sources.clone()));
    ExecRequest {
        job_id: "safety".into(),
        root: root.path().into(),
        relative_dir: "Game".into(),
        sources,
        plans: PlanSet {
            selected: Some(0),
            question: None,
            candidates: vec![Candidate {
                handler: HandlerId("inno".into()),
                strategy: Strategy::Extract,
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements {
                    executes_code: false,
                    runtime: None,
                    may_prompt_elevation: false,
                    interactive: false,
                    est_disk_bytes: None,
                },
                steps: vec![Step::Extract {
                    from: id,
                    select: vec![],
                    strip: None,
                    to: ".".into(),
                }],
                warnings: vec![],
            }],
        },
    }
}

#[test]
fn app_destinations_and_windows_argument_vectors_are_portable_and_confined() {
    for raw in [
        r"{app}\..\escape",
        r"{app}\C:\escape",
        r"{app}\CON",
        r"{app}\nested\..\bad",
        r"{app}\{code:evil}",
        r"{app}\file:stream",
    ] {
        assert!(app_path(raw).is_err(), "{raw}");
    }
    assert_eq!(
        app_path(r"{APP}\bin\Game.exe").unwrap(),
        Some(PathBuf::from("bin/Game.exe"))
    );
    assert_eq!(app_path("{app}").unwrap(), Some(PathBuf::from(".")));
    for raw in [r"{sys}\kernel32.dll", r"{tmp}\helper.exe", r"C:\outside"] {
        assert_eq!(app_path(raw).unwrap(), None);
    }
    assert_eq!(
        split_arguments(r#"--title "A game ü" "C:\Games\Demo" empty """#).unwrap(),
        vec!["--title", "A game ü", r"C:\Games\Demo", "empty", ""]
    );
    assert!(split_arguments("\"unclosed").is_err());
    assert!(split_arguments("bad\0arg").is_err());
}

#[test]
fn icons_become_exact_launch_hints_and_uninstall_steps_remain_game_relative() {
    let mut metadata = inspect_path(&input("inno60-basic"), InnoLimits::default()).unwrap();
    metadata
        .icons
        .push(caskade_installer::formats::windows::inno::IconMetadata {
            name: "{group}\\Game".into(),
            filename: r"{app}\FixtureGame.exe".into(),
            parameters: r#"--title "A game ü""#.into(),
            working_directory: "{app}".into(),
        });
    let hints = metadata.discovery_hints().unwrap();
    assert_eq!(
        hints.launch_targets[0].path,
        PathBuf::from("FixtureGame.exe")
    );
    assert_eq!(hints.launch_targets[0].args, vec!["--title", "A game ü"]);
    assert_eq!(hints.launch_targets[0].working_dir, Some(".".into()));
    metadata.uninstaller = Some(r"{app}\support\unins000.exe".into());
    let node = ArtifactNode {
        id: ArtifactId("input".into()),
        path: "setup.exe".into(),
        format: Format::Inno,
        platform: PayloadPlatform::Windows,
        role: Role::Installer,
        size: None,
        facts: BTreeMap::from([(
            "inno.metadata".into(),
            serde_json::to_value(&metadata).unwrap(),
        )]),
        children: vec![],
        children_complete: false,
    };
    assert!(
        matches!(&uninstall_steps(&node)[0], Step::Run {program:Program::Source(SourcePath::Game(path)),runtime:RuntimeSpec::Native,..} if path==Path::new("support/unins000.exe"))
    );
    metadata.uninstaller = Some(r"{sys}\unins.exe".into());
    let mut unsafe_node = node;
    unsafe_node.facts.insert(
        "inno.metadata".into(),
        serde_json::to_value(metadata).unwrap(),
    );
    assert!(uninstall_steps(&unsafe_node).is_empty());
}

#[tokio::test]
async fn passwords_corruption_and_missing_slices_fail_without_leaving_a_game() {
    let mut cases = Vec::new();
    for id in ["inno5-password", "inno60-password", "inno64-password"] {
        cases.push(input(id));
    }
    let temporary = tempfile::TempDir::new().unwrap();
    let corrupt = temporary.path().join("corrupt.exe");
    let mut bytes = fs::read(input("inno60-basic")).unwrap();
    let payload_offset = {
        let installer = innospect::InnoInstaller::from_bytes(&bytes).unwrap();
        let location = &installer.file_locations()[0];
        (installer.offset_table().offset_setup1
            + u64::from(location.start_offset)
            + location.chunk_compressed_size / 2) as usize
    };
    bytes[payload_offset] ^= 0xff;
    fs::write(&corrupt, bytes).unwrap();
    cases.push(corrupt);
    let sliced = temporary.path().join("sliced");
    fs::create_dir(&sliced).unwrap();
    fs::copy(input("inno5-sliced"), sliced.join("setup.exe")).unwrap();
    cases.push(sliced.join("setup.exe"));
    for source in cases {
        let root = TestRoot::new().unwrap();
        let mut extractor = Arc::new(InnoExtractor::default());
        let request = request(&root, source.clone(), &mut extractor);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            worker(extractor).execute(request, CancellationToken::new()),
        )
        .await
        .expect("extract must not wait for password or volumes");
        assert!(result.is_err(), "{}", source.display());
        root.assert_clean();
    }
}

#[test]
fn all_three_protected_versions_have_typed_password_errors_before_any_write() {
    for id in ["inno5-password", "inno60-password", "inno64-password"] {
        let root = tempfile::TempDir::new().unwrap();
        let mut safe = SafeDir::new(
            open_staging(root.path()).unwrap(),
            Limits::default(),
            3_000_000,
            true,
        );
        let result = extract_path(
            &input(id),
            &mut safe,
            &ArchiveOptions::default(),
            InnoLimits::default(),
            &mut NoopControl,
        );
        assert!(matches!(result, Err(InnoError::PasswordRequired)));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn metadata_and_output_limits_are_enforced_and_truncation_is_bounded() {
    let bytes = fs::read(input("inno60-basic")).unwrap();
    let tiny = InnoLimits {
        max_metadata_input_bytes: 16,
        ..InnoLimits::default()
    };
    assert!(matches!(
        inspect_bytes(&bytes, tiny),
        Err(InnoError::Limit(_))
    ));
    let tiny = InnoLimits {
        max_entries: 1,
        ..InnoLimits::default()
    };
    assert!(matches!(
        inspect_path(&input("inno60-basic"), tiny),
        Err(InnoError::Limit(_))
    ));
    for percent in [0, 25, 50, 75, 99] {
        let slice = &bytes[..bytes.len() * percent / 100];
        let _ = inspect_bytes(slice, InnoLimits::default());
    }
    let root = tempfile::TempDir::new().unwrap();
    let mut safe = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits::default(),
        3_000_000,
        true,
    );
    let options = ArchiveOptions {
        max_unpacked_bytes: 10,
        ..ArchiveOptions::default()
    };
    assert!(matches!(
        extract_path(
            &input("inno60-basic"),
            &mut safe,
            &options,
            InnoLimits::default(),
            &mut NoopControl
        ),
        Err(InnoError::Limit(_))
    ));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn selective_sliced_extraction_preserves_streaming_checksums_and_normalized_paths() {
    let root = tempfile::TempDir::new().unwrap();
    let mut safe = SafeDir::new(
        open_staging(root.path()).unwrap(),
        Limits::default(),
        5_000_000,
        true,
    );
    let options = ArchiveOptions {
        select: vec!["FixtureGame_Data/**".into()],
        strip: Some("FixtureGame_Data".into()),
        ..ArchiveOptions::default()
    };
    let (_, nodes) = extract_path(
        &input("inno5-sliced"),
        &mut safe,
        &options,
        InnoLimits::default(),
        &mut NoopControl,
    )
    .unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].path, PathBuf::from("globalgamemanagers"));
    assert!(root.path().join("globalgamemanagers").is_file());
    assert!(!root.path().join("FixtureGame.exe").exists());
}

#[tokio::test]
async fn cancellation_from_streamed_progress_removes_partial_payload_and_keeps_sentinel() {
    use caskade_installer::{
        events::{EventSink, JobEvent, Phase},
        exec::ExecError,
    };
    struct CancelOnBytes {
        cancellation: CancellationToken,
        events: RecordingSink,
    }
    impl EventSink for CancelOnBytes {
        fn emit(&self, event: JobEvent) {
            if event.phase == Phase::Install && event.bytes > 0 && event.files == 0 {
                self.cancellation.cancel();
            }
            self.events.emit(event);
        }
    }
    let root = TestRoot::new().unwrap();
    let mut extractor = Arc::new(InnoExtractor::default());
    let request = request(&root, input("inno5-sliced"), &mut extractor);
    let cancellation = CancellationToken::new();
    let sink = Arc::new(CancelOnBytes {
        cancellation: cancellation.clone(),
        events: RecordingSink::default(),
    });
    let worker = Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink.clone(),
    )
    .with_extractor(extractor);
    let result = worker.execute(request, cancellation).await;
    assert!(matches!(result, Err(ExecError::Cancelled)), "{result:?}");
    assert!(
        sink.events
            .events()
            .iter()
            .any(|event| event.bytes > 0 && event.files == 0)
    );
    root.assert_clean();
}
