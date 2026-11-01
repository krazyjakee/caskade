//! #158 fault injection: inputs, staged files and output disappear, access
//! is refused, the disk fills, the destination goes away and the network
//! drops, at every stage, without real security software. Each failure is
//! classified with its stage and file, rolls back what the job wrote, and
//! never falls back to another installer strategy.
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[cfg(target_os = "linux")]
use caskade_installer::events::{EventSink, JobEvent};
use caskade_installer::{
    exec::{
        CancellationToken, ExecConfig, ExecError, ExecRequest, Executor, Extractor, StepContext,
    },
    failure::{self, FailureKind, FailureStage, Probe, RecoveryAction, SecurityNote},
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot},
    *,
};

fn candidate(handler: &str, steps: Vec<Step>) -> Candidate {
    Candidate {
        handler: HandlerId(handler.into()),
        strategy: Strategy::Extract,
        confidence: 1.0,
        fidelity: Fidelity::Exact,
        requires: Requirements {
            executes_code: false,
            runtime: None,
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: Some(16),
        },
        steps,
        warnings: vec![],
    }
}

fn extraction() -> Step {
    Step::Extract {
        from: ArtifactId("archive".into()),
        select: vec![],
        strip: None,
        to: ".".into(),
    }
}

/// An extraction that fails, and a strategy behind it that would work.
fn request(root: &Path, source: &Path) -> ExecRequest {
    ExecRequest {
        root: root.into(),
        job_id: "fault".into(),
        relative_dir: "Game".into(),
        plans: PlanSet {
            candidates: vec![
                candidate("first", vec![extraction()]),
                candidate(
                    "second",
                    vec![Step::WriteFile {
                        path: "game.bin".into(),
                        contents: b"fallback".to_vec(),
                    }],
                ),
            ],
            selected: Some(0),
            question: None,
        },
        sources: BTreeMap::from([(ArtifactId("archive".into()), source.to_path_buf())]),
    }
}

fn executor(disk: u64, extractor: Arc<dyn Extractor>) -> Executor {
    Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(disk)),
        Arc::new(RecordingSink::default()),
    )
    .with_config(ExecConfig {
        disk_margin_bytes: 0,
        ..ExecConfig::default()
    })
    .with_extractor(extractor)
}

/// Writes a partial file, then does `fault` and fails with its error.
struct Faulty<F> {
    source: PathBuf,
    fault: F,
    calls: AtomicUsize,
}

impl<F: Fn(&Path) -> io::Error + Send + Sync> Extractor for Faulty<F> {
    fn extract(
        &self,
        _: &ArtifactId,
        _: &[String],
        _: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> exec::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut writer = context.open_destination(destination, 5)?;
        writer.write_file("partial.bin", b"hello".as_slice(), Some(5), 0, 0o644)?;
        Err(ExecError::Io((self.fault)(&self.source)))
    }
}

fn source_file() -> (tempfile::TempDir, PathBuf) {
    let input = tempfile::tempdir().unwrap();
    let source = input.path().join("game.zip");
    fs::write(&source, b"archive").unwrap();
    (input, source)
}

fn probe<'a>(source: &'a Path, root: &'a Path) -> Probe<'a> {
    Probe {
        stage: FailureStage::Install,
        source: Some(source),
        remote: false,
        root: Some(root),
    }
}

