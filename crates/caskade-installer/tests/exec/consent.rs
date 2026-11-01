//! A failed static strategy never falls back to installer code the
//! execution policy hasn't approved (CSK-01).
use super::*;

/// Extraction that fails the way a crafted installer's does: one entry's
/// path leaves the destination.
struct TraversingExtractor;

impl Extractor for TraversingExtractor {
    fn extract(
        &self,
        _: &ArtifactId,
        _: &[String],
        _: Option<&Path>,
        _: &Path,
        _: &StepContext,
    ) -> exec::Result<()> {
        Err(safety::SafetyError::ParentTraversal.into())
    }
}

fn silent_install() -> Candidate {
    Candidate {
        strategy: Strategy::RunSilent,
        requires: Requirements {
            executes_code: true,
            ..candidate(vec![]).requires
        },
        ..candidate(vec![run(vec![])])
    }
}

#[tokio::test]
async fn a_failed_extraction_asks_before_running_the_installer_unless_execution_is_allowed() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::default());
    let candidates = vec![candidate(vec![extraction()]), silent_install()];
    let error = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .with_config(ExecConfig {
            disk_margin_bytes: 0,
            execution: plan::ExecutionPolicy::Ask,
            ..ExecConfig::default()
        })
        .with_extractor(Arc::new(TraversingExtractor))
        .execute(request(&root, candidates), CancellationToken::new())
        .await
        .unwrap_err();
    match error {
        ExecError::NeedsUser {
            question: Question::ConfirmExecution { candidates },
        } => assert_eq!(candidates, vec![silent_install()]),
        error => panic!("expected a confirmation, got {error:?}"),
    }
    assert!(runtime.processes().is_empty(), "nothing may run");
    assert!(!root.path().join("Fixture Game").exists());

    // The Allow policy keeps falling back to the installer.
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::WriteFile {
            path: "game.exe".into(),
            contents: b"payload".to_vec(),
        }],
    }]));
    let candidates = vec![candidate(vec![extraction()]), silent_install()];
    let mut request = request(&root, candidates);
    request.sources =
        BTreeMap::from([(ArtifactId("setup".into()), PathBuf::from("/fake/setup.exe"))]);
    let report = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .with_extractor(Arc::new(TraversingExtractor))
        .execute(request, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.candidate.strategy, Strategy::RunSilent);
    assert_eq!(runtime.processes().len(), 1);
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}
