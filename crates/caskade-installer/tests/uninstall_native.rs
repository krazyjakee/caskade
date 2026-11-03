use caskade_installer::{
    PlanSet, Program, RegistryValue, RuntimeSpec, SourcePath, Step, Strategy,
    exec::{CancellationToken, ExecRequest, Executor},
    manifest::native::{
        NativeFamily, NativeRegistry, NativeUninstallPolicy, NativeUninstaller, RegistryEffect,
        UninstallRegistration, detect_native_uninstaller,
    },
    manifest::{
        ExternalEffect, ExternalLocation, InstallManifest, InstalledFile, ManifestPath,
        RelocationVerdict, Sha256Digest, UninstallRequest, Uninstaller,
    },
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::Path,
    sync::{Arc, Mutex},
};
const CODE: &str = "{F78B4949-77EC-441F-B1E9-9673F41E1A18}";
fn location(key: &str) -> ExternalLocation {
    ExternalLocation {
        scope: "registry.hkcu.32".into(),
        path: ManifestPath::new(key).unwrap(),
    }
}
fn registration(game: &Path, family: NativeFamily) -> UninstallRegistration {
    UninstallRegistration {
        location: location(&format!(
            "Software/Microsoft/Windows/CurrentVersion/Uninstall/{}",
            if family == NativeFamily::Msi {
                CODE
            } else {
                "Fixture"
            }
        )),
        install_location: game.to_string_lossy().into(),
        uninstall_string: Some(if family == NativeFamily::Msi {
            format!("msiexec.exe /x {CODE} /unexpected")
        } else {
            format!(
                "\"{}\" /unexpected",
                game.join(if family == NativeFamily::Inno {
                    "unins000.exe"
                } else {
                    "Uninstall.exe"
                })
                .display()
            )
        }),
        quiet_uninstall_string: None,
        windows_installer: family == NativeFamily::Msi,
    }
}
#[derive(Default)]
struct Registry {
    entries: Mutex<Vec<UninstallRegistration>>,
    values: Mutex<BTreeMap<String, RegistryValue>>,
    refuse: bool,
}
impl NativeRegistry for Registry {
    fn uninstall_entries(&self) -> io::Result<Vec<UninstallRegistration>> {
        Ok(self.entries.lock().unwrap().clone())
    }
    fn remove_uninstall_entry(&self, entry: &UninstallRegistration) -> io::Result<()> {
        if self.refuse {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "registration changed or access denied",
            ));
        }
        self.entries
            .lock()
            .unwrap()
            .retain(|current| current != entry);
        Ok(())
    }
    fn undo_effect(&self, effect: &RegistryEffect) -> io::Result<()> {
        effect.validate()?;
        match effect {
            RegistryEffect::Value {
                location,
                name,
                original,
                ..
            } => {
                let key = format!("{}/{}", location.path.as_str(), name);
                let mut values = self.values.lock().unwrap();
                if let Some(value) = original {
                    values.insert(key, value.clone());
                } else {
                    values.remove(&key);
                }
                Ok(())
            }
            RegistryEffect::UninstallKey {
                location,
                fingerprint,
            } => {
                let mut entries = self.entries.lock().unwrap();
                if entries
                    .iter()
                    .any(|entry| entry.location == *location && entry.fingerprint() != *fingerprint)
                {
                    return Err(io::Error::other("registration changed"));
                }
                entries.retain(|entry| entry.location != *location);
                Ok(())
            }
            RegistryEffect::Key { location } => {
                if self
                    .values
                    .lock()
                    .unwrap()
                    .keys()
                    .any(|key| key.starts_with(&format!("{}/", location.path.as_str())))
                {
                    Err(io::Error::other("key has unrecorded user data"))
                } else {
                    Ok(())
                }
            }
        }
    }
}
fn worker(runtime: Arc<FakeRuntime>) -> Executor {
    Executor::new(
        runtime,
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
}
fn request(root: &TestRoot, manifest: InstallManifest) -> UninstallRequest {
    UninstallRequest {
        job_id: "native-uninstall".into(),
        root: root.path().into(),
        relative_dir: "Fixture Game".into(),
        manifest,
    }
}
fn save(root: &TestRoot, manifest: InstallManifest) -> InstallManifest {
    manifest.write(&root.path().join("Fixture Game")).unwrap();
    manifest
}
async fn install(root: &TestRoot, executor: &Executor, family: NativeFamily) -> InstallManifest {
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.plan.relative_dir = "Fixture Game".into();
    record.launch_targets.clear();
    record.files.clear();
    record.external_effects.clear();
    record.uninstall_steps.clear();
    let program = if family == NativeFamily::Inno {
        "unins000.exe"
    } else {
        "Uninstall.exe"
    };
    // Explicit FakeRuntime unit payload, not a purported real installer fixture.
    let bytes = b"MZfake-uninstaller-for-runtime-seam".to_vec();
    record.plan.candidate.steps = vec![Step::WriteFile {
        path: "game.dat".into(),
        contents: b"payload".to_vec(),
    }];
    if family != NativeFamily::Msi {
        record.plan.candidate.steps.push(Step::WriteFile {
            path: program.into(),
            contents: bytes.clone(),
        });
    }
    let report = executor
        .execute(
            ExecRequest {
                job_id: "install-native-seam".into(),
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
    record.plan.candidate = report.candidate;
    record.plan.candidate.strategy = Strategy::RunSilent;
    record.runtime = RuntimeSpec::Native;
    if family != NativeFamily::Msi {
        record.files.push(InstalledFile {
            path: ManifestPath::new(program).unwrap(),
            size: bytes.len() as u64,
            sha256: Some(Sha256Digest::new(format!("{:x}", Sha256::digest(&bytes))).unwrap()),
        });
    }
    let native = NativeUninstaller {
        family,
        program: if family == NativeFamily::Msi {
            Program::Tool("msiexec".into())
        } else {
            Program::Source(SourcePath::Game(program.into()))
        },
        runtime: RuntimeSpec::Native,
        product_code: (family == NativeFamily::Msi).then(|| CODE.into()),
    };
    record.uninstall_steps = vec![native.step().unwrap()];
    save(root, InstallManifest::new(record).unwrap())
}
#[tokio::test]
async fn all_families_run_presets_then_remove_registry_shortcuts_and_manifest_leftovers() {
    for family in [NativeFamily::Inno, NativeFamily::Nsis, NativeFamily::Msi] {
        let root = TestRoot::new().unwrap();
        let runtime = Arc::new(FakeRuntime::new([FakeScript {
            actions: vec![
                FakeAction::WriteFile {
                    path: "left-by-native".into(),
                    contents: b"leftover".to_vec(),
                },
                FakeAction::Exit { code: 0 },
            ],
        }]));
        let executor = worker(runtime.clone());
        let mut record = install(&root, &executor, family).await.into_record();
        let registry = Arc::new(Registry::default());
        registry
            .entries
            .lock()
            .unwrap()
            .push(registration(&root.path().join("Fixture Game"), family));
        registry.values.lock().unwrap().insert(
            "Software/Fixture/InstallPath".into(),
            RegistryValue::String("new".into()),
        );
        registry.values.lock().unwrap().insert(
            "Software/Fixture/Existing".into(),
            RegistryValue::String("modified".into()),
        );
        record.external_effects = vec![
            ExternalEffect::Registry {
                effect: RegistryEffect::Value {
                    location: location("Software/Fixture"),
                    name: "InstallPath".into(),
                    original: None,
                    installed: None,
                },
            },
            ExternalEffect::Registry {
                effect: RegistryEffect::Value {
                    location: location("Software/Fixture"),
                    name: "Existing".into(),
                    original: Some(RegistryValue::String("old user value".into())),
                    installed: None,
                },
            },
        ];
        let desktop = tempfile::tempdir().unwrap();
        fs::write(desktop.path().join("game.lnk"), b"shortcut").unwrap();
        record.external_effects.push(ExternalEffect::File {
            location: ExternalLocation {
                scope: "desktop".into(),
                path: ManifestPath::new("game.lnk").unwrap(),
            },
            original: None,
        });
        let manifest = save(&root, InstallManifest::new(record).unwrap());
        let json = serde_json::to_vec(&manifest).unwrap();
        let manifest = InstallManifest::from_json(&json).unwrap();
        let report = Uninstaller::new(executor)
            .with_native_registry(registry.clone())
            .with_external_roots(BTreeMap::from([("desktop".into(), desktop.path().into())]))
            .uninstall(request(&root, manifest), CancellationToken::new())
            .await
            .unwrap();
        assert!(report.is_clean());
        assert!(report.native.unwrap().succeeded);
        assert_eq!(report.external_effects_reverted, 3);
        root.assert_clean();
        assert!(!desktop.path().join("game.lnk").exists());
        assert!(registry.entries.lock().unwrap().is_empty());
        let values = registry.values.lock().unwrap();
        assert_eq!(
            values.get("Software/Fixture/Existing"),
            Some(&RegistryValue::String("old user value".into()))
        );
        assert!(!values.contains_key("Software/Fixture/InstallPath"));
        let processes = runtime.processes();
        assert_eq!(processes.len(), 1);
        let args = &processes[0].command.args;
        match family {
            NativeFamily::Inno => {
                assert_eq!(
                    args.as_slice(),
                    ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]
                )
            }
            NativeFamily::Nsis => {
                assert_eq!(args[0], "/S");
                assert!(args.last().unwrap().starts_with("_?="));
                assert!(!args.last().unwrap().contains('"'));
            }
            NativeFamily::Msi => assert_eq!(args.as_slice(), ["/x", CODE, "/qn", "/norestart"]),
        }
    }
}
#[tokio::test]
async fn failed_or_timed_out_native_uninstaller_falls_back_and_reports_the_attempt() {
    for action in [FakeAction::Exit { code: 42 }, FakeAction::Hang] {
        let root = TestRoot::new().unwrap();
        let runtime = Arc::new(FakeRuntime::new([FakeScript {
            actions: vec![action],
        }]));
        let executor = worker(runtime.clone());
        let manifest = install(&root, &executor, NativeFamily::Inno).await;
        let report = Uninstaller::new(executor)
            .with_native_registry(Arc::new(Registry::default()))
            .uninstall(request(&root, manifest), CancellationToken::new())
            .await
            .unwrap();
        assert!(!report.steps_completed);
        let native = report.native.unwrap();
        assert!(native.attempted && !native.succeeded && native.manifest_fallback);
        assert!(!native.diagnostics.is_empty());
        root.assert_clean();
        assert_eq!(runtime.processes().len(), 1);
    }
}
#[tokio::test]
async fn stale_missing_or_changed_uninstallers_and_relocation_use_manifest_only() {
    for mode in ["missing", "changed", "rewritable", "fragile", "policy"] {
        let root = TestRoot::new().unwrap();
        let runtime = Arc::new(FakeRuntime::default());
        let executor = worker(runtime.clone());
        let mut manifest = install(&root, &executor, NativeFamily::Nsis).await;
        match mode {
            "missing" => fs::remove_file(root.path().join("Fixture Game/Uninstall.exe")).unwrap(),
            "changed" => fs::write(
                root.path().join("Fixture Game/Uninstall.exe"),
                b"MZtampered-uninstaller-for-runtime-s",
            )
            .unwrap(),
            "rewritable" | "fragile" => {
                let mut record = manifest.into_record();
                record.relocation.verdict = if mode == "fragile" {
                    RelocationVerdict::Fragile
                } else {
                    RelocationVerdict::Rewritable
                };
                manifest = save(&root, InstallManifest::new(record).unwrap());
            }
            _ => {}
        }
        let uninstaller =
            Uninstaller::new(executor).with_native_registry(Arc::new(Registry::default()));
        let uninstaller = if mode == "policy" {
            uninstaller.with_native_policy(NativeUninstallPolicy::ManifestOnly)
        } else {
            uninstaller
        };
        let report = uninstaller
            .uninstall(request(&root, manifest), CancellationToken::new())
            .await
            .unwrap();
        assert!(report.native.unwrap().manifest_fallback);
        assert!(runtime.processes().is_empty());
        root.assert_clean();
    }
}
#[tokio::test]
async fn registry_detection_is_scoped_and_never_reuses_arbitrary_registry_arguments() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript::default()]));
    let executor = worker(runtime.clone());
    let mut record = install(&root, &executor, NativeFamily::Inno)
        .await
        .into_record();
    record.uninstall_steps.clear();
    let owned = registration(&root.path().join("Fixture Game"), NativeFamily::Inno);
    let mut foreign = owned.clone();
    foreign.install_location.push_str("-foreign");
    let registry = Arc::new(Registry::default());
    *registry.entries.lock().unwrap() = vec![foreign.clone(), owned];
    let manifest = save(&root, InstallManifest::new(record).unwrap());
    assert_eq!(
        detect_native_uninstaller(
            &root.path().join("Fixture Game"),
            &manifest,
            &registry.entries.lock().unwrap()
        )
        .unwrap()
        .unwrap()
        .family,
        NativeFamily::Inno
    );
    Uninstaller::new(executor)
        .with_native_registry(registry.clone())
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(*registry.entries.lock().unwrap(), vec![foreign]);
    assert!(
        !runtime.processes()[0]
            .command
            .args
            .iter()
            .any(|arg| arg == "/unexpected")
    );
    root.assert_clean();
}
#[tokio::test]
async fn stale_msi_registration_skips_msiexec_and_registry_cleanup_failure_retains_retry_manifest()
{
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::default());
    let executor = worker(runtime.clone());
    let manifest = install(&root, &executor, NativeFamily::Msi).await;
    let report = Uninstaller::new(executor)
        .with_native_registry(Arc::new(Registry::default()))
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert!(report.native.unwrap().manifest_fallback);
    assert!(runtime.processes().is_empty());
    root.assert_clean();
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript::default()]));
    let executor = worker(runtime);
    let manifest = install(&root, &executor, NativeFamily::Inno).await;
    let registry = Arc::new(Registry {
        refuse: true,
        ..Registry::default()
    });
    registry.entries.lock().unwrap().push(registration(
        &root.path().join("Fixture Game"),
        NativeFamily::Inno,
    ));
    let error = Uninstaller::new(executor)
        .with_native_registry(registry)
        .uninstall(request(&root, manifest.clone()), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(!error.report().unwrap().game_removed);
    assert_eq!(
        InstallManifest::read(&root.path().join("Fixture Game")).unwrap(),
        manifest
    );
    root.assert_sentinel_untouched();
}
#[tokio::test]
async fn cancellation_never_becomes_destructive_native_failure_fallback() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::default());
    let executor = worker(runtime.clone());
    let manifest = install(&root, &executor, NativeFamily::Inno).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(
        Uninstaller::new(executor)
            .uninstall(request(&root, manifest.clone()), cancel)
            .await
            .is_err()
    );
    assert!(runtime.processes().is_empty());
    assert_eq!(
        InstallManifest::read(&root.path().join("Fixture Game")).unwrap(),
        manifest
    );
}
#[test]
fn registry_inverse_records_reject_unscoped_hives_and_preserve_unknown_user_values() {
    for scope in ["registry.hkcr.32", "registry.hkcu", "registry.hklm.99"] {
        let effect = RegistryEffect::Key {
            location: ExternalLocation {
                scope: scope.into(),
                path: ManifestPath::new("Software/Game").unwrap(),
            },
        };
        assert!(effect.validate().is_err());
    }
    assert!(
        RegistryEffect::Key {
            location: location("System/CurrentControlSet/Game")
        }
        .validate()
        .is_err()
    );
    let registry = Registry::default();
    registry.values.lock().unwrap().insert(
        "Software/Fixture/UserSave".into(),
        RegistryValue::String("keep".into()),
    );
    assert!(
        registry
            .undo_effect(&RegistryEffect::Key {
                location: location("Software/Fixture")
            })
            .is_err()
    );
    assert!(!registry.values.lock().unwrap().is_empty());
}
#[cfg(unix)]
#[tokio::test]
async fn leaf_symlink_uninstaller_is_stale_and_its_target_is_preserved() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::default());
    let executor = worker(runtime.clone());
    let manifest = install(&root, &executor, NativeFamily::Inno).await;
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("unins000.exe");
    fs::write(&target, b"MZprivate").unwrap();
    let path = root.path().join("Fixture Game/unins000.exe");
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    Uninstaller::new(executor)
        .uninstall(request(&root, manifest), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(fs::read(target).unwrap(), b"MZprivate");
    assert!(runtime.processes().is_empty());
    root.assert_clean();
}

