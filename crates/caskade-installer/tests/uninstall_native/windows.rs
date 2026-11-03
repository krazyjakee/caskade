use super::*;
use caskade_installer::manifest::native::PlatformRegistry;
use caskade_installer::manifest::{ManifestSource, effects_from_journal};
use caskade_installer::runtime::{NativeRuntime, OutOfRootWatcher, alias::AliasStore};
use caskade_installer::{ArtifactId, Candidate, Fidelity, HandlerId, Requirements};
use winreg::{RegKey, enums::*};

/// Native acceptance gate: this test intentionally performs genuine per-user
/// installs only when the dedicated Windows runner opts in. Linux tests exercise
/// the same lifecycle with injected runtime and registry seams.
#[tokio::test]
async fn genuine_inno_nsis_msi_native_install_uninstall_remove_files_registry_and_uninstall_key() {
    if std::env::var_os("CASKADE_TEST_NATIVE").is_none() {
        return;
    }
    for (family, id) in [
        (NativeFamily::Inno, "native-inno-shortcuts"),
        (NativeFamily::Nsis, "native-nsis-uninstall"),
        (NativeFamily::Msi, "native-msi-uninstall"),
    ] {
        let label = match family {
            NativeFamily::Inno => "inno",
            NativeFamily::Nsis => "nsis",
            NativeFamily::Msi => "msi",
        };
        let registry_key = format!("Software\\CaskadeNativeFixture\\{label}");
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        assert!(
            hkcu.open_subkey_with_flags(&registry_key, KEY_READ | KEY_WOW64_32KEY)
                .is_err(),
            "fixture registry key already exists; use a clean dedicated runner"
        );
        let root = TestRoot::new().unwrap();
        let alias_home = tempfile::tempdir().unwrap();
        let aliases = AliasStore::new(alias_home.path().join("g"));
        let watcher = OutOfRootWatcher::for_host();
        let executor = Executor::new(
            Arc::new(NativeRuntime::new()),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .with_aliases(aliases.clone(), "root-fixture-0001")
        .with_observer(Arc::new(watcher.clone()));
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            // By component: msiexec reads a '/' in its package path as a switch
            // (exit 1619).
            .join("tests")
            .join("fixtures")
            .join("bin")
            .join("native-uninstall")
            .join(id)
            .join(if family == NativeFamily::Msi {
                "setup.msi"
            } else {
                "setup.exe"
            });
        let from = SourcePath::Artifact {
            artifact: ArtifactId("source".into()),
            path: ".".into(),
        };
        let (dir_args, program) = match family {
            NativeFamily::Inno => (
                vec![
                    "/VERYSILENT".into(),
                    "/SUPPRESSMSGBOXES".into(),
                    "/NORESTART".into(),
                    "/SP-".into(),
                    format!("/DIR={}", caskade_installer::exec::INSTALL_DIR),
                ],
                Program::Source(from),
            ),
            NativeFamily::Nsis => (
                vec![
                    "/S".into(),
                    format!("/D={}", caskade_installer::exec::INSTALL_DIR),
                ],
                Program::Source(from),
            ),
            NativeFamily::Msi => (
                vec![
                    "/i".into(),
                    source.to_string_lossy().into(),
                    "/qn".into(),
                    "/norestart".into(),
                    format!("INSTALLDIR={}", caskade_installer::exec::INSTALL_DIR),
                ],
                Program::Tool("msiexec".into()),
            ),
        };
        let candidate = Candidate {
            handler: HandlerId(label.into()),
            strategy: Strategy::RunSilent,
            confidence: 1.0,
            fidelity: Fidelity::Native,
            requires: Requirements {
                executes_code: true,
                runtime: Some(RuntimeSpec::Native),
                may_prompt_elevation: false,
                interactive: false,
                est_disk_bytes: None,
            },
            steps: vec![Step::Run {
                program,
                args: dir_args,
                env: BTreeMap::new(),
                runtime: RuntimeSpec::Native,
                timeout_secs: 300,
                expect: vec!["FixtureGame.exe".into()],
            }],
            warnings: vec![],
        };
        let report = executor
            .execute(
                ExecRequest {
                    job_id: format!("native-{label}"),
                    root: root.path().into(),
                    relative_dir: "Fixture Game".into(),
                    plans: PlanSet {
                        candidates: vec![candidate],
                        selected: Some(0),
                        question: None,
                    },
                    sources: BTreeMap::from([(ArtifactId("source".into()), source.clone())]),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            report.alias.is_some(),
            "native fixture requires a stable alias"
        );
        let installed_key = hkcu
            .open_subkey_with_flags(&registry_key, KEY_READ | KEY_WOW64_32KEY)
            .unwrap();
        let install_path: String = installed_key.get_value("InstallPath").unwrap();
        assert!(!install_path.is_empty());
        drop(installed_key);
        let mut record =
            InstallManifest::from_json(include_bytes!("../manifests/install.v1.posix.json"))
                .unwrap()
                .into_record();
        record.title = id.into();
        record.source =
            ManifestSource::from_input(&caskade_installer::Input::Path(source), None, None)
                .unwrap();
        // The record names the handler that ran, as an install's would.
        record.handlers = vec![caskade_installer::manifest::HandlerVersion {
            id: report.candidate.handler.clone(),
            version: "0.1.0".into(),
        }];
        record.plan.candidate = report.candidate;
        record.plan.relative_dir = report.relative_dir;
        record.files.clear();
        record.launch_targets.clear();
        record.runtime = RuntimeSpec::Native;
        record.alias = report.alias;
        fn inventory(root: &Path, dir: &Path, files: &mut Vec<InstalledFile>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                if metadata.is_dir() {
                    inventory(root, &path, files)
                } else if metadata.is_file() {
                    let bytes = fs::read(&path).unwrap();
                    files.push(InstalledFile {
                        path: ManifestPath::new(path.strip_prefix(root).unwrap().to_str().unwrap())
                            .unwrap(),
                        size: bytes.len() as u64,
                        sha256: Some(
                            Sha256Digest::new(format!("{:x}", Sha256::digest(bytes))).unwrap(),
                        ),
                    });
                }
            }
        }
        inventory(&report.dir, &report.dir, &mut record.files);
        record.external_effects =
            effects_from_journal(&report.journal, watcher.external_roots()).unwrap();
        record.external_effects.extend(
            caskade_installer::manifest::native::uninstall_registration_effects(
                &report.dir,
                &PlatformRegistry,
            )
            .unwrap(),
        );
        record.external_effects.extend([
            ExternalEffect::Registry {
                effect: RegistryEffect::Key {
                    location: location(&registry_key.replace('\\', "/")),
                },
            },
            ExternalEffect::Registry {
                effect: RegistryEffect::Value {
                    location: location(&registry_key.replace('\\', "/")),
                    name: "InstallPath".into(),
                    original: None,
                    installed: None,
                },
            },
        ]);
        let program = if family == NativeFamily::Msi {
            Program::Tool("msiexec".into())
        } else {
            Program::Source(SourcePath::Game(
                if family == NativeFamily::Inno {
                    "unins000.exe"
                } else {
                    "Uninstall.exe"
                }
                .into(),
            ))
        };
        record.uninstall_steps = vec![
            NativeUninstaller {
                family,
                program,
                runtime: RuntimeSpec::Native,
                product_code: (family == NativeFamily::Msi).then(|| CODE.into()),
            }
            .step()
            .unwrap(),
        ];
        let manifest = save(&root, InstallManifest::new(record).unwrap());
        let before = PlatformRegistry.uninstall_entries().unwrap();
        let matched: Vec<_> = before
            .iter()
            .filter(|entry| {
                dunce::canonicalize(&entry.install_location)
                    .is_ok_and(|location| location == dunce::canonicalize(&report.dir).unwrap())
            })
            .cloned()
            .collect();
        assert!(
            !matched.is_empty(),
            "native installer registered no owned Uninstall key"
        );
        let result = Uninstaller::new(executor)
            .with_aliases(aliases)
            .with_external_roots(watcher.external_roots().clone())
            .uninstall(request(&root, manifest), CancellationToken::new())
            .await
            .unwrap();
        assert!(result.native.as_ref().unwrap().succeeded, "{result:?}");
        assert!(result.is_clean());
        root.assert_clean();
        assert!(
            hkcu.open_subkey_with_flags(&registry_key, KEY_READ | KEY_WOW64_32KEY)
                .is_err()
        );
        let after = PlatformRegistry.uninstall_entries().unwrap();
        for entry in matched {
            assert!(
                !after
                    .iter()
                    .any(|current| current.location == entry.location)
            );
        }
    }
}