#[tokio::test]
async fn a_source_removed_during_extraction_stops_and_names_the_file() {
    let root = TestRoot::new().unwrap();
    let (_input, source) = source_file();
    let extractor = Arc::new(Faulty {
        source: source.clone(),
        // Security software takes the archive away mid-read.
        fault: |source: &Path| {
            fs::remove_file(source).unwrap();
            io::Error::from(io::ErrorKind::NotFound)
        },
        calls: AtomicUsize::new(0),
    });
    let error = executor(u64::MAX, extractor.clone())
        .execute(request(root.path(), &source), CancellationToken::new())
        .await
        .unwrap_err();
    // No fallback to the strategy that would have "worked".
    assert!(matches!(error, ExecError::Io(_)), "{error:?}");
    assert_eq!(extractor.calls.load(Ordering::SeqCst), 1);
    root.assert_clean();
    let failure = failure::classify(&error, &probe(&source, root.path()));
    assert_eq!(failure.kind, FailureKind::MissingSource);
    assert_eq!(failure.path.as_deref(), Some(source.as_path()));
    assert_eq!(failure.security, Some(SecurityNote::Possible));
    assert!(!failure.transient);
    assert_eq!(
        failure.actions,
        [RecoveryAction::LocateSource, RecoveryAction::Cancel]
    );
}