#[cfg(windows)]
#[path = "uninstall_native/windows.rs"]
mod windows;

#[tokio::test]
async fn recorded_uninstall_key_survives_a_move_without_recording_an_absolute_path() {
    for changed in [false, true] {
        let root = TestRoot::new().unwrap();
        let runtime = Arc::new(FakeRuntime::default());
        let executor = worker(runtime.clone());
        let mut record = install(&root, &executor, NativeFamily::Inno)
            .await
            .into_record();
        let registry = Arc::new(Registry::default());
        registry.entries.lock().unwrap().push(registration(
            &root.path().join("Fixture Game"),
            NativeFamily::Inno,
        ));
        record.external_effects =
            caskade_installer::manifest::native::uninstall_registration_effects(
                &root.path().join("Fixture Game"),
                &*registry,
            )
            .unwrap();
        assert_eq!(record.external_effects.len(), 1);
        let json = serde_json::to_string(&record.external_effects).unwrap();
        assert!(!json.contains(&root.path().to_string_lossy().replace('\\', "\\\\")));
        // Simulate a registration still naming the pre-move path. The identity
        // effect remains stable and permits only an unchanged snapshot.
        record.relocation.verdict = RelocationVerdict::Fragile;
        let original = registry.entries.lock().unwrap()[0].clone();
        if changed {
            registry.entries.lock().unwrap()[0].quiet_uninstall_string =
                Some("foreign replacement".into());
        }
        fs::rename(
            root.path().join("Fixture Game"),
            root.path().join("Moved Game"),
        )
        .unwrap();
        record.plan.relative_dir = "Moved Game".into();
        let manifest = InstallManifest::new(record).unwrap();
        manifest.write(&root.path().join("Moved Game")).unwrap();
        let mut uninstall_request = request(&root, manifest);
        uninstall_request.relative_dir = "Moved Game".into();
        let result = Uninstaller::new(executor)
            .with_native_registry(registry.clone())
            .uninstall(uninstall_request, CancellationToken::new())
            .await;
        if changed {
            assert!(result.is_err());
            assert!(root.path().join("Moved Game").exists());
            assert_ne!(registry.entries.lock().unwrap()[0], original);
        } else {
            assert!(result.unwrap().is_clean());
            assert!(registry.entries.lock().unwrap().is_empty());
            root.assert_clean();
        }
        assert!(runtime.processes().is_empty());
    }
}
