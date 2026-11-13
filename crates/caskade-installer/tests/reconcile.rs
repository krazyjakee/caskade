use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use caskade_installer::{
    Input, Question, RuntimeSpec, Strategy,
    exec::{CancellationToken, Executor},
    manifest::{
        InstallManifest, InstalledFile, ManifestPath, RelocationVerdict, RootManifest,
        Sha256Digest, UninstallRequest, Uninstaller,
    },
    reconcile::{ReconcileError, ReconcileRequest, ReconcileStatus, Reconciler, ScanLimits},
    relocate::{RegistryStore, RelocateError, Relocator, RunningCheck, ShortcutEditor},
    runtime::alias::AliasStore,
    scan::{self, ScanInput},
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink},
};
use sha2::{Digest, Sha256};

type Key = (String, String, String);
#[derive(Default)]
struct Registry {
    values: Mutex<BTreeMap<Key, scan::RegistryValue>>,
    fail_once: AtomicBool,
}
impl RegistryStore for Registry {
    fn read(&self, hive: &str, key: &str, name: &str) -> io::Result<Option<scan::RegistryValue>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&(hive.into(), key.into(), name.into()))
            .cloned())
    }
    fn apply(&self, writes: &[scan::RegistryValue]) -> io::Result<()> {
        for write in writes {
            self.values.lock().unwrap().insert(
                (write.hive.clone(), write.key.clone(), write.name.clone()),
                write.clone(),
            );
        }
        if self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(io::Error::other("injected partial registry batch failure"));
        }
        Ok(())
    }
}
struct Shortcuts;
impl ShortcutEditor for Shortcuts {
    fn rewrite(&self, _: &Path, _: &str, _: &str) -> io::Result<()> {
        Err(io::Error::other("no shortcut editor in this test"))
    }
}
struct Running(AtomicBool);
impl RunningCheck for Running {
    fn running(&self, _: &Path) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
fn worker(registry: Arc<dyn RegistryStore>) -> Reconciler {
    Reconciler::new(Arc::new(Relocator {
        disk: Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink: Arc::new(RecordingSink::default()),
        registry,
        shortcuts: Arc::new(Shortcuts),
        running: Arc::new(Running(AtomicBool::new(false))),
    }))
}
fn root(parent: &Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    fs::create_dir(&root).unwrap();
    RootManifest::new("root-reconcile-1")
        .unwrap()
        .write(&root)
        .unwrap();
    dunce::canonicalize(root).unwrap()
}
fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::new(format!("{:x}", Sha256::digest(bytes))).unwrap()
}
fn save_files(manifest: InstallManifest, game: &Path) -> InstallManifest {
    let mut record = manifest.into_record();
    record.files = ["config.ini", "payload.bin", "unins000.exe"]
        .into_iter()
        .filter(|path| game.join(path).exists())
        .map(|path| {
            let bytes = fs::read(game.join(path)).unwrap();
            InstalledFile {
                path: ManifestPath::new(path).unwrap(),
                size: bytes.len() as u64,
                sha256: Some(digest(&bytes)),
            }
        })
        .collect();
    InstallManifest::new(record).unwrap()
}
fn game(root: &Path, name: &str, verdict: RelocationVerdict) -> PathBuf {
    let game = root.join(name);
    fs::create_dir(&game).unwrap();
    let config = if matches!(
        verdict,
        RelocationVerdict::Free | RelocationVerdict::Aliased
    ) {
        "[game]\r\nrelative=payload.bin\r\n".into()
    } else {
        format!("[game]\r\npath={}\r\n", game.display())
    };
    fs::write(game.join("config.ini"), config).unwrap();
    fs::write(game.join("payload.bin"), b"authored payload\0").unwrap();
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.plan.root_id = "root-reconcile-1".into();
    record.plan.relative_dir = name.into();
    record.launch_targets.clear();
    record.uninstall_steps.clear();
    record.external_effects.clear();
    record.alias = None;
    record.relocation.verdict = verdict;
    record.relocation.last_path = Some(game.to_string_lossy().into_owned());
    record.relocation.references = scan::scan_references(&ScanInput {
        game_dir: &game,
        real_path: &game.to_string_lossy(),
        alias_path: None,
        real_variants: &[],
        registry: &[],
        external_files: &[],
        delegated: false,
        limits: Default::default(),
    })
    .unwrap()
    .references;
    save_files(InstallManifest::new(record).unwrap(), &game)
        .write(&game)
        .unwrap();
    game
}
fn assert_manifest_clean(game: &Path) -> InstallManifest {
    let manifest = InstallManifest::read(game).unwrap();
    let verification = manifest.verify(game).unwrap();
    assert!(
        verification.missing.is_empty() && verification.changed.is_empty(),
        "{verification:?}"
    );
    assert_eq!(
        manifest.record().relocation.last_path.as_deref(),
        dunce::canonicalize(game).unwrap().to_str()
    );
    manifest
}

