use caskade_installer::{
    PlanSet, Step,
    exec::{CancellationToken, ExecRequest, Executor},
    manifest::{InstallManifest, UninstallRequest, Uninstaller},
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot},
};
use caskade_installer::{
    Program, RuntimeSpec,
    exec::{JournalEntry, JournalHandle, ProcessObservation, ProcessObserver, StepContext},
    manifest::{
        AliasRemover, ExternalEffect, ExternalLocation, ManifestPath, UninstallError,
        UninstallFailureReason, UninstallOperation, effects_from_journal,
    },
    runtime::{ProcessCommand, ProcessExit, ProcessId, RuntimeError},
    testing::{FakeAction, FakeScript},
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

fn executor() -> Executor {
    Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
}

fn save(root: &TestRoot, manifest: InstallManifest) -> InstallManifest {
    manifest.write(&root.path().join("Fixture Game")).unwrap();
    manifest
}

struct ObserveFiles {
    entries: Vec<JournalEntry>,
}
impl ProcessObserver for ObserveFiles {
    fn start(
        &self,
        _: &ProcessCommand,
        journal: &JournalHandle,
        _: &StepContext,
    ) -> caskade_installer::exec::Result<Box<dyn ProcessObservation>> {
        for entry in &self.entries {
            journal.record(entry.clone())?;
        }
        Ok(Box::new(NoObservation))
    }
}
struct NoObservation;
impl ProcessObservation for NoObservation {
    fn finish(self: Box<Self>, _: &JournalHandle) -> caskade_installer::exec::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn retained_executor_journal_removes_created_files_and_restores_previous_bytes() {
    let root = TestRoot::new().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(outside.path()).unwrap();
    let settings = base.join("settings.ini");
    let shortcut = base.join("Game/shortcut.lnk");
    fs::write(&settings, b"old config").unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![
            FakeAction::WriteFile {
                path: shortcut.clone(),
                contents: b"shortcut".to_vec(),
            },
            FakeAction::WriteFile {
                path: settings.clone(),
                contents: b"new config".to_vec(),
            },
            FakeAction::Exit { code: 0 },
        ],
    }]));
    let entries = vec![
        JournalEntry::ExternalDirectory {
            path: base.join("Game"),
        },
        JournalEntry::ExternalFile {
            path: shortcut.clone(),
            original: None,
        },
        JournalEntry::ExternalFile {
            path: settings.clone(),
            original: Some(b"old config".to_vec()),
        },
    ];
    let worker = Executor::new(
        runtime,
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_observer(Arc::new(ObserveFiles { entries }));
    let (manifest, report) = install_extra(
        &root,
        &worker,
        vec![Step::Run {
            program: Program::Tool("fixture-installer".into()),
            args: vec![],
            env: BTreeMap::new(),
            runtime: RuntimeSpec::Native,
            timeout_secs: 30,
            expect: vec![],
        }],
    )
    .await;
    assert_eq!(fs::read(&settings).unwrap(), b"new config");
    assert_eq!(report.journal.len(), 3);
    let roots = BTreeMap::from([("desktop".into(), base.clone())]);
    let mut record = manifest.into_record();
    record.external_effects = effects_from_journal(&report.journal, &roots).unwrap();
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    let report = Uninstaller::new(worker)
        .with_external_roots(roots)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.external_effects_reverted, 3);
    assert!(report.is_clean());
    assert_eq!(fs::read(&settings).unwrap(), b"old config");
    assert!(!shortcut.exists());
    assert!(!base.join("Game").exists());
    root.assert_clean();
}

struct TestAlias {
    path: PathBuf,
    marker: Option<PathBuf>,
    fail: bool,
    calls: AtomicUsize,
}
impl AliasRemover for TestAlias {
    fn remove_alias(&self, game: &Path, _: &InstallManifest) -> std::io::Result<bool> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(game.is_dir(), "alias removal must precede game removal");
        if let Some(marker) = &self.marker {
            assert_eq!(fs::read(marker)?, b"ran");
        }
        if self.fail {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "alias busy",
            ));
        }
        fs::remove_file(&self.path)?;
        Ok(true)
    }
}

