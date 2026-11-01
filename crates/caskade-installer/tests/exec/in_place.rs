//! Recorded uninstall steps run against an existing game folder.
use super::*;

#[tokio::test]
async fn in_place_uninstall_steps_preserve_the_existing_folder_and_need_no_inverse() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join("Installed Game")).unwrap();
    fs::write(root.path().join("Installed Game/keep.bin"), b"keep").unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::Exit { code: 0 }],
    }]));
    let worker = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(0)),
        Arc::new(RecordingSink::default()),
    );
    worker
        .execute_steps_in_place(
            root.path(),
            Path::new("Installed Game"),
            "uninstall-job",
            vec![Step::Delegate {
                tool: "flatpak".into(),
                args: vec!["uninstall".into(), "fixture".into()],
                uninstall: vec![],
                only_if: None,
            }],
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        runtime.processes()[0].command.args,
        vec!["uninstall", "fixture"]
    );
    assert_eq!(
        fs::read(root.path().join("Installed Game/keep.bin")).unwrap(),
        b"keep"
    );
    assert!(!root.path().join(".caskade").exists());
    fs::remove_dir_all(root.path().join("Installed Game")).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn in_place_uninstall_skips_a_delegate_whose_check_finds_it_gone() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join("Installed Game")).unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::Exit { code: 1 }],
    }]));
    let worker = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(0)),
        Arc::new(RecordingSink::default()),
    );
    worker
        .execute_steps_in_place(
            root.path(),
            Path::new("Installed Game"),
            "uninstall-job",
            vec![Step::Delegate {
                tool: "apt".into(),
                args: vec!["remove".into(), "fixture".into()],
                uninstall: vec![],
                only_if: Some(DelegateCheck {
                    tool: "dpkg-query".into(),
                    args: vec!["--status".into(), "fixture".into()],
                }),
            }],
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let processes = runtime.processes();
    assert_eq!(processes.len(), 1);
    assert_eq!(processes[0].command.program, Path::new("dpkg-query"));
    fs::remove_dir_all(root.path().join("Installed Game")).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn in_place_finalization_can_handle_a_failed_uninstaller_and_delete_under_the_root_lease() {
    let root = TestRoot::new().unwrap();
    fs::create_dir(root.path().join("Installed Game")).unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::Exit { code: 42 }],
    }]));
    let worker = executor(runtime, Arc::new(RecordingSink::default()));
    let warning = worker
        .execute_steps_in_place_then(
            root.path(),
            std::path::Path::new("Installed Game"),
            "uninstall-job",
            vec![Step::Delegate {
                tool: "uninstall".into(),
                args: vec![],
                uninstall: vec![],
                only_if: None,
            }],
            CancellationToken::new(),
            |capability, result| {
                assert!(matches!(result, Err(ExecError::ProcessFailed { code: 42 })));
                capability.remove_dir_all("Installed Game")?;
                Ok("uninstaller failed; files removed")
            },
        )
        .await
        .unwrap();
    assert_eq!(warning, "uninstaller failed; files removed");
    root.assert_clean();
}

#[tokio::test]
async fn missing_game_still_runs_the_uninstall_cleanup_callback_under_the_root_lease() {
    let root = TestRoot::new().unwrap();
    let worker = executor(
        Arc::new(FakeRuntime::default()),
        Arc::new(RecordingSink::default()),
    );
    let value = worker.execute_steps_in_place_then(root.path(), Path::new("Missing Game"), "gone-job", vec![], CancellationToken::new(),
        |_, result| { assert!(matches!(result, Err(ExecError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)); Ok(7) }).await.unwrap();
    assert_eq!(value, 7);
    root.assert_clean();
}