#[tokio::test]
async fn explorer_rename_is_repaired_on_next_scan_without_moving_files_again() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let before = game(&root, "Old Game", RelocationVerdict::Rewritable);
    let after = root.join("Renamed Café Game");
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(&before).unwrap().ino()
    };
    fs::rename(&before, &after).unwrap(); // Explorer, not the relocate engine.
    let scanner = worker(Arc::new(Registry::default()));
    let report = scanner
        .scan_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.games.len(), 1);
    assert_eq!(report.games[0].status, ReconcileStatus::Reconciled);
    assert_eq!(report.games[0].rewritten_files, 1);
    assert!(!before.exists());
    assert_eq!(
        fs::read_to_string(after.join("config.ini")).unwrap(),
        format!("[game]\r\npath={}\r\n", after.display())
    );
    let manifest = assert_manifest_clean(&after);
    assert_eq!(
        manifest.record().plan.relative_dir,
        Path::new("Renamed Café Game")
    );
    assert_eq!(
        manifest.record().relocation.verdict,
        RelocationVerdict::Rewritable
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(&after).unwrap().ino(), inode);
    }
    assert_eq!(
        scanner
            .scan_root(&root, CancellationToken::new())
            .await
            .unwrap()
            .games[0]
            .status,
        ReconcileStatus::Unchanged
    );
}

#[tokio::test]
async fn root_mount_path_change_reconciles_every_game_preserving_root_identity() {
    let home = tempfile::tempdir().unwrap();
    let old_root = root(home.path(), "drive-before");
    game(&old_root, "One", RelocationVerdict::Rewritable);
    game(&old_root, "Two", RelocationVerdict::Free);
    let new_root = home.path().join("drive-after");
    fs::rename(&old_root, &new_root).unwrap();
    let report = worker(Arc::new(Registry::default()))
        .scan_root(&new_root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.root_id, "root-reconcile-1");
    assert_eq!(report.games.len(), 2);
    for entry in report.games {
        assert_eq!(entry.status, ReconcileStatus::Reconciled, "{entry:?}");
        let manifest = assert_manifest_clean(&new_root.join(entry.relative_dir));
        assert_eq!(manifest.record().plan.root_id, report.root_id);
    }
    assert!(!old_root.exists());
}

#[tokio::test]
async fn aliases_are_retargeted_after_external_moves_and_created_when_missing() {
    for missing in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let root = root(home.path(), "library");
        let old = game(&root, "Old", RelocationVerdict::Aliased);
        let store = AliasStore::new(home.path().join("aliases"));
        let mut record = InstallManifest::read(&old).unwrap().into_record();
        record.alias = Some("external-move".into());
        InstallManifest::new(record).unwrap().write(&old).unwrap();
        if !missing {
            store.link("external-move", &old).unwrap();
        }
        let new = root.join("New");
        fs::rename(&old, &new).unwrap();
        let report = worker(Arc::new(Registry::default()))
            .with_aliases(store.clone())
            .scan_root(&root, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            report.games[0].status,
            ReconcileStatus::Reconciled,
            "{report:?}"
        );
        assert_eq!(store.target("external-move").unwrap(), Some(new.clone()));
        assert_manifest_clean(&new);
    }
}