#[tokio::test]
async fn alias_hook_runs_after_steps_and_before_removing_the_game() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let mut record = install(&root, &worker).await.into_record();
    record.uninstall_steps = vec![Step::WriteFile {
        path: "marker".into(),
        contents: b"ran".to_vec(),
    }];
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path().join("Fixture Game"), &path).unwrap();
    #[cfg(not(unix))]
    fs::write(&path, b"alias hook token").unwrap();
    let alias = Arc::new(TestAlias {
        path: path.clone(),
        marker: Some(root.path().join("Fixture Game/marker")),
        fail: false,
        calls: AtomicUsize::new(0),
    });
    let report = Uninstaller::new(worker)
        .with_alias_remover(alias.clone())
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert!(report.alias_removed);
    assert_eq!(alias.calls.load(Ordering::SeqCst), 1);
    assert!(!path.exists());
    root.assert_clean();
}

/// #31: the manifest's alias ID is removed from the host AliasStore; only the
/// link goes, never what it pointed at.
#[tokio::test]
async fn recorded_alias_is_removed_from_the_alias_store() {
    use caskade_installer::runtime::alias::AliasStore;
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let home = tempfile::tempdir().unwrap();
    let store = AliasStore::new(home.path().join("g"));
    let mut record = install(&root, &worker).await.into_record();
    let id = AliasStore::short_id("test-root/Fixture Game");
    let link = store
        .link(
            &id,
            &dunce::canonicalize(root.path())
                .unwrap()
                .join("Fixture Game"),
        )
        .unwrap();
    record.alias = Some(id.clone());
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    let report = Uninstaller::new(worker)
        .with_aliases(store.clone())
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert!(report.alias_removed && report.game_removed);
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(store.target(&id).unwrap(), None);
    root.assert_clean();
}

#[test]
fn invalid_alias_ids_are_rejected_by_the_manifest() {
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.alias = Some("../escape".into());
    assert!(InstallManifest::new(record).is_err());
}

#[tokio::test]
async fn alias_failure_is_reported_and_keeps_the_manifest_for_retry() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let manifest = install(&root, &worker).await;
    let alias = Arc::new(TestAlias {
        path: PathBuf::new(),
        marker: None,
        fail: true,
        calls: AtomicUsize::new(0),
    });
    let error = Uninstaller::new(worker.clone())
        .with_alias_remover(alias)
        .uninstall(request(&root, manifest.clone()), CancellationToken::new())
        .await
        .unwrap_err();
    let report = error.report().unwrap();
    assert_eq!(report.failures[0].operation, UninstallOperation::Alias);
    assert!(!report.game_removed);
    assert_eq!(
        InstallManifest::read(&root.path().join("Fixture Game")).unwrap(),
        manifest
    );
    Uninstaller::new(worker)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn all_missing_external_scopes_are_reported_and_retry_can_complete() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let mut record = install(&root, &worker).await.into_record();
    record.external_effects = ["desktop", "appdata"]
        .into_iter()
        .map(|scope| ExternalEffect::File {
            location: ExternalLocation {
                scope: scope.into(),
                path: ManifestPath::new("game-file").unwrap(),
            },
            original: None,
        })
        .collect();
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    let error = Uninstaller::new(worker.clone())
        .uninstall(request(&root, manifest.clone()), CancellationToken::new())
        .await
        .unwrap_err();
    let report = error.report().unwrap();
    assert_eq!(report.failures.len(), 2);
    assert!(report.failures.iter().all(|f| matches!(
        f.reason,
        UninstallFailureReason::MissingExternalScope { .. }
    )));
    assert!(!report.game_removed);
    let desktop = tempfile::tempdir().unwrap();
    let appdata = tempfile::tempdir().unwrap();
    let roots = BTreeMap::from([
        ("desktop".into(), desktop.path().into()),
        ("appdata".into(), appdata.path().into()),
    ]);
    Uninstaller::new(worker)
        .with_external_roots(roots)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn user_files_in_an_external_directory_are_preserved_and_reported() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let mut record = install(&root, &worker).await.into_record();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join("Game")).unwrap();
    fs::write(outside.path().join("Game/user-save"), b"keep").unwrap();
    record.external_effects = vec![ExternalEffect::Directory {
        location: ExternalLocation {
            scope: "desktop".into(),
            path: ManifestPath::new("Game").unwrap(),
        },
    }];
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    let error = Uninstaller::new(worker)
        .with_external_roots(BTreeMap::from([("desktop".into(), outside.path().into())]))
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error.report().unwrap().failures[0].operation,
        UninstallOperation::External(_)
    ));
    assert_eq!(
        fs::read(outside.path().join("Game/user-save")).unwrap(),
        b"keep"
    );
    root.assert_sentinel_untouched();
}