#[tokio::test]
async fn a_replaced_source_is_checked_again_rather_than_trusted() {
    let root = TestRoot::new().unwrap();
    let (_input, source) = source_file();
    let extractor = Arc::new(Faulty {
        source: source.clone(),
        // Replaced by something else between stages: still there, but the
        // extractor can't read it.
        fault: |source: &Path| {
            fs::write(source, b"not an archive").unwrap();
            io::Error::new(io::ErrorKind::InvalidData, "not an archive")
        },
        calls: AtomicUsize::new(0),
    });
    let error = executor(u64::MAX, extractor)
        .execute(request(root.path(), &source), CancellationToken::new())
        .await;
    // Unrecognised content is the strategy's problem: the next one runs,
    // from a clean staging folder.
    let report = error.unwrap();
    assert_eq!(report.candidate_index, 1);
    assert!(!report.dir.join("partial.bin").exists());
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn refused_access_waits_for_the_user_without_another_strategy() {
    let root = TestRoot::new().unwrap();
    let (_input, source) = source_file();
    let extractor = Arc::new(Faulty {
        source: source.clone(),
        fault: |_: &Path| io::Error::from(io::ErrorKind::PermissionDenied),
        calls: AtomicUsize::new(0),
    });
    let error = executor(u64::MAX, extractor.clone())
        .execute(request(root.path(), &source), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(extractor.calls.load(Ordering::SeqCst), 1);
    root.assert_clean();
    let failure = failure::classify(&error, &probe(&source, root.path()));
    assert_eq!(failure.kind, FailureKind::AccessDenied);
    assert!(!failure.transient);
    assert!(failure.actions.contains(&RecoveryAction::Retry));
    assert!(
        failure.summary.contains("can't tell for sure"),
        "{failure:?}"
    );
}

#[tokio::test]
async fn a_file_in_use_is_transient() {
    let root = TestRoot::new().unwrap();
    let (_input, source) = source_file();
    let extractor = Arc::new(Faulty {
        source: source.clone(),
        fault: |_: &Path| io::Error::from(io::ErrorKind::ResourceBusy),
        calls: AtomicUsize::new(0),
    });
    let error = executor(u64::MAX, extractor)
        .execute(request(root.path(), &source), CancellationToken::new())
        .await
        .unwrap_err();
    root.assert_clean();
    let failure = failure::classify(&error, &probe(&source, root.path()));
    assert_eq!(failure.kind, FailureKind::Locked);
    assert!(failure.transient);
}

#[tokio::test]
async fn a_full_disk_stops_before_any_strategy_runs_twice() {
    let root = TestRoot::new().unwrap();
    let (_input, source) = source_file();
    let extractor = Arc::new(Faulty {
        source: source.clone(),
        fault: |_: &Path| io::Error::other("unreachable"),
        calls: AtomicUsize::new(0),
    });
    let error = executor(0, extractor.clone())
        .execute(request(root.path(), &source), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, ExecError::DiskFull { .. }), "{error:?}");
    root.assert_clean();
    let failure = failure::classify(&error, &probe(&source, root.path()));
    assert_eq!(failure.kind, FailureKind::DiskFull);
    assert_eq!(
        failure.actions,
        [
            RecoveryAction::Retry,
            RecoveryAction::ChooseDestination,
            RecoveryAction::Cancel
        ]
    );
}

#[tokio::test]
async fn a_disconnected_destination_is_named() {
    let root = TestRoot::new().unwrap();
    let (_input, source) = source_file();
    let unplugged = root.parent_path().join("unplugged");
    let extractor = Arc::new(Faulty {
        source: source.clone(),
        fault: |_: &Path| io::Error::other("unreachable"),
        calls: AtomicUsize::new(0),
    });
    let error = executor(u64::MAX, extractor.clone())
        .execute(request(&unplugged, &source), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(extractor.calls.load(Ordering::SeqCst), 0);
    let failure = failure::classify(&error, &probe(&source, &unplugged));
    assert_eq!(failure.kind, FailureKind::StorageDisconnected);
    assert_eq!(failure.path.as_deref(), Some(unplugged.as_path()));
    assert!(failure.summary.contains("unplugged"), "{failure:?}");
    root.assert_clean();
}

#[tokio::test]
async fn repeated_disappearance_is_reported_as_such() {
    let root = TestRoot::new().unwrap();
    let mut failures = vec![];
    for _ in 0..2 {
        let (_input, source) = source_file();
        let extractor = Arc::new(Faulty {
            source: source.clone(),
            fault: |source: &Path| {
                fs::remove_file(source).unwrap();
                io::Error::from(io::ErrorKind::NotFound)
            },
            calls: AtomicUsize::new(0),
        });
        let error = executor(u64::MAX, extractor)
            .execute(request(root.path(), &source), CancellationToken::new())
            .await
            .unwrap_err();
        failures.push(failure::classify(&error, &probe(&source, root.path())));
        root.assert_clean();
    }
    // The caller counts repeats; the summary then says it happened again
    // and never offers to retry on its own.
    let mut again = failures.pop().unwrap();
    again.occurrences = 2;
    again.resummarize();
    assert!(again.summary.contains("happened again"), "{again:?}");
    assert!(!again.transient);
}

/// Deletes the game's launch target as soon as the install is in place.
#[cfg(target_os = "linux")]
struct Quarantine {
    root: PathBuf,
    file: &'static str,
    inner: RecordingSink,
}

#[cfg(target_os = "linux")]
impl EventSink for Quarantine {
    fn emit(&self, event: JobEvent) {
        if event.message == "committed" {
            for entry in fs::read_dir(&self.root).unwrap().flatten() {
                let target = entry.path().join(self.file);
                if target.exists() {
                    fs::remove_file(target).unwrap();
                }
            }
        }
        self.inner.emit(event);
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_install_whose_launch_target_vanishes_is_undone_not_reported_installed() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Games");
    fs::create_dir(&root).unwrap();
    let source = temp.path().join("Fixture Runner");
    fs::create_dir(&source).unwrap();
    let exe = source.join("fixture-runner.x86_64");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/fixture-game-linux-x86_64"),
        &exe,
    )
    .unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(source.join("data.pak"), vec![7u8; 4096]).unwrap();
    let sink = Arc::new(Quarantine {
        root: root.clone(),
        file: "fixture-runner.x86_64",
        inner: RecordingSink::default(),
    });
    let ctx = pipeline::Context::new(sink, CancellationToken::new());
    let error = pipeline::install(
        source.to_string_lossy().into_owned(),
        root.clone(),
        false,
        false,
        None,
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<ExecError>(),
            Some(ExecError::MissingExpected { .. })
        ),
        "{error:?}"
    );
    // The job's own output is gone; the source and the root's identity stay.
    let left: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|name| name != ".caskade")
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(exe.is_file());
    let failure = failure::classify(
        &*error,
        &Probe {
            stage: FailureStage::Commit,
            source: Some(&source),
            remote: false,
            root: Some(&root),
        },
    );
    assert_eq!(failure.kind, FailureKind::MissingOutput);
    assert!(
        failure.summary.contains("fixture-runner.x86_64"),
        "{failure:?}"
    );
}

