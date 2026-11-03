#![cfg(feature = "windows-formats")]

use caskade_installer::{
    ArtifactNode, Candidate, Format, PlanSet, Strategy,
    exec::{CancellationToken, ExecRequest, Executor, Extractor},
    formats::windows::{
        inno::{InnoExtractor, InnoHandler},
        msi::{MsiExtractor, MsiHandler},
        nsis::{NsisExtractor, NsisHandler},
    },
    host::HostCaps,
    plan::{PlanEngine, Policy, WindowsMethod},
    probe::{ProbeEngine, ProbeLimits},
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot},
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

use crate::fixtures;

fn corpus() -> Vec<fixtures::Fixture> {
    fixtures::load_native_windows(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
}
fn probe(fixture: &fixtures::Fixture) -> ArtifactNode {
    ProbeEngine::with_builtin_formats(ProbeLimits::default())
        .probe_path(&fixture.input())
        .unwrap()
        .root
}
fn host(name: &str) -> HostCaps {
    toml::from_str(match name {
        "windows" => include_str!("hosts/windows.toml"),
        "linux" => include_str!("hosts/linux.toml"),
        "macos" => include_str!("hosts/macos.toml"),
        _ => unreachable!(),
    })
    .unwrap()
}
fn plans(node: &ArtifactNode, host: &HostCaps, extract: bool) -> PlanSet {
    let mut engine = PlanEngine::new();
    engine.register(Box::new(InnoHandler::new())).unwrap();
    engine.register(Box::new(NsisHandler::new())).unwrap();
    engine.register(Box::new(MsiHandler::new())).unwrap();
    engine
        .plan(
            std::slice::from_ref(node),
            host,
            &Policy {
                windows_method: if extract {
                    WindowsMethod::Extract
                } else {
                    WindowsMethod::RunInstaller
                },
                ..Default::default()
            },
        )
        .unwrap()
}
/// Authored fixture recipe flags. There is no generic NSIS/MSI shortcut switch.
fn fixture_shortcut_recipe(candidate: &mut Candidate) {
    use caskade_installer::Step;
    for step in &mut candidate.steps {
        let args = match step {
            Step::Run { args, .. } | Step::RunInteractive { args, .. } => args,
            _ => continue,
        };
        match candidate.handler.0.as_str() {
            "nsis" => {
                let index = args.iter().position(|arg| arg.starts_with("/D=")).unwrap();
                args.insert(index, "/CASKADE_NO_SHORTCUTS=1".into());
            }
            "msi" => args.push("CASKADE_NOSHORTCUTS=1".into()),
            "inno" => assert!(args.iter().any(|arg| arg == "/NOICONS")),
            _ => unreachable!(),
        }
    }
}
fn extractor(node: &ArtifactNode, source: &Path) -> Arc<dyn Extractor> {
    let sources = BTreeMap::from([(node.id.clone(), source.to_owned())]);
    match node.format {
        Format::Inno => Arc::new(InnoExtractor::new(sources)),
        Format::Nsis => Arc::new(NsisExtractor::new(sources)),
        Format::Msi => Arc::new(MsiExtractor::new(sources)),
        _ => unreachable!(),
    }
}
fn request(
    root: &TestRoot,
    fixture: &fixtures::Fixture,
    node: &ArtifactNode,
    plans: PlanSet,
) -> ExecRequest {
    ExecRequest {
        job_id: fixture.expected.id.clone(),
        root: root.path().into(),
        relative_dir: "Fixture Café Space".into(),
        plans,
        sources: BTreeMap::from([(node.id.clone(), fixture.input())]),
    }
}
fn assert_golden(fixture: &fixtures::Fixture, dir: &Path, native: bool) {
    let mut actual = fixtures::snapshot_tree(dir).unwrap();
    let mut expected = fixture.tree.clone();
    if native {
        let generated = &fixture.expected.windows.as_ref().unwrap().native_generated;
        for path in generated {
            assert!(dir.join(path).is_file(), "missing generated {path}");
        }
        actual.retain(|entry| !generated.contains(&entry.path));
    }
    if native || cfg!(windows) {
        for entry in actual.iter_mut().chain(expected.iter_mut()) {
            entry.executable = false;
        }
    }
    assert_eq!(
        actual, expected,
        "{} shared native/extract golden",
        fixture.expected.id
    );
}

#[tokio::test]
async fn every_native_acceptance_installer_extracts_to_the_shared_golden_tree() {
    let corpus = corpus();
    assert_eq!(corpus.len(), 6);
    for fixture in corpus {
        let node = probe(&fixture);
        let root = TestRoot::new().unwrap();
        let executor = Executor::new(
            Arc::new(FakeRuntime::default()),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .with_extractor(extractor(&node, &fixture.input()));
        let mut actual_host = HostCaps::detect();
        actual_host.filesystem.writable = true; // The test-created root is writable.
        let plans = plans(&node, &actual_host, true);
        assert!(
            plans.question.is_none(),
            "{}: {:?}",
            fixture.expected.id,
            plans.question
        );
        assert_eq!(
            plans.candidates[plans.selected.unwrap() as usize].strategy,
            Strategy::Extract
        );
        let report = executor
            .execute(
                request(&root, &fixture, &node, plans),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_golden(&fixture, &report.dir, false);
        fs::remove_dir_all(&report.dir).unwrap();
        root.assert_clean();
    }
}

#[test]
fn native_recipes_and_cross_host_plans_preserve_the_expected_fallbacks() {
    let mut snapshot = BTreeMap::new();
    for fixture in corpus() {
        let node = probe(&fixture);
        let mut matrix = BTreeMap::new();
        for name in ["windows", "linux", "macos"] {
            let mut candidates = plans(&node, &host(name), false);
            assert!(candidates.question.is_none());
            let selected = candidates.selected.unwrap() as usize;
            assert_eq!(
                candidates.candidates[selected].strategy,
                if name == "windows" {
                    Strategy::RunSilent
                } else {
                    Strategy::Extract
                }
            );
            for candidate in &mut candidates.candidates {
                fixture_shortcut_recipe(candidate);
            }
            matrix.insert(name, candidates);
        }
        snapshot.insert(fixture.expected.id, matrix);
    }
    insta::assert_json_snapshot!("native_windows_host_plans", snapshot);
}

#[cfg(feature = "cli")]
#[test]
fn t9_mini_installers_pass_and_malformed_inputs_fail_through_the_cli() {
    use caskade_installer::manifest::{InstallManifest, ROOT_MANIFEST_PATH};
    use serde_json::Value;
    use std::process::Command;
    fn clean(root: &TestRoot) {
        let identity = root.path().join(ROOT_MANIFEST_PATH);
        if identity.is_file() {
            fs::remove_file(identity).unwrap();
        }
        let state = root.path().join(".caskade");
        if state.exists() {
            fs::remove_dir(state).unwrap();
        }
        root.assert_clean();
    }
    for fixture in corpus() {
        let root = TestRoot::new().unwrap();
        let installed = Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .envs(caskade_installer::testing::windows_runner_path().map(|path| ("PATH", path)))
            .args([
                "--json",
                "install",
                fixture.input().to_str().unwrap(),
                "--extract",
                "--root",
                root.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            installed.status.success(),
            "{}: {} {}",
            fixture.expected.id,
            String::from_utf8_lossy(&installed.stdout),
            String::from_utf8_lossy(&installed.stderr)
        );
        let output: Value = serde_json::from_slice(&installed.stdout).unwrap();
        assert_eq!(output["type"], "installed");
        let game = root.path().join(output["value"]["dir"].as_str().unwrap());
        let manifest = InstallManifest::read(&game).unwrap();
        let verified = manifest.verify(&game).unwrap();
        assert!(verified.changed.is_empty() && verified.missing.is_empty());
        let actual: BTreeMap<_, _> = manifest
            .record()
            .files
            .iter()
            .map(|file| {
                (
                    file.path.as_str(),
                    (file.size, file.sha256.as_ref().unwrap().as_str()),
                )
            })
            .collect();
        let expected: BTreeMap<_, _> = fixture
            .tree
            .iter()
            .filter(|entry| entry.kind == fixtures::EntryKind::File)
            .map(|entry| {
                (
                    entry.path.as_str(),
                    (entry.size, entry.sha256.as_deref().unwrap()),
                )
            })
            .collect();
        assert_eq!(actual, expected);
        let removed = Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .envs(caskade_installer::testing::windows_runner_path().map(|path| ("PATH", path)))
            .args(["--json", "uninstall", game.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            removed.status.success(),
            "{}",
            String::from_utf8_lossy(&removed.stderr)
        );
        clean(&root);

        let scratch = tempfile::tempdir().unwrap();
        let truncated = scratch.path().join(fixture.input().file_name().unwrap());
        fs::write(&truncated, &fs::read(fixture.input()).unwrap()[..16]).unwrap();
        let root = TestRoot::new().unwrap();
        let rejected = Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .envs(caskade_installer::testing::windows_runner_path().map(|path| ("PATH", path)))
            .args([
                "--json",
                "install",
                truncated.to_str().unwrap(),
                "--extract",
                "--root",
                root.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            !rejected.status.success(),
            "malformed {} was accepted",
            fixture.expected.id
        );
        let value: Value = serde_json::from_slice(&rejected.stdout).unwrap();
        assert!(
            matches!(value["type"].as_str(), Some("error" | "unsupported")),
            "{value}"
        );
        clean(&root);
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use caskade_installer::{
        Input, InstallPlan, RuntimeSpec,
        manifest::{
            ExternalEffect, ExternalLocation, HandlerVersion, InstallManifest, InstalledFile,
            ManifestPath, ManifestSource, Sha256Digest, UninstallRequest, Uninstaller,
            native::{
                NativeRegistry, PlatformRegistry, RegistryEffect, uninstall_registration_effects,
            },
        },
        runtime::{NativeRuntime, ProcessCommand, Runtime, alias::AliasStore},
    };
    use std::{path::PathBuf, time::Duration};
    use winreg::{RegKey, enums::*};

    fn product_code(node: &ArtifactNode) -> Option<String> {
        (node.format == Format::Msi).then(|| {
            caskade_installer::formats::windows::msi::metadata_from_artifact(node)
                .unwrap()
                .properties["ProductCode"]
                .clone()
        })
    }
    fn uninstall_key(fixture: &fixtures::Fixture, code: Option<&str>) -> String {
        match fixture.expected.format.as_str() {
            "inno" => format!("{}_is1", fixture.expected.id),
            "msi" => code.unwrap().into(),
            "nsis" => fixture.expected.id.clone(),
            _ => unreachable!(),
        }
    }
    fn shortcut(id: &str) -> PathBuf {
        PathBuf::from(std::env::var_os("APPDATA").expect("Windows APPDATA"))
            .join("Microsoft/Windows/Start Menu/Programs")
            .join(format!("Caskade T7 {id}.lnk"))
    }
    fn default_fixture_dir(id: &str) -> PathBuf {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("Windows LOCALAPPDATA"))
            .join("CaskadeT7")
            .join(id)
    }
    fn assert_registry_absent(custom: &str, uninstall: &str) {
        for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
            let path =
                format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{uninstall}");
            for (hive, path) in [
                (HKEY_CURRENT_USER, custom),
                (HKEY_CURRENT_USER, path.as_str()),
                (HKEY_LOCAL_MACHINE, path.as_str()),
            ] {
                match RegKey::predef(hive).open_subkey_with_flags(path, KEY_READ | view) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    result => panic!(
                        "fixture key must be absent and readable at preflight: {path}, {result:?}"
                    ),
                }
            }
        }
    }
    /// Even an assertion failure gets a bounded native uninstall and removal of
    /// only this fixture's keys/shortcut (pre-existing keys are refused first).
    struct Cleanup {
        id: String,
        dir: PathBuf,
        fallback_dir: PathBuf,
        code: Option<String>,
        uninstall: String,
        alias: Option<String>,
        aliases: AliasStore,
        armed: bool,
    }
    impl Cleanup {
        fn clean(&mut self) {
            if !self.armed {
                return;
            }
            // Incorrect /D serialization can make NSIS use its authored default.
            // It was checked absent before launch and belongs only to this fixture.
            let cleanup_dir = if !self.dir.join("Uninstall.exe").is_file()
                && self.fallback_dir.join("Uninstall.exe").is_file()
            {
                &self.fallback_dir
            } else {
                &self.dir
            };
            let program = if self.code.is_some() {
                Some(PathBuf::from("msiexec"))
            } else {
                ["unins000.exe", "Uninstall.exe"]
                    .into_iter()
                    .map(|name| cleanup_dir.join(name))
                    .find(|path| path.is_file())
            };
            if let Some(program) = program {
                let family = if self.code.is_some() {
                    caskade_installer::plan::presets::InstallerFamily::Msi
                } else if program
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("unins")
                {
                    caskade_installer::plan::presets::InstallerFamily::Inno
                } else {
                    caskade_installer::plan::presets::InstallerFamily::Nsis
                };
                let args = caskade_installer::plan::presets::build_uninstall_args(
                    family,
                    self.code.as_deref(),
                )
                .unwrap()
                .into_iter()
                .map(|arg| {
                    arg.replace(
                        caskade_installer::exec::INSTALL_DIR,
                        &cleanup_dir.to_string_lossy(),
                    )
                })
                .collect();
                let runtime = NativeRuntime::new();
                if let Ok(pid) = runtime.spawn(&ProcessCommand {
                    program,
                    args,
                    env: BTreeMap::new(),
                    cwd: std::env::temp_dir(),
                }) {
                    let _ = runtime.wait(pid, Duration::from_secs(30));
                    let _ = runtime.kill(pid);
                }
            }
            // The fallback location was absent at preflight. Removing only this
            // test-owned directory also covers interrupted NSIS self-deletion.
            let _ = fs::remove_dir_all(&self.fallback_dir);
            let _ = fs::remove_file(shortcut(&self.id));
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
                if let Ok(parent) =
                    hkcu.open_subkey_with_flags("Software\\CaskadeT7", KEY_ALL_ACCESS | view)
                {
                    let _ = parent.delete_subkey_all(&self.id);
                }
                for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
                    if let Ok(parent) = RegKey::predef(hive).open_subkey_with_flags(
                        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                        KEY_ALL_ACCESS | view,
                    ) {
                        let _ = parent.delete_subkey_all(&self.uninstall);
                    }
                }
            }
            if let Some(id) = &self.alias {
                let _ = self.aliases.remove(id);
            }
            self.armed = false;
        }
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.clean();
        }
    }

    struct ObservedRuntime {
        native: NativeRuntime,
        started: std::sync::Mutex<Option<std::sync::mpsc::Sender<(u32, ProcessCommand)>>>,
    }
    impl Runtime for ObservedRuntime {
        fn spawn(
            &self,
            command: &ProcessCommand,
        ) -> Result<caskade_installer::runtime::ProcessId, caskade_installer::runtime::RuntimeError>
        {
            let id = self.native.spawn(command)?;
            let pid = self.native.system_pid(id)?;
            if let Some(sender) = self.started.lock().unwrap().take() {
                let _ = sender.send((pid, command.clone()));
            }
            Ok(id)
        }
        fn wait(
            &self,
            id: caskade_installer::runtime::ProcessId,
            timeout: Duration,
        ) -> Result<caskade_installer::runtime::ProcessExit, caskade_installer::runtime::RuntimeError>
        {
            self.native.wait(id, timeout)
        }
        fn kill(
            &self,
            id: caskade_installer::runtime::ProcessId,
        ) -> Result<(), caskade_installer::runtime::RuntimeError> {
            self.native.kill(id)
        }
    }
    fn os_arguments(pid: u32) -> Option<Vec<String>> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut system = System::new();
        let pid = Pid::from_u32(pid);
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < until {
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
            );
            if let Some(process) = system.process(pid)
                && process.cmd().len() > 1
            {
                return Some(
                    process
                        .cmd()
                        .iter()
                        .skip(1)
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect(),
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }
    fn expected_os_arguments(handler: &str, args: &[String]) -> Vec<String> {
        if handler == "nsis" {
            assert!(args.last().unwrap().starts_with("/D="));
            let mut expected = args[..args.len() - 1].to_vec();
            // NSIS consumes the complete raw /D= tail. CommandLineToArgvW,
            // which sysinfo uses, splits its intentionally unquoted spaces.
            expected.extend(
                args.last()
                    .unwrap()
                    .split_ascii_whitespace()
                    .map(str::to_owned),
            );
            expected
        } else {
            args.iter()
                .map(|arg| {
                    if handler == "inno"
                        && let Some(dir) = arg
                            .strip_prefix("/DIR=\"")
                            .and_then(|dir| dir.strip_suffix('"'))
                    {
                        format!("/DIR={dir}")
                    } else {
                        arg.clone()
                    }
                })
                .collect()
        }
    }
    fn save_command_line_report(reports: &[serde_json::Value]) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target/native-windows");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("interactive-argv.json"),
            serde_json::to_vec_pretty(reports).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    #[ignore = "OS-reported installer command lines; dedicated CASKADE_TEST_NATIVE=1 Windows runner"]
    async fn interactive_presets_match_sysinfo_command_lines_then_cancel_cleanly() {
        if std::env::var("CASKADE_TEST_NATIVE").as_deref() != Ok("1") {
            return;
        }
        let mut reports = vec![];
        for fixture in corpus() {
            let node = probe(&fixture);
            let code = product_code(&node);
            let custom = format!("Software\\CaskadeT7\\{}", fixture.expected.id);
            let uninstall = uninstall_key(&fixture, code.as_deref());
            assert_registry_absent(&custom, &uninstall);
            assert!(!shortcut(&fixture.expected.id).exists());
            let fallback_dir = default_fixture_dir(&fixture.expected.id);
            assert!(
                !fallback_dir.exists(),
                "fixture default directory already exists"
            );
            let root = TestRoot::new().unwrap();
            let alias_home = tempfile::tempdir().unwrap();
            let aliases = AliasStore::new(alias_home.path().join("Café Alias Space"));
            let (send, receive) = std::sync::mpsc::channel();
            let runtime = Arc::new(ObservedRuntime {
                native: NativeRuntime::new(),
                started: std::sync::Mutex::new(Some(send)),
            });
            let executor = Executor::new(
                runtime,
                Arc::new(FakeDiskSpace::fixed(u64::MAX)),
                Arc::new(RecordingSink::default()),
            )
            .with_aliases(aliases.clone(), "root-fixture-0001");
            let mut candidate = plans(&node, &host("windows"), false)
                .candidates
                .into_iter()
                .find(|candidate| candidate.strategy == Strategy::RunInteractive)
                .unwrap();
            fixture_shortcut_recipe(&mut candidate);
            let handler = candidate.handler.0.clone();
            let plan = PlanSet {
                candidates: vec![candidate],
                selected: Some(0),
                question: None,
            };
            let mut cleanup = Cleanup {
                id: fixture.expected.id.clone(),
                dir: root.path().join("Fixture Café Space"),
                fallback_dir,
                code,
                uninstall: uninstall.clone(),
                alias: None,
                aliases,
                armed: true,
            };
            let request = request(&root, &fixture, &node, plan);
            let cancel = CancellationToken::new();
            let token = cancel.clone();
            let running = tokio::spawn(async move { executor.execute(request, token).await });
            let observed = tokio::task::spawn_blocking(move || {
                receive
                    .recv_timeout(Duration::from_secs(10))
                    .map(|(pid, command)| (pid, os_arguments(pid), command))
            })
            .await
            .unwrap();
            cancel.cancel();
            let finished = running.await.unwrap();
            if let Ok(report) = &finished {
                cleanup.alias = report.alias.clone();
            }
            cleanup.clean();
            if cleanup.dir.exists() {
                fs::remove_dir_all(&cleanup.dir).unwrap();
            }
            root.assert_clean();
            assert!(!cleanup.fallback_dir.exists());
            assert_registry_absent(&custom, &uninstall);
            assert!(!shortcut(&fixture.expected.id).exists());
            let (pid, actual, command) = observed.expect("real installer did not start");
            let expected = expected_os_arguments(&handler, &command.args);
            reports.push(serde_json::json!({"fixture":fixture.expected.id, "version":fixture.expected.windows.as_ref().unwrap().version,
                "pid":pid, "preset_argv":command.args, "expected_os_argv":expected, "os_argv":actual, "clean":true,
                "completed_before_cancel":finished.is_ok()}));
            save_command_line_report(&reports);
            assert_eq!(
                actual.as_ref(),
                Some(&expected),
                "OS argument serialization for {}",
                fixture.expected.id
            );
            assert!(!command.args.iter().any(|arg| matches!(
                arg.to_ascii_lowercase().as_str(),
                "/s" | "/qn" | "/verysilent" | "/silent"
            )));
            assert!(
                matches!(finished, Err(caskade_installer::exec::ExecError::Cancelled))
                    || handler == "msi" && finished.is_ok(),
                "interactive wizard did not cancel cleanly: {finished:?}"
            );
        }
    }

    #[tokio::test]
    #[ignore = "genuine per-user Windows installers; dedicated CASKADE_TEST_NATIVE=1 runner only"]
    async fn silent_inno5_inno6_nsis2_nsis3_and_both_msi_cabs_match_goldens_and_uninstall_cleanly()
    {
        if std::env::var("CASKADE_TEST_NATIVE").as_deref() != Ok("1") {
            return;
        }
        for fixture in corpus() {
            // Names the installer if one hangs until the runtime's timeout.
            eprintln!("native silent install: {}", fixture.expected.id);
            let node = probe(&fixture);
            let code = product_code(&node);
            let custom = format!("Software\\CaskadeT7\\{}", fixture.expected.id);
            let uninstall = uninstall_key(&fixture, code.as_deref());
            assert_registry_absent(&custom, &uninstall);
            assert!(!shortcut(&fixture.expected.id).exists());
            let fallback_dir = default_fixture_dir(&fixture.expected.id);
            assert!(
                !fallback_dir.exists(),
                "fixture default directory already exists"
            );
            let root = TestRoot::new().unwrap();
            let alias_home = tempfile::tempdir().unwrap();
            let aliases = AliasStore::new(alias_home.path().join("Café Alias Space"));
            let mut record =
                InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
                    .unwrap()
                    .into_record();
            let root_id = record.plan.root_id.clone();
            let executor = Executor::new(
                Arc::new(NativeRuntime::new()),
                Arc::new(FakeDiskSpace::fixed(u64::MAX)),
                Arc::new(RecordingSink::default()),
            )
            .with_aliases(aliases.clone(), root_id.clone());
            let mut plan = plans(&node, &host("windows"), false);
            let mut candidate = plan.candidates.remove(plan.selected.unwrap() as usize);
            assert_eq!(candidate.strategy, Strategy::RunSilent);
            fixture_shortcut_recipe(&mut candidate);
            plan = PlanSet {
                candidates: vec![candidate],
                selected: Some(0),
                question: None,
            };
            let dir = root.path().join("Fixture Café Space");
            let mut cleanup = Cleanup {
                id: fixture.expected.id.clone(),
                dir: dir.clone(),
                fallback_dir,
                code,
                uninstall: uninstall.clone(),
                alias: None,
                aliases: aliases.clone(),
                armed: true,
            };
            let report = executor
                .execute(
                    request(&root, &fixture, &node, plan),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            cleanup.alias = report.alias.clone();
            assert!(
                report.failures.is_empty(),
                "native failure must never hide behind an extraction fallback"
            );
            assert!(
                report.alias.is_some(),
                "NTFS native gate requires a stable alias"
            );
            assert_golden(&fixture, &report.dir, true);
            assert!(
                !shortcut(&fixture.expected.id).exists(),
                "shortcut opt-out was ineffective"
            );
            let registrations =
                uninstall_registration_effects(&report.dir, &PlatformRegistry).unwrap();
            assert!(
                !registrations.is_empty(),
                "{} created no owned Uninstall registration",
                fixture.expected.id
            );
            record.title = fixture.expected.title.clone();
            record.source =
                ManifestSource::from_input(&Input::Path(fixture.input()), None, None).unwrap();
            record.plan = InstallPlan {
                schema_version: 1,
                input: record.source.input.clone(),
                artifacts: vec![node.clone()],
                root_id,
                relative_dir: report.relative_dir.clone(),
                candidate: report.candidate.clone(),
            };
            record.handlers = vec![HandlerVersion {
                id: report.candidate.handler.clone(),
                version: env!("CARGO_PKG_VERSION").into(),
            }];
            record.files = fixtures::snapshot_tree(&report.dir)
                .unwrap()
                .into_iter()
                .filter(|entry| entry.kind == fixtures::EntryKind::File)
                .map(|entry| InstalledFile {
                    path: ManifestPath::new(entry.path).unwrap(),
                    size: entry.size,
                    sha256: Some(Sha256Digest::new(entry.sha256.unwrap()).unwrap()),
                })
                .collect();
            record.launch_targets.clear();
            record.runtime = RuntimeSpec::Native;
            record.uninstall_steps = caskade_installer::formats::windows::uninstall_steps(
                std::slice::from_ref(&node),
                &report.candidate,
            );
            assert_eq!(record.uninstall_steps.len(), 1);
            record.external_effects = registrations;
            let location = ExternalLocation {
                scope: "registry.hkcu.32".into(),
                path: ManifestPath::new(custom.replace('\\', "/")).unwrap(),
            };
            record.external_effects.extend([
                ExternalEffect::Registry {
                    effect: RegistryEffect::Key {
                        location: location.clone(),
                    },
                },
                ExternalEffect::Registry {
                    effect: RegistryEffect::Value {
                        location,
                        name: "InstallPath".into(),
                        original: None,
                        installed: None,
                    },
                },
            ]);
            record.alias = report.alias;
            let manifest = InstallManifest::new(record).unwrap();
            manifest.write(&report.dir).unwrap();
            let outcome = Uninstaller::new(executor)
                .with_aliases(aliases)
                .uninstall(
                    UninstallRequest {
                        job_id: format!("remove-{}", fixture.expected.id),
                        root: root.path().into(),
                        relative_dir: report.relative_dir,
                        manifest,
                    },
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert!(outcome.native.as_ref().unwrap().succeeded, "{outcome:?}");
            assert!(outcome.is_clean(), "{outcome:?}");
            assert_registry_absent(&custom, &uninstall);
            assert!(!shortcut(&fixture.expected.id).exists());
            assert!(!cleanup.fallback_dir.exists());
            cleanup.armed = false;
            root.assert_clean();
            assert!(
                PlatformRegistry
                    .uninstall_entries()
                    .unwrap()
                    .iter()
                    .all(|entry| !entry.location.path.as_str().ends_with(&uninstall))
            );
        }
    }
}
