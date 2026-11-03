//! T7 macOS runner-only tests. Inputs are genuine native tool output produced
//! by xtask/macos-fixtures.py; no synthetic filesystem image is accepted.
#![cfg(all(target_os = "macos", feature = "macos-formats"))]
use crate::fixtures;
use caskade_installer::{
    Input, InstallPlan, RuntimeSpec, Step, Strategy,
    exec::{CancellationToken, ExecRequest, Executor, Extractor},
    formats::macos::{
        app::{self, AppExtractor, AppHandler},
        dmg::{DmgExtractor, DmgHandler},
        pkg::{PkgExtractor, PkgHandler},
    },
    host::HostCaps,
    manifest::{
        HandlerVersion, InstallManifest, InstalledFile, ManifestPath, ManifestSource, Sha256Digest,
        UninstallRequest, Uninstaller,
    },
    plan::{PlanEngine, Policy},
    probe::{ProbeEngine, ProbeLimits},
    runtime::{NativeRuntime, ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError},
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};
fn artifact_root() -> PathBuf {
    let root=std::env::var_os("CASKADE_MACOS_FIXTURES").map(PathBuf::from).expect("build genuine native fixtures: python3 xtask/macos-fixtures.py --artifact target/macos-fixture-artifact; set CASKADE_MACOS_FIXTURES");
    let bytes = std::fs::read(root.join("native-build.json")).unwrap();
    assert!(bytes.len() < 4 * 1024 * 1024);
    let inventory: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(inventory["schema_version"], 1);
    assert!(inventory["binary_bytes"].as_u64().unwrap() <= 1_500_000);
    for (name, expected) in inventory["outputs"].as_object().unwrap() {
        fixtures::safe_relative(name).unwrap();
        let path = root.join(name);
        assert_eq!(
            path.metadata().unwrap().len(),
            expected["bytes"].as_u64().unwrap()
        );
        assert_eq!(
            fixtures::sha256_file(&path).unwrap(),
            expected["sha256"].as_str().unwrap()
        );
    }
    root
}
fn fixture(id: &str) -> fixtures::Fixture {
    fixtures::load_macos_native(&artifact_root())
        .unwrap()
        .into_iter()
        .find(|f| f.expected.id == id)
        .unwrap()
}
fn host() -> HostCaps {
    let mut host = HostCaps::detect();
    host.filesystem.writable = true;
    host
}
fn attached_images() -> BTreeSet<PathBuf> {
    let output = Command::new("/usr/bin/hdiutil")
        .args(["info", "-plist"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value = plist::Value::from_reader_xml(output.stdout.as_slice()).unwrap();
    value
        .as_dictionary()
        .unwrap()
        .get("images")
        .and_then(plist::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_dictionary()?.get("image-path")?.as_string())
        .map(|p| dunce::canonicalize(p).unwrap_or_else(|_| p.into()))
        .collect()
}
fn worker(extractor: Arc<dyn Extractor>) -> Executor {
    Executor::new(
        Arc::new(NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor)
}
async fn managed(id: &str, launch: bool, quarantine: bool) {
    let fixture = fixture(id);
    let source = dunce::canonicalize(fixture.input()).unwrap();
    let root = TestRoot::new().unwrap();
    let probe = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    let mut artifact = probe.probe_path(&source).unwrap().root;
    if fixture.expected.format == "zip" {
        let id = artifact.id.clone();
        probe
            .list_children(
                &mut std::fs::File::open(&source).unwrap(),
                &mut artifact,
                &id,
            )
            .unwrap();
    }
    let host = host();
    let sources = BTreeMap::from([(artifact.id.clone(), source.clone())]);
    let mut planner = PlanEngine::new();
    let extractor: Arc<dyn Extractor> = match fixture.expected.format.as_str() {
        "dmg" => {
            planner.register(Box::new(DmgHandler::new())).unwrap();
            assert!(!attached_images().contains(&source));
            Arc::new(DmgExtractor::new(
                sources.clone(),
                host.clone(),
                Arc::new(NativeRuntime::new()),
            ))
        }
        "pkg" => {
            planner.register(Box::new(PkgHandler::new())).unwrap();
            Arc::new(PkgExtractor::new(sources.clone()))
        }
        _ => {
            planner.register(Box::new(AppHandler::new())).unwrap();
            Arc::new(AppExtractor::new(sources.clone()))
        }
    };
    let plans = planner
        .plan(std::slice::from_ref(&artifact), &host, &Policy::default())
        .unwrap();
    assert!(plans.question.is_none());
    assert_eq!(
        plans.candidates[plans.selected.unwrap() as usize].strategy,
        Strategy::Extract
    );
    struct SourceQuarantine(PathBuf);
    impl Drop for SourceQuarantine {
        fn drop(&mut self) {
            let _ = xattr::remove(&self.0, "com.apple.quarantine");
        }
    }
    let _source_quarantine = if quarantine {
        xattr::set(
            &source,
            "com.apple.quarantine",
            b"0083;fixture;CaskadeNative;preserved",
        )
        .unwrap();
        Some(SourceQuarantine(source.clone()))
    } else {
        None
    };
    let worker = worker(extractor);
    let report = worker
        .execute(
            ExecRequest {
                job_id: id.into(),
                root: root.path().into(),
                relative_dir: "Native Game".into(),
                plans,
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    fixture.assert_tree(&report.dir).unwrap();
    if fixture.expected.format == "dmg" {
        assert!(
            !attached_images().contains(&source),
            "DMG remains attached after install"
        );
    }
    let framework = report
        .dir
        .join("FixtureGame.app/Contents/Frameworks/X.framework");
    assert_eq!(
        std::fs::read_link(framework.join("Versions/Current")).unwrap(),
        Path::new("A")
    );
    assert!(std::fs::read(framework.join("X")).is_ok());
    if quarantine {
        assert_eq!(
            xattr::get(report.dir.join("FixtureGame.app"), "com.apple.quarantine")
                .unwrap()
                .unwrap(),
            b"0083;fixture;CaskadeNative;preserved"
        );
    }
    let discovery = app::discover(&report.dir, &host).unwrap();
    assert_eq!(discovery.title, "Fixture Game");
    if launch {
        let token = root.path().join("token");
        let game = report.dir.join(&fixture.expected.launch_target);
        let magic = std::fs::read(&game).unwrap();
        assert!(matches!(
            &magic[..4],
            b"\xcf\xfa\xed\xfe" | b"\xfe\xed\xfa\xcf" | b"\xca\xfe\xba\xbe"
        ));
        let output = Command::new(game)
            .args(["--token", token.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            std::fs::read_to_string(&token)
                .unwrap()
                .trim()
                .ends_with("/FixtureGame")
        );
        std::fs::remove_file(token).unwrap();
    }
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.source = ManifestSource::from_input(&Input::Path(source.clone()), None, None).unwrap();
    record.plan = InstallPlan {
        schema_version: 1,
        input: Input::Path(source.clone()),
        artifacts: vec![artifact],
        root_id: "native-root".into(),
        relative_dir: report.relative_dir.clone(),
        candidate: report.candidate.clone(),
    };
    record.handlers = vec![HandlerVersion {
        id: report.candidate.handler,
        version: env!("CARGO_PKG_VERSION").into(),
    }];
    record.files = fixture
        .tree
        .iter()
        .filter(|e| e.kind == fixtures::EntryKind::File)
        .map(|e| InstalledFile {
            path: ManifestPath::new(&e.path).unwrap(),
            size: e.size,
            sha256: e.sha256.as_ref().map(|s| Sha256Digest::new(s).unwrap()),
        })
        .collect();
    record.launch_targets[0].path = fixture.expected.launch_target.into();
    record.launch_targets[0].working_dir = ".".into();
    record.runtime = RuntimeSpec::Native;
    record.launch_targets[0].runtime = RuntimeSpec::Native;
    record.uninstall_steps.clear();
    record.external_effects.clear();
    let manifest = InstallManifest::new(record).unwrap();
    manifest.write(&report.dir).unwrap();
    assert!(manifest.verify(&report.dir).unwrap().is_ok());
    let removed = Uninstaller::new(worker)
        .uninstall(
            UninstallRequest {
                job_id: format!("remove-{id}"),
                root: root.path().into(),
                relative_dir: report.relative_dir,
                manifest,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(removed.is_clean());
    root.assert_clean();
    if fixture.expected.format == "dmg" {
        assert!(!attached_images().contains(&source));
    }
}
#[tokio::test]
#[ignore = "native macOS T7 (#22/#44): genuine ditto/hdiutil/pkgbuild artifact, Mach-O launch and managed uninstall"]
async fn native_zip_udzo_ulfo_sla_and_pkg_install_launch_uninstall_clean() {
    for id in [
        "mac-native-app-zip",
        "mac-dmg-udzo",
        "mac-dmg-ulfo",
        "mac-dmg-sla",
        "mac-native-pkg",
    ] {
        managed(id, true, false).await;
    }
}
#[tokio::test]
#[ignore = "native macOS T7 (#22/#44): preserves quarantine without launching unsigned quarantined code"]
async fn native_dmg_quarantine_is_kept_then_managed_uninstall_is_clean() {
    managed("mac-dmg-udzo", false, true).await;
}
#[tokio::test]
#[ignore = "native macOS T7 (#22/#44): cancel actual DMG copy and assert detach/rollback"]
async fn native_dmg_cancelled_copy_detaches_and_rolls_back() {
    use caskade_installer::events::{EventSink, JobEvent};
    struct Cancel(CancellationToken);
    impl EventSink for Cancel {
        fn emit(&self, event: JobEvent) {
            if event.bytes > 0 {
                self.0.cancel();
            }
        }
    }
    let fixture = fixture("mac-dmg-sla");
    let source = dunce::canonicalize(fixture.input()).unwrap();
    assert!(!attached_images().contains(&source));
    let root = TestRoot::new().unwrap();
    let host = host();
    let node = ProbeEngine::with_builtin_formats(ProbeLimits::default())
        .probe_path(&source)
        .unwrap()
        .root;
    let sources = BTreeMap::from([(node.id.clone(), source.clone())]);
    let mut planner = PlanEngine::new();
    planner.register(Box::new(DmgHandler::new())).unwrap();
    let plans = planner.plan(&[node], &host, &Policy::default()).unwrap();
    let token = CancellationToken::new();
    let result = Executor::new(
        Arc::new(NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(Cancel(token.clone())),
    )
    .with_extractor(Arc::new(DmgExtractor::new(
        sources.clone(),
        host,
        Arc::new(NativeRuntime::new()),
    )))
    .execute(
        ExecRequest {
            job_id: "native-dmg-cancel".into(),
            root: root.path().into(),
            relative_dir: "Game".into(),
            plans,
            sources,
        },
        token,
    )
    .await;
    assert!(result.is_err());
    assert!(!attached_images().contains(&source));
    root.assert_clean();
}
struct RootRuntime {
    native: NativeRuntime,
    commands: Mutex<Vec<ProcessCommand>>,
}
impl Runtime for RootRuntime {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        assert!(matches!(
            command.program.to_str(),
            Some("/usr/sbin/installer" | "/usr/sbin/pkgutil")
        ));
        self.commands.lock().unwrap().push(command.clone());
        let mut sudo = command.clone();
        sudo.program = "/usr/bin/sudo".into();
        sudo.args
            .insert(0, command.program.to_string_lossy().into_owned());
        sudo.args.insert(0, "-n".into());
        self.native.spawn(&sudo)
    }
    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        self.native.wait(id, timeout)
    }
    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        self.native.kill(id)
    }
}
#[tokio::test]
#[ignore = "native macOS T7 (#22/#44): genuine pkgbuild/productbuild system install, receipt and scoped fixture cleanup"]
async fn native_system_and_product_packages_delegate_and_clean_their_fixture_receipts() {
    for id in ["mac-native-pkg-system", "mac-native-product-pkg"] {
        let fixture = fixture(id);
        let root = TestRoot::new().unwrap();
        let source = fixture.input();
        let directory = if id.ends_with("system") {
            PathBuf::from("/Library/CaskadePkgFixtureNative")
        } else {
            PathBuf::from("/Applications/FixtureGame.app")
        };
        let receipt = if id.ends_with("system") {
            "org.caskade.fixture.mac-native-pkg-system"
        } else {
            "org.caskade.fixture.mac-native-pkg"
        };
        assert!(
            matches!(std::fs::symlink_metadata(&directory),Err(e)if e.kind()==std::io::ErrorKind::NotFound),
            "refuse existing fixture path"
        );
        assert!(
            !Command::new("/usr/sbin/pkgutil")
                .args(["--pkg-info", receipt])
                .output()
                .unwrap()
                .status
                .success(),
            "refuse existing fixture receipt"
        );
        struct Cleanup {
            path: PathBuf,
            receipt: String,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = Command::new("sudo")
                    .args(["-n", "rm", "-rf", "--"])
                    .arg(&self.path)
                    .status();
                let _ = Command::new("sudo")
                    .args(["-n", "/usr/sbin/pkgutil", "--forget", &self.receipt])
                    .status();
            }
        }
        let _cleanup = Cleanup {
            path: directory.clone(),
            receipt: receipt.into(),
        };
        let mut host = host();
        host.packages.root = true;
        let node = ProbeEngine::with_builtin_formats(ProbeLimits::default())
            .probe_path(&source)
            .unwrap()
            .root;
        let mut planner = PlanEngine::new();
        planner.register(Box::new(PkgHandler::new())).unwrap();
        let plans = planner
            .plan(std::slice::from_ref(&node), &host, &Policy::default())
            .unwrap();
        assert_eq!(
            plans.candidates[plans.selected.unwrap() as usize].strategy,
            Strategy::Delegate
        );
        let runtime = Arc::new(RootRuntime {
            native: NativeRuntime::new(),
            commands: Mutex::default(),
        });
        let report = Executor::new(
            runtime.clone(),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .execute(
            ExecRequest {
                job_id: id.into(),
                root: root.path().into(),
                relative_dir: "Delegated Fixture".into(),
                plans,
                sources: BTreeMap::from([(node.id, source)]),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            runtime.commands.lock().unwrap()[0].program,
            Path::new("/usr/sbin/installer")
        );
        assert!(
            Command::new("/usr/sbin/pkgutil")
                .args(["--pkg-info", receipt])
                .output()
                .unwrap()
                .status
                .success()
        );
        let app = if id.ends_with("system") {
            directory.join("FixtureGame.app")
        } else {
            directory.clone()
        };
        let token = root.path().join("token");
        assert!(
            Command::new(app.join("Contents/MacOS/FixtureGame"))
                .args(["--token", token.to_str().unwrap()])
                .status()
                .unwrap()
                .success()
        );
        assert!(token.is_file());
        let Step::Delegate { uninstall, .. } = &report.candidate.steps[0] else {
            panic!()
        };
        assert_eq!(uninstall[0].args, ["--forget", receipt]);
        // Native pkgutil --forget removes receipts, not payload files. This
        // explicit fixture cleanup does not claim a production file uninstaller.
        assert!(
            Command::new("sudo")
                .args(["-n", "rm", "-rf", "--"])
                .arg(&directory)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("sudo")
                .args(["-n", "/usr/sbin/pkgutil", "--forget", receipt])
                .status()
                .unwrap()
                .success()
        );
        assert!(!directory.exists());
        assert!(
            !Command::new("/usr/sbin/pkgutil")
                .args(["--pkg-info", receipt])
                .output()
                .unwrap()
                .status
                .success()
        );
        root.assert_sentinel_untouched();
    }
}