#[cfg(feature = "fetch")]
mod network {
    use super::*;
    use caskade_installer::fetch::{DownloadRequest, FetchConfig, FetchEngine};
    use std::time::Duration;
    use url::Url;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

    fn engine() -> FetchEngine {
        FetchEngine::new(
            FetchConfig {
                retries: 1,
                disk_margin_bytes: 0,
                initial_backoff: Duration::ZERO,
                max_backoff: Duration::ZERO,
                connect_timeout: Duration::from_secs(2),
                stall_timeout: Duration::from_secs(5),
                ..FetchConfig::default()
            },
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .unwrap()
    }

    fn remote(stage: FailureStage) -> Probe<'static> {
        Probe {
            stage,
            source: None,
            remote: true,
            root: None,
        }
    }

    #[tokio::test]
    async fn a_lost_network_is_a_transient_download_failure_without_secrets() {
        // A port nothing listens on any more: the connection is refused.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let root = TestRoot::new().unwrap();
        let request =
            DownloadRequest::new(Url::parse(&format!("{uri}/game.zip?token=secret")).unwrap());
        let error = engine()
            .download(root.path(), "job", &request, CancellationToken::new())
            .await
            .unwrap_err();
        let failure = failure::classify(&error, &remote(FailureStage::Inspect));
        assert_eq!(failure.kind, FailureKind::Download);
        assert_eq!(failure.stage, FailureStage::Download);
        assert!(failure.transient);
        assert!(!failure.detail.contains("secret"), "{failure:?}");
        assert!(!failure.summary.contains("secret"));
    }

    #[tokio::test]
    async fn a_corrupt_download_is_discarded_and_must_be_downloaded_again() {
        let server = MockServer::start().await;
        Mock::given(path("/game.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"tampered".to_vec()))
            .mount(&server)
            .await;
        let root = TestRoot::new().unwrap();
        let mut request =
            DownloadRequest::new(Url::parse(&format!("{}/game.zip", server.uri())).unwrap());
        request.sha256 = Some("0".repeat(64));
        let error = engine()
            .download(root.path(), "job", &request, CancellationToken::new())
            .await
            .unwrap_err();
        let failure = failure::classify(&error, &remote(FailureStage::Download));
        assert_eq!(failure.kind, FailureKind::Integrity);
        assert!(!failure.transient);
        assert_eq!(
            failure.actions,
            [RecoveryAction::Redownload, RecoveryAction::Cancel]
        );
    }

    #[tokio::test]
    async fn a_download_removed_after_it_finished_is_fetched_again_on_retry() {
        let server = MockServer::start().await;
        Mock::given(path("/game.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"archive".to_vec()))
            .expect(2)
            .mount(&server)
            .await;
        let root = TestRoot::new().unwrap();
        let request =
            DownloadRequest::new(Url::parse(&format!("{}/game.zip", server.uri())).unwrap());
        let fetch = engine();
        let file = fetch
            .download(root.path(), "job", &request, CancellationToken::new())
            .await
            .unwrap();
        // Quarantined after the download finished.
        fs::remove_file(&file.path).unwrap();
        let failure = failure::classify(
            &ExecError::Io(io::Error::from(io::ErrorKind::NotFound)),
            &Probe {
                stage: FailureStage::Inspect,
                source: Some(&file.path),
                remote: true,
                root: Some(root.path()),
            },
        );
        assert_eq!(failure.kind, FailureKind::MissingSource);
        assert_eq!(failure.actions[0], RecoveryAction::Redownload);
        let again = fetch
            .download(root.path(), "job", &request, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(fs::read(&again.path).unwrap(), b"archive");
        fetch.after_install(root.path(), "job").await.unwrap();
        root.assert_clean();
    }
}