#[tokio::test]
async fn unsupported_steps_are_explicit_errors_and_leave_the_game_for_retry() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let mut record = install(&root, &worker).await.into_record();
    record.uninstall_steps = vec![Step::Mount {
        from: caskade_installer::ArtifactId("source".into()),
        mount_point: "disc".into(),
    }];
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    let error = Uninstaller::new(worker)
        .uninstall(request(&root, manifest.clone()), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, UninstallError::Unsupported { .. }));
    assert!(!error.report().unwrap().game_removed);
    assert_eq!(
        InstallManifest::read(&root.path().join("Fixture Game")).unwrap(),
        manifest
    );
    root.assert_sentinel_untouched();
}

fn run_game() -> Step {
    Step::Run {
        program: Program::Source(caskade_installer::SourcePath::Game(
            "bin/FixtureGame".into(),
        )),
        args: vec!["--uninstall".into()],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 30,
        expect: vec![],
    }
}

/// Stand-in for a runtime that cannot run anything on this host.
struct UnsupportedRuntime;
impl caskade_installer::runtime::Runtime for UnsupportedRuntime {
    fn spawn(&self, _: &ProcessCommand) -> std::result::Result<ProcessId, RuntimeError> {
        Err(RuntimeError::NotImplemented {
            operation: "spawn".into(),
        })
    }
    fn wait(
        &self,
        _: ProcessId,
        _: std::time::Duration,
    ) -> std::result::Result<ProcessExit, RuntimeError> {
        Err(RuntimeError::NotImplemented {
            operation: "wait".into(),
        })
    }
    fn kill(&self, _: ProcessId) -> std::result::Result<(), RuntimeError> {
        Err(RuntimeError::NotImplemented {
            operation: "kill".into(),
        })
    }
}

#[tokio::test]
async fn process_failure_is_reported_and_unimplemented_native_runtime_is_unsupported() {
    for native_stub in [false, true] {
        let root = TestRoot::new().unwrap();
        let runtime: Arc<dyn caskade_installer::runtime::Runtime> = if native_stub {
            Arc::new(UnsupportedRuntime)
        } else {
            Arc::new(FakeRuntime::new([FakeScript {
                actions: vec![FakeAction::Exit { code: 42 }],
            }]))
        };
        let worker = Executor::new(
            runtime,
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        );
        let mut record = install(&root, &worker).await.into_record();
        record.uninstall_steps = vec![run_game()];
        let manifest = save(&root, InstallManifest::new(record).unwrap());
        let error = Uninstaller::new(worker)
            .uninstall(request(&root, manifest), CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(
            matches!(error, UninstallError::Unsupported { .. }),
            native_stub
        );
        assert!(!error.report().unwrap().game_removed);
        root.assert_sentinel_untouched();
    }
}

#[tokio::test]
async fn already_missing_game_is_idempotent_and_cancellation_preserves_an_existing_game() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let manifest = install(&root, &worker).await;
    let token = CancellationToken::new();
    token.cancel();
    assert!(
        Uninstaller::new(worker.clone())
            .uninstall(request(&root, manifest.clone()), token)
            .await
            .is_err()
    );
    assert!(root.path().join("Fixture Game").exists());
    let uninstaller = Uninstaller::new(worker);
    uninstaller
        .uninstall(request(&root, manifest.clone()), CancellationToken::new())
        .await
        .unwrap();
    root.assert_clean();
    assert!(
        uninstaller
            .uninstall(request(&root, manifest), CancellationToken::new())
            .await
            .unwrap()
            .is_clean()
    );
    root.assert_clean();
}

