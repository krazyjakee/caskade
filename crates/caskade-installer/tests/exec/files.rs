//! Copy steps, disk space and final folder naming.
use super::*;

#[tokio::test]
async fn folder_templates_and_collisions_preserve_existing_games() {
    assert_eq!(
        folder_name("{title} [{id}]", "Fixture Game", "42").unwrap(),
        Path::new("Fixture Game [42]")
    );
    for template in ["../{title}", "{unknown}", "CON", ".caskade", "/root"] {
        assert!(folder_name(template, "Game", "id").is_err());
    }
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join("Fixture Game")).unwrap();
    fs::write(root.path().join("Fixture Game/keep"), b"keep").unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::WriteFile {
            path: "game.exe".into(),
            contents: b"game".to_vec(),
        }],
    }]));
    let mut job = request(&root, vec![candidate(vec![run(vec!["game.exe".into()])])]);
    job.sources
        .insert(ArtifactId("setup".into()), "/fake/setup.exe".into());
    let report = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .execute(job, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.relative_dir, Path::new("Fixture Game (2)"));
    assert_eq!(
        runtime.processes()[0].command.args[0],
        format!("/DIR={}", report.dir.display())
    );
    assert_eq!(
        fs::read(root.path().join("Fixture Game/keep")).unwrap(),
        b"keep"
    );
    fs::remove_dir_all(report.dir).unwrap();
    fs::remove_dir_all(root.path().join("Fixture Game")).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn space_is_checked_before_and_during_streamed_writes() {
    for disk in [
        FakeDiskSpace::fixed(0),
        FakeDiskSpace::decreasing(1_000_000, 100_000),
    ] {
        let root = TestRoot::new().unwrap();
        let sink = Arc::new(RecordingSink::default());
        let worker = Executor::new(
            Arc::new(FakeRuntime::default()),
            Arc::new(disk),
            sink.clone(),
        )
        .with_config(ExecConfig {
            disk_margin_bytes: 0,
            ..ExecConfig::default()
        });
        let result = worker
            .execute(
                request(
                    &root,
                    vec![candidate(vec![Step::WriteFile {
                        path: "large".into(),
                        contents: vec![7; 500_000],
                    }])],
                ),
                CancellationToken::new(),
            )
            .await;
        assert!(result.is_err());
        root.assert_clean();
    }
}

#[tokio::test]
async fn copy_uses_confined_streaming_and_keeps_original_inputs() {
    let root = TestRoot::new().unwrap();
    let input = tempfile::tempdir().unwrap();
    fs::create_dir(input.path().join("data")).unwrap();
    fs::write(input.path().join("data/original.bin"), b"original").unwrap();
    let mut job = request(
        &root,
        vec![candidate(vec![Step::Copy {
            from: SourcePath::Artifact {
                artifact: ArtifactId("input".into()),
                path: ".".into(),
            },
            to: ".".into(),
        }])],
    );
    job.sources
        .insert(ArtifactId("input".into()), input.path().into());
    let report = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .execute(job, CancellationToken::new())
    .await
    .unwrap();
    assert_eq!(
        fs::read(report.dir.join("data/original.bin")).unwrap(),
        b"original"
    );
    assert_eq!(
        fs::read(input.path().join("data/original.bin")).unwrap(),
        b"original"
    );
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn copy_range_writes_a_slice_of_a_file_artifact_and_refuses_overruns() {
    let input = tempfile::tempdir().unwrap();
    let pack = input.path().join("game.pack");
    fs::write(&pack, b"HEADERgame-bytes|readme").unwrap();
    let range = |offset, length| Step::CopyRange {
        from: ArtifactId("input".into()),
        offset,
        length,
        path: "bin/game".into(),
    };

    let root = TestRoot::new().unwrap();
    let mut job = request(&root, vec![candidate(vec![range(6, 10)])]);
    job.sources.insert(ArtifactId("input".into()), pack.clone());
    let report = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .execute(job, CancellationToken::new())
    .await
    .unwrap();
    assert_eq!(
        fs::read(report.dir.join("bin/game")).unwrap(),
        b"game-bytes"
    );
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();

    for (offset, length) in [(20, 10), (u64::MAX, 2)] {
        let root = TestRoot::new().unwrap();
        let mut job = request(&root, vec![candidate(vec![range(offset, length)])]);
        job.sources.insert(ArtifactId("input".into()), pack.clone());
        executor(
            Arc::new(FakeRuntime::default()),
            Arc::new(RecordingSink::default()),
        )
        .execute(job, CancellationToken::new())
        .await
        .unwrap_err();
        root.assert_clean();
    }
}

#[tokio::test]
async fn a_small_file_does_not_require_an_entire_read_buffers_worth_of_space() {
    let root = TestRoot::new().unwrap();
    let worker = Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(5)),
        Arc::new(RecordingSink::default()),
    )
    .with_config(ExecConfig {
        disk_margin_bytes: 0,
        ..ExecConfig::default()
    });
    let mut proposal = candidate(vec![Step::WriteFile {
        path: "tiny".into(),
        contents: b"small".to_vec(),
    }]);
    proposal.requires.est_disk_bytes = Some(5);
    let report = worker
        .execute(request(&root, vec![proposal]), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(fs::read(report.dir.join("tiny")).unwrap(), b"small");
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[cfg(unix)]
#[tokio::test]
async fn copy_cannot_traverse_a_source_symlink_parent() {
    let root = TestRoot::new().unwrap();
    let input = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private"), b"private").unwrap();
    std::os::unix::fs::symlink(outside.path(), input.path().join("escape")).unwrap();
    let mut job = request(
        &root,
        vec![candidate(vec![Step::Copy {
            from: SourcePath::Artifact {
                artifact: ArtifactId("input".into()),
                path: "escape/private".into(),
            },
            to: "copied".into(),
        }])],
    );
    job.sources
        .insert(ArtifactId("input".into()), input.path().into());
    let result = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    )
    .execute(job, CancellationToken::new())
    .await;
    assert!(result.is_err());
    assert_eq!(
        fs::read(outside.path().join("private")).unwrap(),
        b"private"
    );
    root.assert_clean();
}

#[tokio::test]
async fn root_collision_matching_honours_case_insensitive_profiles() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join("fixture game")).unwrap();
    let worker = Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_config(ExecConfig {
        case_sensitive: false,
        disk_margin_bytes: 0,
        ..ExecConfig::default()
    });
    let report = worker
        .execute(
            request(&root, vec![candidate(happy_steps())]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(report.relative_dir, Path::new("Fixture Game (2)"));
    fs::remove_dir_all(report.dir).unwrap();
    fs::remove_dir(root.path().join("fixture game")).unwrap();
    root.assert_clean();
}