#[tokio::test]
async fn copied_game_requires_choice_instead_of_stealing_the_original_alias() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let old = game(&root, "Original", RelocationVerdict::Aliased);
    let store = AliasStore::new(home.path().join("aliases"));
    let mut record = InstallManifest::read(&old).unwrap().into_record();
    record.alias = Some("copied-game".into());
    InstallManifest::new(record).unwrap().write(&old).unwrap();
    store.link("copied-game", &old).unwrap();
    let new = root.join("Copy");
    fs::create_dir_all(new.join(".caskade")).unwrap();
    for path in ["config.ini", "payload.bin", ".caskade/install.json"] {
        fs::copy(old.join(path), new.join(path)).unwrap();
    }
    let original = fs::read(new.join("config.ini")).unwrap();
    let report = worker(Arc::new(Registry::default()))
        .with_aliases(store.clone())
        .scan_root(&root, CancellationToken::new())
        .await
        .unwrap();
    let copied = report
        .games
        .iter()
        .find(|g| g.relative_dir == Path::new("Copy"))
        .unwrap();
    assert_eq!(copied.status, ReconcileStatus::NeedsUser);
    assert!(matches!(
        copied.question,
        Some(Question::ChooseInstallDirectory { .. })
    ));
    assert_eq!(store.target("copied-game").unwrap(), Some(old));
    assert_eq!(fs::read(new.join("config.ini")).unwrap(), original);
}

#[tokio::test]
async fn fragile_reference_is_not_rewritten_and_offers_source_url_reinstallation() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let old = game(&root, "Old", RelocationVerdict::Fragile);
    let binary = [b"\0binary\0".as_slice(), old.to_str().unwrap().as_bytes()].concat();
    fs::write(old.join("payload.bin"), &binary).unwrap();
    let mut record = InstallManifest::read(&old).unwrap().into_record();
    let source = Input::Urls(vec!["https://example.test/kept-game.zip".parse().unwrap()]);
    record.source.input = source.clone();
    record.relocation.references.push(scan::Reference {
        location: scan::Location::GameFile {
            path: "payload.bin".into(),
            rewritable: false,
        },
        via: scan::Via::Real,
        encoding: scan::Encoding::Utf8,
    });
    save_files(InstallManifest::new(record).unwrap(), &old)
        .write(&old)
        .unwrap();
    let new = root.join("New");
    fs::rename(&old, &new).unwrap();
    let report = worker(Arc::new(Registry::default()))
        .scan_root(&root, CancellationToken::new())
        .await
        .unwrap();
    let entry = &report.games[0];
    assert_eq!(entry.status, ReconcileStatus::NeedsUser);
    assert!(entry.warnings.iter().any(|w| w.code == "reconcile_fragile"));
    assert!(
        matches!(&entry.question, Some(Question::ReinstallAfterMove {source: found, ..}) if *found==source)
    );
    assert_eq!(fs::read(new.join("payload.bin")).unwrap(), binary);
    assert_manifest_clean(&new);
}

#[tokio::test]
async fn limits_and_precancellation_happen_before_any_repairs() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let old = game(&root, "Old", RelocationVerdict::Rewritable);
    let new = root.join("New");
    fs::rename(old, &new).unwrap();
    let before = fs::read(new.join("config.ini")).unwrap();
    for name in ["A", "B", "C"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let scanner = worker(Arc::new(Registry::default())).with_limits(ScanLimits {
        max_entries: 2,
        max_depth: 4,
    });
    assert!(matches!(
        scanner.scan_root(&root, CancellationToken::new()).await,
        Err(ReconcileError::Limit(2))
    ));
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(scanner.scan_root(&root, cancel).await.is_err());
    assert_eq!(fs::read(new.join("config.ini")).unwrap(), before);
}

