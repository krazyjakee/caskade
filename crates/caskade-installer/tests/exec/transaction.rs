//! Staged installs, cancellation, recovery and root exclusion.
use super::*;

#[tokio::test]
async fn happy_path_is_atomic_and_event_stream_is_snapshotted() {
    let root = TestRoot::new().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let report = executor(Arc::new(FakeRuntime::default()), sink.clone())
        .execute(
            request(&root, vec![candidate(happy_steps())]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(report.relative_dir, Path::new("Fixture Game"));
    assert_eq!(
        fs::read(report.dir.join("game.bin")).unwrap(),
        vec![42; 131_072]
    );
    assert_eq!(fs::read(report.dir.join("start.sh")).unwrap(), b"game\n");
    assert!(!root.path().join(".caskade").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(report.dir.join("start.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0o111
        );
    }
    // ETA is wall-clock dependent; the event contract retains the field while
    // snapshots normalize it. Counters, phases and step positions stay exact.
    let mut events = serde_json::to_value(sink.events()).unwrap();
    for event in events.as_array_mut().unwrap() {
        event["eta_secs"] = serde_json::Value::Null;
    }
    insta::assert_json_snapshot!("happy_event_stream", events);
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn cancellation_at_every_event_index_restores_an_empty_root() {
    let root = TestRoot::new().unwrap();
    let baseline = Arc::new(RecordingSink::default());
    let report = executor(Arc::new(FakeRuntime::default()), baseline.clone())
        .execute(
            request(&root, vec![candidate(happy_steps())]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    fs::remove_dir_all(report.dir).unwrap();
    let count = baseline.events().len();
    assert!(count > 10, "exercise events within writes and after rename");
    for index in 0..count {
        let token = CancellationToken::new();
        let sink = Arc::new(CancelAt {
            events: RecordingSink::default(),
            token: token.clone(),
            index,
            count: AtomicUsize::new(0),
        });
        let result = executor(Arc::new(FakeRuntime::default()), sink)
            .execute(request(&root, vec![candidate(happy_steps())]), token)
            .await;
        assert!(
            matches!(result, Err(ExecError::Cancelled)),
            "event {index}: {result:?}"
        );
        root.assert_clean();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_job_future_and_startup_cleanup_cannot_race_a_detached_writer() {
    let root = TestRoot::new().unwrap();
    let (release, receiver) = std::sync::mpsc::channel();
    let extractor = Arc::new(PausedExtractor {
        entered: tokio::sync::Notify::new(),
        release: Mutex::new(receiver),
    });
    let executor = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor.clone());
    let worker = executor.clone();
    let job = request(&root, vec![candidate(vec![extraction()])]);
    let task = tokio::spawn(async move { worker.execute(job, CancellationToken::new()).await });
    extractor.entered.notified().await;
    assert!(
        root.path()
            .join(".caskade/staging/test-job/partial.bin")
            .exists()
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    // Startup cleanup must wait until the blocking worker acknowledges the drop.
    let cleanup_executor = executor.clone();
    let cleanup_root = root.path().to_owned();
    let cleanup = tokio::spawn(async move { cleanup_executor.cleanup(&cleanup_root).await });
    tokio::task::yield_now().await;
    assert!(!cleanup.is_finished());
    release.send(()).unwrap();
    cleanup.await.unwrap().unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn cleanup_removes_actual_orphans_and_unfinished_native_targets() {
    let root = TestRoot::new().unwrap();
    fs::create_dir_all(root.path().join(".caskade/staging/orphan")).unwrap();
    fs::write(
        root.path().join(".caskade/staging/orphan/partial"),
        b"partial",
    )
    .unwrap();
    fs::create_dir_all(root.path().join(".caskade/journals")).unwrap();
    fs::create_dir(root.path().join("Fixture Game")).unwrap();
    fs::write(root.path().join("Fixture Game/partial"), b"native partial").unwrap();
    let record = serde_json::json!({ "schema_version": 1, "job_id": "orphan", "relative_dir": "Fixture Game", "final_owned": true, "completed": false, "entries": [] });
    fs::write(
        root.path().join(".caskade/journals/orphan.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .cleanup(root.path())
    .await
    .unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn rejected_plans_and_unsafe_paths_do_not_touch_the_root() {
    let root = TestRoot::new().unwrap();
    let worker = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    );
    for path in ["../sentinel", "/tmp/escape", "C:\\escape", ".caskade"] {
        let mut job = request(&root, vec![candidate(happy_steps())]);
        job.relative_dir = path.into();
        assert!(worker.execute(job, CancellationToken::new()).await.is_err());
        root.assert_clean();
    }
    for path in ["../sentinel", "/tmp/escape", "C:\\escape"] {
        let job = request(
            &root,
            vec![candidate(vec![Step::WriteFile {
                path: path.into(),
                contents: b"bad".to_vec(),
            }])],
        );
        assert!(worker.execute(job, CancellationToken::new()).await.is_err());
        root.assert_clean();
    }
    let mut job = request(&root, vec![candidate(happy_steps())]);
    job.plans.question = Some(Question::LowConfidence { candidates: vec![] });
    assert!(matches!(
        worker.execute(job, CancellationToken::new()).await,
        Err(ExecError::DecisionRequired)
    ));
    root.assert_clean();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn separate_executor_instances_serialize_jobs_for_the_same_root() {
    let root = TestRoot::new().unwrap();
    let (release, receiver) = std::sync::mpsc::channel();
    let extractor = Arc::new(PausedExtractor {
        entered: tokio::sync::Notify::new(),
        release: Mutex::new(receiver),
    });
    let first = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor.clone());
    let first_job = request(&root, vec![candidate(vec![extraction()])]);
    let first_task =
        tokio::spawn(async move { first.execute(first_job, CancellationToken::new()).await });
    extractor.entered.notified().await;
    let second_events = Arc::new(RecordingSink::default());
    let second = executor(Arc::new(FakeRuntime::default()), second_events.clone());
    let mut second_job = request(&root, vec![candidate(happy_steps())]);
    second_job.job_id = "second-job".into();
    let second_token = CancellationToken::new();
    let second_cancel = second_token.clone();
    let second_task = tokio::spawn(async move { second.execute(second_job, second_token).await });
    tokio::task::yield_now().await;
    assert!(second_events.events().is_empty());
    second_cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), second_task)
            .await
            .unwrap()
            .unwrap(),
        Err(ExecError::Cancelled)
    ));
    release.send(()).unwrap();
    let report = first_task.await.unwrap().unwrap();
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn recovery_preserves_completed_installs_and_rejects_traversal_journals() {
    let root = TestRoot::new().unwrap();
    let worker = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    );
    fs::create_dir_all(root.path().join(".caskade/journals")).unwrap();
    fs::create_dir(root.path().join("Fixture Game")).unwrap();
    let mut record = serde_json::json!({ "schema_version": 1, "job_id": "done", "relative_dir": "Fixture Game", "final_owned": true, "completed": true, "entries": [] });
    let journal = root.path().join(".caskade/journals/done.json");
    fs::write(&journal, serde_json::to_vec(&record).unwrap()).unwrap();
    worker.cleanup(root.path()).await.unwrap();
    assert!(root.path().join("Fixture Game").is_dir());
    fs::remove_dir(root.path().join("Fixture Game")).unwrap();
    fs::create_dir_all(root.path().join(".caskade/journals")).unwrap();
    record["relative_dir"] = serde_json::json!("../sentinel");
    record["completed"] = serde_json::json!(false);
    fs::write(&journal, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(worker.cleanup(root.path()).await.is_err());
    root.assert_sentinel_untouched();
    fs::remove_dir_all(root.path().join(".caskade")).unwrap();
    root.assert_clean();
}
