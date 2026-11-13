//! Fedora delegate proof: only docker exec runs dnf/rpm; host package managers
//! and pkexec are never invoked. The container is discarded on every exit.
#![cfg(all(target_os = "linux", feature = "linux-formats"))]
use caskade_installer::{Input, InstallPlan, PlanSet, RuntimeSpec, Step, Strategy};
use caskade_installer::{
    exec::{CancellationToken, ExecRequest, Executor},
    formats::linux::{
        deb::{PackageInfo, PackageProbe},
        rpm::RpmHandler,
    },
    host::HostCaps,
    manifest::{
        HandlerVersion, InstallManifest, ManifestSource, RelocationRecord, RelocationVerdict,
        UninstallRequest, Uninstaller,
    },
    plan::{PlanEngine, Policy},
    probe::ProbeEngine,
    runtime::{ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError},
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
const FEDORA: &str =
    "fedora:43@sha256:a651ddf48ea28a06ed4e1e6519f51c9f47e7a5a138722ade87369b8fbb7e5b42";
struct Container {
    id: String,
}
impl Container {
    fn new(fixtures: &Path) -> Self {
        let output = Command::new("docker")
            .args([
                "run",
                "--rm",
                "-d",
                "-v",
                &format!("{}:/fixtures:ro", fixtures.display()),
                FEDORA,
                "sleep",
                "600",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self {
            id: String::from_utf8(output.stdout).unwrap().trim().into(),
        }
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new("docker")
            .args(["exec", &self.id])
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.id])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
struct FedoraRuntime {
    container: String,
    fixtures: PathBuf,
    next: AtomicU64,
    children: Mutex<BTreeMap<ProcessId, Child>>,
    commands: Mutex<Vec<ProcessCommand>>,
}
fn failure(error: impl ToString) -> RuntimeError {
    RuntimeError::Io {
        message: error.to_string(),
    }
}
impl Runtime for FedoraRuntime {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        assert!(
            matches!(command.program.to_str(), Some("dnf" | "/usr/bin/dnf")),
            "host tool escaped test allowlist: {:?}",
            command.program
        );
        let args: Vec<_> = command
            .args
            .iter()
            .map(|arg| {
                Path::new(arg)
                    .strip_prefix(&self.fixtures)
                    .map(|relative| format!("/fixtures/{}", relative.display()))
                    .unwrap_or_else(|_| arg.clone())
            })
            .collect();
        let child = Command::new("docker")
            .args(["exec", &self.container, "dnf"])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(failure)?;
        let id = ProcessId(self.next.fetch_add(1, Ordering::Relaxed));
        self.children.lock().unwrap().insert(id, child);
        self.commands.lock().unwrap().push(command.clone());
        Ok(id)
    }
    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        let start = Instant::now();
        loop {
            let status = self
                .children
                .lock()
                .unwrap()
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownProcess { id })?
                .try_wait()
                .map_err(failure)?;
            if let Some(status) = status {
                return Ok(ProcessExit {
                    code: status.code().unwrap_or(-1),
                });
            }
            if start.elapsed() > timeout {
                self.kill(id)?;
                return Err(RuntimeError::TimedOut { id });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        if let Some(child) = self.children.lock().unwrap().get_mut(&id)
            && child.try_wait().map_err(failure)?.is_none()
        {
            let _ = Command::new("docker")
                .args(["kill", &self.container])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}
#[tokio::test]
#[ignore = "native Fedora dnf delegate: disposable Docker container and package repository network access"]
async fn rpm_indie_delegate_installs_launches_and_uninstalls_inside_disposable_fedora() {
    if std::env::var_os("CASKADE_TEST_NATIVE").is_none() {
        return;
    }
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .canonicalize()
        .unwrap();
    let package = fixtures.join("bin/packages/rpm-self-contained.rpm");
    let container = Container::new(&fixtures);
    let runtime = Arc::new(FedoraRuntime {
        container: container.id.clone(),
        fixtures: fixtures.clone(),
        next: AtomicU64::new(1),
        children: Mutex::default(),
        commands: Mutex::default(),
    });
    let mut host: HostCaps = caskade_installer::pipeline::input::host(Some("linux")).unwrap();
    host.packages.root = true;
    host.packages.managers = BTreeMap::from([("dnf".into(), "/usr/bin/dnf".into())]);
    let mut probe = ProbeEngine::default();
    probe.register(Box::new(PackageProbe));
    let node = probe.probe_path(&package).unwrap().root;
    let info: PackageInfo = serde_json::from_value(node.facts["linux.package"].clone()).unwrap();
    let mut planner = PlanEngine::new();
    planner.register(Box::new(RpmHandler)).unwrap();
    let mut plans = planner
        .plan(std::slice::from_ref(&node), &host, &Policy::default())
        .unwrap();
    assert!(plans.question.is_none());
    let mut candidate = plans.candidates.remove(
        plans
            .candidates
            .iter()
            .position(|candidate| candidate.strategy == Strategy::Delegate)
            .expect("Fedora native delegate alternative"),
    );
    // Persist a logical host-tool ID; the injected runtime resolves it strictly
    // inside the container. No absolute host package-manager capability leaks.
    for step in &mut candidate.steps {
        if let Step::Delegate {
            tool, uninstall, ..
        } = step
        {
            *tool = "dnf".into();
            for undo in uninstall {
                undo.tool = "dnf".into();
            }
        }
    }
    let root = TestRoot::new().unwrap();
    let worker = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    );
    let report = worker
        .execute(
            ExecRequest {
                job_id: "fedora-rpm".into(),
                root: root.path().into(),
                relative_dir: "RPM Delegate".into(),
                plans: PlanSet {
                    candidates: vec![candidate],
                    selected: Some(0),
                    question: None,
                },
                sources: BTreeMap::from([(node.id.clone(), package.clone())]),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let query = container.run(&["rpm", "-q", &info.name]);
    assert!(
        query.status.success(),
        "{}",
        String::from_utf8_lossy(&query.stderr)
    );
    let launch = container.run(&[
        "/opt/caskade-fixture/start.sh",
        "--token",
        "/tmp/caskade-rpm-token",
    ]);
    assert!(
        launch.status.success(),
        "{}",
        String::from_utf8_lossy(&launch.stderr)
    );
    let token = container.run(&["cat", "/tmp/caskade-rpm-token"]);
    assert!(
        String::from_utf8_lossy(&token.stdout)
            .trim()
            .ends_with("/FixtureGame.x86_64")
    );
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    let source = ManifestSource::from_input(&Input::Path(package), None, None).unwrap();
    record.title = info.name.clone();
    record.source = source.clone();
    record.plan = InstallPlan {
        schema_version: 1,
        input: source.input,
        artifacts: vec![node],
        root_id: "fedora-test".into(),
        relative_dir: report.relative_dir.clone(),
        candidate: report.candidate.clone(),
    };
    record.files.clear();
    record.launch_targets.clear();
    record.runtime = RuntimeSpec::Native;
    record.external_effects.clear();
    record.handlers = vec![HandlerVersion {
        id: report.candidate.handler.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
    }];
    record.relocation = RelocationRecord {
        verdict: RelocationVerdict::Pinned,
        ..Default::default()
    };
    record.uninstall_steps = report
        .candidate
        .steps
        .iter()
        .flat_map(|step| match step {
            Step::Delegate { uninstall, .. } => uninstall
                .iter()
                .map(|undo| Step::Delegate {
                    tool: undo.tool.clone(),
                    args: undo.args.clone(),
                    uninstall: vec![],
                    only_if: undo.only_if.clone(),
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect();
    let manifest = InstallManifest::new(record).unwrap();
    manifest.write(&report.dir).unwrap();
    Uninstaller::new(worker)
        .uninstall(
            UninstallRequest {
                job_id: "fedora-rpm-uninstall".into(),
                root: root.path().into(),
                relative_dir: report.relative_dir,
                manifest,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    root.assert_clean();
    assert!(!container.run(&["rpm", "-q", &info.name]).status.success());
    assert!(
        !container
            .run(&["test", "-e", "/opt/caskade-fixture/start.sh"])
            .status
            .success()
    );
    assert!(
        runtime
            .commands
            .lock()
            .unwrap()
            .iter()
            .all(|command| command.program == Path::new("dnf"))
    );
}
