//! I4 native lifecycle proof: genuine install -> real Proton launch -> own
//! uninstaller -> manifest leftovers cleanup. Runs only in the native profile.
#![cfg(all(target_os = "linux", feature = "fetch", feature = "windows-formats"))]
use caskade_installer::{
    exec::{CancellationToken, ExecRequest, Executor},
    host::HostCaps,
    manifest::{
        InstallManifest, InstalledFile, ManifestPath, ManifestSource, Sha256Digest,
        UninstallRequest, Uninstaller,
        native::{NativeFamily, NativeUninstaller},
    },
    runtime::{
        ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError,
        proton::{GAME_DIR, Launcher, PrefixRuntime},
        umu::UmuBootstrap,
    },
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
    *,
};
use sha2::{Digest, Sha256};
use std::result::Result;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
struct CheckedRuntime {
    inner: PrefixRuntime,
    commands: Mutex<BTreeMap<ProcessId, ProcessCommand>>,
    custom_key: String,
    uninstall_key: String,
    verified: Mutex<bool>,
}
impl CheckedRuntime {
    fn query(&self, game: &Path, key: &str, view: &str) -> Result<bool, RuntimeError> {
        let command = ProcessCommand {
            program: "reg.exe".into(),
            args: vec!["query".into(), key.into(), format!("/reg:{view}")],
            env: BTreeMap::new(),
            cwd: game.into(),
        };
        let process = self.inner.spawn(&command)?;
        let result = self.inner.wait(process, Duration::from_secs(300));
        self.inner.kill(process)?;
        Ok(result?.code == 0)
    }
}
impl Runtime for CheckedRuntime {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        let id = self.inner.spawn(command)?;
        self.commands.lock().unwrap().insert(id, command.clone());
        Ok(id)
    }
    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        let exit = self.inner.wait(id, timeout)?;
        let command = self.commands.lock().unwrap().remove(&id).unwrap();
        let name = command
            .program
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if exit.code == 0 && (name.starts_with("unins") || name == "uninstall.exe") {
            assert!(
                !command.cwd.join("FixtureGame.exe").exists(),
                "own uninstaller left core payload"
            );
            for key in [&self.custom_key, &self.uninstall_key] {
                for view in ["32", "64"] {
                    assert!(
                        !self.query(&command.cwd, key, view)?,
                        "own uninstaller left {key} view {view}"
                    );
                }
            }
            *self.verified.lock().unwrap() = true;
        }
        Ok(exit)
    }
    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        self.inner.kill(id)
    }
}
fn inventory(game: &Path, dir: &Path, files: &mut Vec<InstalledFile>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path == game.join(".caskade") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            inventory(game, &path, files)
        } else if metadata.is_file() {
            files.push(InstalledFile {
                path: ManifestPath::new(path.strip_prefix(game).unwrap().to_str().unwrap())
                    .unwrap(),
                size: metadata.len(),
                sha256: Some(
                    Sha256Digest::new(format!("{:x}", Sha256::digest(fs::read(path).unwrap())))
                        .unwrap(),
                ),
            });
        }
    }
}
async fn install_launch_remove(family: NativeFamily, label: &str, id: &str, entry: &str) {
    if std::env::var_os("CASKADE_TEST_NATIVE").is_none() {
        return;
    }
    let umu = UmuBootstrap::for_host().unwrap().ensure().await.unwrap();
    let host = HostCaps::detect();
    let proton = host.proton_builds.first().expect("cached UMU-Proton build");
    {
        let runtime = Arc::new(CheckedRuntime {
            inner: PrefixRuntime::new(Launcher::Umu {
                umu_run: umu.clone(),
                proton_path: proton.to_string_lossy().into_owned(),
                game_id: "umu-default".into(),
            }),
            commands: Mutex::default(),
            custom_key: format!(r"HKCU\Software\CaskadeNativeFixture\{label}"),
            uninstall_key: format!(
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{entry}"
            ),
            verified: Mutex::new(false),
        });
        let spec = RuntimeSpec::ProtonPrefix {
            id: "default".into(),
        };
        let root = TestRoot::new().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/bin/native-uninstall")
            .join(id)
            .join("setup.exe");
        let artifact = ArtifactId("native-setup".into());
        let args = if family == NativeFamily::Inno {
            vec![
                "/VERYSILENT".into(),
                "/SUPPRESSMSGBOXES".into(),
                "/NORESTART".into(),
                "/SP-".into(),
                "/NOICONS".into(),
                format!("/DIR={}", exec::INSTALL_DIR),
            ]
        } else {
            vec!["/S".into(), format!("/D={}", exec::INSTALL_DIR)]
        };
        let candidate = Candidate {
            handler: HandlerId(label.into()),
            strategy: Strategy::RunSilent,
            confidence: 1.0,
            fidelity: Fidelity::Native,
            requires: Requirements {
                executes_code: true,
                runtime: Some(spec.clone()),
                may_prompt_elevation: false,
                interactive: false,
                est_disk_bytes: None,
            },
            steps: vec![Step::Run {
                program: Program::Source(SourcePath::Artifact {
                    artifact: artifact.clone(),
                    path: ".".into(),
                }),
                args,
                env: BTreeMap::new(),
                runtime: spec.clone(),
                timeout_secs: 1800,
                expect: vec!["FixtureGame.exe".into()],
            }],
            warnings: vec![],
        };
        let mut worker = Executor::new(
            Arc::new(runtime::NativeRuntime::new()),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        );
        worker.register_runtime(spec.clone(), runtime.clone());
        let report = worker
            .execute(
                ExecRequest {
                    job_id: format!("proton-native-{label}"),
                    root: root.path().into(),
                    relative_dir: "Fixture Game".into(),
                    plans: PlanSet {
                        candidates: vec![candidate],
                        selected: Some(0),
                        question: None,
                    },
                    sources: BTreeMap::from([(artifact, source.clone())]),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        for key in [&runtime.custom_key, &runtime.uninstall_key] {
            assert!(
                runtime.query(&report.dir, key, "32").unwrap()
                    || runtime.query(&report.dir, key, "64").unwrap(),
                "installer omitted {key}"
            );
        }
        let launch = ProcessCommand {
            program: format!(r"{GAME_DIR}\FixtureGame.exe").into(),
            args: vec![
                "--token".into(),
                format!(r"{GAME_DIR}\launch-token"),
                "--check-z-drive".into(),
            ],
            env: BTreeMap::new(),
            cwd: report.dir.clone(),
        };
        let process = runtime.spawn(&launch).unwrap();
        let exit = runtime.wait(process, Duration::from_secs(600));
        runtime.kill(process).unwrap();
        assert_eq!(exit.unwrap().code, 0);
        assert!(
            fs::read_to_string(report.dir.join("launch-token"))
                .unwrap()
                .trim()
                .to_ascii_lowercase()
                .ends_with(r"\fixturegame.exe")
        );
        let mut record =
            InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
                .unwrap()
                .into_record();
        record.source =
            ManifestSource::from_input(&Input::Path(source.clone()), None, None).unwrap();
        record.title = id.into();
        record.plan.input = record.source.input.clone();
        let mut node =
            caskade_installer::probe::ProbeEngine::with_builtin_formats(Default::default())
                .probe_path(&source)
                .unwrap()
                .root;
        node.id = ArtifactId("native-setup".into());
        record.plan.artifacts = vec![node];
        record.handlers = vec![caskade_installer::manifest::HandlerVersion {
            id: report.candidate.handler.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
        }];
        record.plan.candidate = report.candidate;
        record.plan.relative_dir = report.relative_dir.clone();
        record.runtime = spec.clone();
        record.files.clear();
        record.launch_targets.clear();
        record.external_effects.clear();
        record.alias = report.alias;
        inventory(&report.dir, &report.dir, &mut record.files);
        record.uninstall_steps = vec![
            NativeUninstaller {
                family,
                program: Program::Source(SourcePath::Game(PathBuf::from(
                    if family == NativeFamily::Inno {
                        "unins000.exe"
                    } else {
                        "Uninstall.exe"
                    },
                ))),
                runtime: spec,
                product_code: None,
            }
            .step()
            .unwrap(),
        ];
        let manifest = InstallManifest::new(record).unwrap();
        manifest.write(&report.dir).unwrap();
        let removed = Uninstaller::new(worker)
            .uninstall(
                UninstallRequest {
                    job_id: format!("proton-remove-{label}"),
                    root: root.path().into(),
                    relative_dir: report.relative_dir,
                    manifest,
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(removed.native.as_ref().unwrap().succeeded, "{removed:?}");
        assert!(removed.is_clean());
        assert!(*runtime.verified.lock().unwrap());
        root.assert_clean();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn inno_installs_launches_and_own_uninstalls_under_proton() {
    install_launch_remove(
        NativeFamily::Inno,
        "inno",
        "native-inno-shortcuts",
        "CaskadeNativeInnoFixture_is1",
    )
    .await;
}
#[tokio::test(flavor = "multi_thread")]
async fn nsis_installs_launches_and_own_uninstalls_under_proton() {
    install_launch_remove(
        NativeFamily::Nsis,
        "nsis",
        "native-nsis-uninstall",
        "CaskadeNativeNSISFixture",
    )
    .await;
}