#[tokio::test]
async fn web_game_bundles_are_skipped_whole() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let bundle = root.join("Needle");
    fs::create_dir_all(bundle.join("files")).unwrap();
    fs::write(bundle.join("caskade-web.json"), "{}").unwrap();
    for n in 0..50 {
        fs::write(bundle.join("files").join(format!("{n:032x}")), "x").unwrap();
    }
    let scanner = worker(Arc::new(Registry::default())).with_limits(ScanLimits {
        max_entries: 10,
        max_depth: 4,
    });
    let report = scanner
        .scan_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[tokio::test]
async fn failed_registry_repair_restores_files_manifest_and_the_original_dangling_alias() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let old = game(&root, "Old", RelocationVerdict::Rewritable);
    let store = AliasStore::new(home.path().join("aliases"));
    store.link("rollback-alias", &old).unwrap();
    let registry = Arc::new(Registry::default());
    let value = scan::RegistryValue {
        hive: "HKCU".into(),
        key: "Software\\CaskadeReconcile".into(),
        name: "Path".into(),
        value_type: "REG_SZ".into(),
        data: old
            .to_str()
            .unwrap()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
    };
    registry.apply(std::slice::from_ref(&value)).unwrap();
    registry.fail_once.store(true, Ordering::SeqCst);
    let mut record = InstallManifest::read(&old).unwrap().into_record();
    record.alias = Some("rollback-alias".into());
    record.relocation.references.push(scan::Reference {
        location: scan::Location::Registry {
            hive: value.hive.clone(),
            key: value.key.clone(),
            name: value.name.clone(),
            value_type: value.value_type.clone(),
            rewritable: true,
        },
        via: scan::Via::Real,
        encoding: scan::Encoding::Utf16Le,
    });
    InstallManifest::new(record).unwrap().write(&old).unwrap();
    let new = root.join("New");
    fs::rename(&old, &new).unwrap();
    let manifest_bytes = fs::read(new.join(".caskade/install.json")).unwrap();
    let ini = fs::read(new.join("config.ini")).unwrap();
    let failure = worker(registry.clone())
        .with_aliases(store.clone())
        .reconcile(
            ReconcileRequest {
                root: root.clone(),
                relative_dir: "New".into(),
                old_dir: None,
                job_id: "failed-reconcile".into(),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            failure,
            ReconcileError::Repair(RelocateError::RolledBack(_))
        ),
        "{failure}"
    );
    assert_eq!(fs::read(new.join("config.ini")).unwrap(), ini);
    assert_eq!(
        fs::read(new.join(".caskade/install.json")).unwrap(),
        manifest_bytes
    );
    assert_eq!(
        registry.read(&value.hive, &value.key, &value.name).unwrap(),
        Some(value)
    );
    assert_eq!(store.target("rollback-alias").unwrap(), Some(old.clone()));
    assert!(!old.exists());
    assert!(new.exists());
}