#[tokio::test]
async fn unsafe_game_paths_and_manifest_mismatches_are_refused_before_deletion() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let manifest = install(&root, &worker).await;
    for path in ["../sentinel", ".caskade", "/tmp", "C:\\Games", "Game/other"] {
        let mut bad = request(&root, manifest.clone());
        bad.relative_dir = path.into();
        assert!(
            Uninstaller::new(worker.clone())
                .uninstall(bad, CancellationToken::new())
                .await
                .is_err()
        );
        root.assert_sentinel_untouched();
    }
    let mut stale = manifest.into_record();
    stale.title = "another install".into();
    assert!(matches!(
        Uninstaller::new(worker)
            .uninstall(
                request(&root, InstallManifest::new(stale).unwrap()),
                CancellationToken::new()
            )
            .await,
        Err(UninstallError::Invalid(_))
    ));
    assert!(root.path().join("Fixture Game").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn external_symlink_escapes_are_reported_and_game_symlinks_are_unlinked_safely() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let mut record = install(&root, &worker).await.into_record();
    let outside = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    fs::write(private.path().join("secret"), b"keep").unwrap();
    std::os::unix::fs::symlink(private.path(), outside.path().join("escape")).unwrap();
    record.external_effects = vec![ExternalEffect::File {
        location: ExternalLocation {
            scope: "desktop".into(),
            path: ManifestPath::new("escape/secret").unwrap(),
        },
        original: None,
    }];
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    let error = Uninstaller::new(worker.clone())
        .with_external_roots(BTreeMap::from([("desktop".into(), outside.path().into())]))
        .uninstall(request(&root, manifest.clone()), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error.report().unwrap().failures[0].operation,
        UninstallOperation::External(_)
    ));
    assert_eq!(fs::read(private.path().join("secret")).unwrap(), b"keep");
    let mut record = manifest.into_record();
    record.external_effects.clear();
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    std::os::unix::fs::symlink(
        private.path(),
        root.path().join("Fixture Game/external-link"),
    )
    .unwrap();
    Uninstaller::new(worker)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(fs::read(private.path().join("secret")).unwrap(), b"keep");
    root.assert_clean();
}

async fn install(root: &TestRoot, worker: &Executor) -> InstallManifest {
    install_extra(root, worker, Vec::new()).await.0
}