#[tokio::test]
async fn missing_legacy_anchor_requires_an_explicit_old_location() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let old = game(&root, "Old", RelocationVerdict::Rewritable);
    let mut record = InstallManifest::read(&old).unwrap().into_record();
    record.relocation.last_path = None;
    InstallManifest::new(record).unwrap().write(&old).unwrap();
    let new = root.join("New");
    fs::rename(&old, &new).unwrap();
    let scanner = worker(Arc::new(Registry::default()));
    assert_eq!(
        scanner
            .scan_root(&root, CancellationToken::new())
            .await
            .unwrap()
            .games[0]
            .status,
        ReconcileStatus::NeedsUser
    );
    let report = scanner
        .reconcile(
            ReconcileRequest {
                root,
                relative_dir: "New".into(),
                old_dir: Some(old.to_string_lossy().into_owned()),
                job_id: "legacy".into(),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(report.status, ReconcileStatus::Reconciled);
    assert_manifest_clean(&new);
}

#[tokio::test]
async fn moved_free_and_aliased_games_use_native_uninstall_but_rewritten_and_fragile_use_manifest()
{
    for (verdict, tampered) in [
        (RelocationVerdict::Free, false),
        (RelocationVerdict::Aliased, false),
        (RelocationVerdict::Rewritable, false),
        (RelocationVerdict::Fragile, false),
        (RelocationVerdict::Free, true),
        (RelocationVerdict::Unscanned, false),
    ] {
        let home = tempfile::tempdir().unwrap();
        let root = root(home.path(), "library");
        let old = game(&root, "Old", verdict);
        fs::write(
            old.join("unins000.exe"),
            b"MZauthored native uninstaller stub",
        )
        .unwrap();
        let store = AliasStore::new(home.path().join("aliases"));
        let mut record = InstallManifest::read(&old).unwrap().into_record();
        record.plan.candidate.handler = caskade_installer::HandlerId("inno".into());
        record.plan.candidate.strategy = Strategy::RunSilent;
        record.handlers = vec![caskade_installer::manifest::HandlerVersion {
            id: record.plan.candidate.handler.clone(),
            version: "test-1".into(),
        }];
        record.runtime = RuntimeSpec::Native;
        record.uninstall_steps = vec![caskade_installer::Step::Run {
            program: caskade_installer::Program::Source(caskade_installer::SourcePath::Game(
                "unins000.exe".into(),
            )),
            args: vec![
                "/VERYSILENT".into(),
                "/SUPPRESSMSGBOXES".into(),
                "/NORESTART".into(),
            ],
            env: BTreeMap::new(),
            runtime: RuntimeSpec::Native,
            timeout_secs: 30,
            expect: vec![],
        }];
        if verdict == RelocationVerdict::Aliased {
            record.alias = Some("uninstall-alias".into());
            store.link("uninstall-alias", &old).unwrap();
        }
        save_files(InstallManifest::new(record).unwrap(), &old)
            .write(&old)
            .unwrap();
        if tampered {
            fs::write(
                old.join("unins000.exe"),
                b"MZchanged uninstaller must not be reapproved",
            )
            .unwrap();
        }
        let new = root.join("New");
        fs::rename(old, &new).unwrap();
        let report = worker(Arc::new(Registry::default()))
            .with_aliases(store.clone())
            .scan_root(&root, CancellationToken::new())
            .await
            .unwrap();
        assert_ne!(
            report.games[0].status,
            ReconcileStatus::Failed,
            "{report:?}"
        );
        if tampered {
            assert!(
                !InstallManifest::read(&new)
                    .unwrap()
                    .verify(&new)
                    .unwrap()
                    .changed
                    .is_empty(),
                "reconcile blessed an unrelated changed uninstaller"
            );
        }
        let runtime = Arc::new(FakeRuntime::new([FakeScript {
            actions: vec![FakeAction::Exit { code: 0 }],
        }]));
        let executor = Executor::new(
            runtime.clone(),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        );
        let uninstall = Uninstaller::new(executor)
            .with_aliases(store.clone())
            .uninstall(
                UninstallRequest {
                    job_id: "uninstall-after-move".into(),
                    root: root.clone(),
                    relative_dir: "New".into(),
                    manifest: InstallManifest::read(&new).unwrap(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let native = !tampered
            && matches!(
                verdict,
                RelocationVerdict::Free | RelocationVerdict::Aliased
            );
        assert_eq!(
            runtime.processes().len(),
            usize::from(native),
            "{verdict:?}: {uninstall:?}"
        );
        let attempt = uninstall.native.unwrap();
        assert_eq!(attempt.succeeded, native);
        assert_eq!(attempt.manifest_fallback, !native);
        assert!(!new.exists());
        assert!(store.target("uninstall-alias").unwrap().is_none());
    }
}

#[tokio::test]
async fn unsafe_manifest_references_fail_before_any_write_and_external_roots_are_explicit() {
    for target in ["../outside.ini", ".caskade/install.json"] {
        let home = tempfile::tempdir().unwrap();
        let root = root(home.path(), "library");
        let old = game(&root, "Old", RelocationVerdict::Rewritable);
        let outside = root.join("outside.ini");
        fs::write(&outside, old.to_str().unwrap()).unwrap();
        let mut record = InstallManifest::read(&old).unwrap().into_record();
        record.relocation.references.push(scan::Reference {
            location: scan::Location::GameFile {
                path: target.into(),
                rewritable: true,
            },
            via: scan::Via::Real,
            encoding: scan::Encoding::Utf8,
        });
        InstallManifest::new(record).unwrap().write(&old).unwrap();
        let new = root.join("New");
        fs::rename(&old, &new).unwrap();
        let ini = fs::read(new.join("config.ini")).unwrap();
        let metadata = fs::read(new.join(".caskade/install.json")).unwrap();
        let report = worker(Arc::new(Registry::default()))
            .scan_root(&root, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(report.games[0].status, ReconcileStatus::Failed);
        assert_eq!(fs::read(new.join("config.ini")).unwrap(), ini);
        assert_eq!(
            fs::read(new.join(".caskade/install.json")).unwrap(),
            metadata
        );
        assert_eq!(fs::read_to_string(outside).unwrap(), old.to_str().unwrap());
    }
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let old = game(&root, "Old", RelocationVerdict::Rewritable);
    let outside = home.path().join("external");
    fs::create_dir(&outside).unwrap();
    let config = outside.join("game.ini");
    fs::write(&config, old.to_str().unwrap()).unwrap();
    let mut record = InstallManifest::read(&old).unwrap().into_record();
    record.relocation.references.push(scan::Reference {
        location: scan::Location::ExternalFile {
            path: config.to_str().unwrap().into(),
            rewritable: true,
        },
        via: scan::Via::Real,
        encoding: scan::Encoding::Utf8,
    });
    InstallManifest::new(record).unwrap().write(&old).unwrap();
    let new = root.join("New");
    fs::rename(&old, &new).unwrap();
    let scanner = worker(Arc::new(Registry::default()));
    assert_eq!(
        scanner
            .scan_root(&root, CancellationToken::new())
            .await
            .unwrap()
            .games[0]
            .status,
        ReconcileStatus::Failed
    );
    assert_eq!(fs::read_to_string(&config).unwrap(), old.to_str().unwrap());
    let report = scanner
        .with_external_roots(vec![outside])
        .scan_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.games[0].status, ReconcileStatus::Reconciled);
    assert_eq!(fs::read_to_string(config).unwrap(), new.to_str().unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn game_and_reference_symlinks_never_grant_external_write_access() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let old = game(&root, "Old", RelocationVerdict::Rewritable);
    let new = root.join("New");
    fs::rename(&old, &new).unwrap();
    let external = home.path().join("outside.ini");
    fs::write(&external, old.to_str().unwrap()).unwrap();
    fs::remove_file(new.join("config.ini")).unwrap();
    std::os::unix::fs::symlink(&external, new.join("config.ini")).unwrap();
    std::os::unix::fs::symlink(home.path(), root.join("linked-directory")).unwrap();
    let report = worker(Arc::new(Registry::default()))
        .scan_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.games.len(), 1);
    assert_eq!(report.games[0].status, ReconcileStatus::Failed);
    assert_eq!(fs::read_to_string(external).unwrap(), old.to_str().unwrap());
}

#[tokio::test]
async fn authoritative_rescan_finds_new_game_references_missing_from_the_old_manifest() {
    let home = tempfile::tempdir().unwrap();
    let root = root(home.path(), "library");
    let old = game(&root, "Old", RelocationVerdict::Free);
    fs::write(old.join("config.ini"), old.to_str().unwrap()).unwrap();
    let manifest = save_files(InstallManifest::read(&old).unwrap(), &old);
    assert!(manifest.record().relocation.references.is_empty());
    manifest.write(&old).unwrap();
    let new = root.join("New");
    fs::rename(&old, &new).unwrap();
    let report = worker(Arc::new(Registry::default()))
        .scan_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.games[0].status, ReconcileStatus::Reconciled);
    assert_eq!(
        fs::read_to_string(new.join("config.ini")).unwrap(),
        new.to_str().unwrap()
    );
    let manifest = assert_manifest_clean(&new);
    assert_eq!(
        manifest.record().relocation.verdict,
        RelocationVerdict::Rewritable
    );
    assert!(manifest.record().relocation.references.iter().any(|reference|matches!(&reference.location,scan::Location::GameFile {path,..} if path=="config.ini")));
}