async fn install_extra(
    root: &TestRoot,
    worker: &Executor,
    extra: Vec<Step>,
) -> (InstallManifest, caskade_installer::exec::ExecReport) {
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.plan.candidate.steps = vec![
        Step::WriteFile {
            path: "bin/FixtureGame".into(),
            contents: b"game".to_vec(),
        },
        Step::WriteFile {
            path: "data/config.ini".into(),
            contents: b"config".to_vec(),
        },
        Step::WriteFile {
            path: "data/café.txt".into(),
            contents: b"ok".to_vec(),
        },
    ];
    record.plan.candidate.steps.extend(extra);
    record.uninstall_steps.clear();
    let report = worker
        .execute(
            ExecRequest {
                job_id: "install-for-uninstall".into(),
                root: root.path().into(),
                relative_dir: "Fixture Game".into(),
                plans: PlanSet {
                    candidates: vec![record.plan.candidate.clone()],
                    selected: Some(0),
                    question: None,
                },
                sources: BTreeMap::new(),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    record.plan.relative_dir = report.relative_dir.clone();
    record.plan.candidate = report.candidate.clone();
    let manifest = InstallManifest::new(record).unwrap();
    manifest.write(&report.dir).unwrap();
    assert!(manifest.verify(&report.dir).unwrap().is_ok());
    (manifest, report)
}

fn request(root: &TestRoot, manifest: InstallManifest) -> UninstallRequest {
    UninstallRequest {
        job_id: "uninstall-test".into(),
        root: root.path().into(),
        relative_dir: "Fixture Game".into(),
        manifest,
    }
}

#[tokio::test]
async fn executor_install_then_manifest_uninstall_leaves_a_clean_root() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let mut manifest = install(&root, &worker).await.into_record();
    manifest.uninstall_steps.clear();
    let manifest = InstallManifest::new(manifest).unwrap();
    manifest.write(&root.path().join("Fixture Game")).unwrap();
    let report = Uninstaller::new(worker)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert!(report.is_clean());
    assert!(report.steps_completed);
    assert_eq!(report.relative_dir.as_path(), Path::new("Fixture Game"));
    root.assert_clean();
}

#[tokio::test]
async fn recorded_supported_steps_run_in_place_before_deleting_the_game() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let mut record = install(&root, &worker).await.into_record();
    record.uninstall_steps = vec![Step::WriteFile {
        path: "uninstall-marker".into(),
        contents: b"ran".to_vec(),
    }];
    let manifest = InstallManifest::new(record).unwrap();
    manifest.write(&root.path().join("Fixture Game")).unwrap();
    assert!(!root.path().join("Fixture Game/uninstall-marker").exists());
    let report = Uninstaller::new(worker)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert!(report.steps_completed);
    assert!(report.is_clean());
    assert!(!root.path().join("Fixture Game").exists());
    root.assert_clean();
    // The installed tree (including the generated marker) is removed together.
    assert_eq!(
        fs::read(root.sentinel_path()).unwrap(),
        b"caskade-test-sentinel\n"
    );
}

#[tokio::test]
async fn copy_and_writefile_install_steps_are_both_reversed_by_directory_removal() {
    let root = TestRoot::new().unwrap();
    let worker = executor();
    let (manifest, _) = install_extra(
        &root,
        &worker,
        vec![Step::Copy {
            from: caskade_installer::SourcePath::Game("bin/FixtureGame".into()),
            to: "data/copied.bin".into(),
        }],
    )
    .await;
    let mut record = manifest.into_record();
    let mut copied = record.files[0].clone();
    copied.path = ManifestPath::new("data/copied.bin").unwrap();
    record.files.push(copied);
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    assert_eq!(
        fs::read(root.path().join("Fixture Game/data/copied.bin")).unwrap(),
        b"game"
    );
    assert!(
        manifest
            .verify(&root.path().join("Fixture Game"))
            .unwrap()
            .is_ok()
    );
    Uninstaller::new(worker)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn recorded_delegate_commands_use_the_configured_runtime_without_inverse_commands() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::Exit { code: 0 }],
    }]));
    let worker = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    );
    let mut record = install(&root, &worker).await.into_record();
    record.uninstall_steps = vec![Step::Delegate {
        tool: "flatpak".into(),
        args: vec!["uninstall".into(), "fixture".into()],
        uninstall: vec![],
        only_if: None,
    }];
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    Uninstaller::new(worker)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    let processes = runtime.processes();
    assert_eq!(processes.len(), 1);
    assert_eq!(processes[0].command.program, Path::new("flatpak"));
    assert_eq!(processes[0].command.args, ["uninstall", "fixture"]);
    root.assert_clean();
}
