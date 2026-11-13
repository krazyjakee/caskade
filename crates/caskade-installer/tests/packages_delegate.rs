//! Native profile only: apt and its dependency solver run as root exclusively
//! inside a disposable Debian container. No host pkexec/package manager runs.
#![cfg(all(target_os = "linux", feature = "linux-formats"))]
use caskade_installer::{
    Strategy,
    exec::{CancellationToken, ExecRequest, Executor},
    formats::linux::deb::{self, DebHandler, PackageInfo, PackageProbe},
    host::HostCaps,
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

struct Debian {
    id: String,
}
impl Debian {
    fn start(fixtures: &Path) -> Self {
        // Digest observed from the genuine Debian trixie-slim image. The
        // explicit ignored profile may download this image and apt indexes.
        let image = "debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad";
        let volume = format!("{}:/fixtures:ro", fixtures.display());
        let output = Command::new("docker")
            .args(["run", "--rm", "-d", "-v", &volume, image, "sleep", "600"])
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
    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new("docker")
            .args(["exec", "-e", "DEBIAN_FRONTEND=noninteractive", &self.id])
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Debian {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.id])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
struct DockerRuntime {
    container: String,
    fixtures: PathBuf,
    next: AtomicU64,
    children: Mutex<BTreeMap<ProcessId, Child>>,
    commands: Mutex<Vec<ProcessCommand>>,
}
impl DockerRuntime {
    fn new(container: &Debian, fixtures: PathBuf) -> Self {
        Self {
            container: container.id.clone(),
            fixtures,
            next: AtomicU64::new(1),
            children: Mutex::default(),
            commands: Mutex::default(),
        }
    }
}
fn runtime_error(error: impl ToString) -> RuntimeError {
    RuntimeError::Io {
        message: error.to_string(),
    }
}
impl Runtime for DockerRuntime {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        assert!(
            matches!(
                command.program.to_str(),
                Some("apt" | "dpkg-query" | "/usr/bin/apt" | "/usr/bin/dpkg-query")
            ),
            "unexpected process {:?}",
            command.program
        );
        let args: Vec<_> = command
            .args
            .iter()
            .map(|arg| {
                Path::new(arg)
                    .strip_prefix(&self.fixtures)
                    .map(|p| format!("/fixtures/{}", p.display()))
                    .unwrap_or_else(|_| arg.clone())
            })
            .collect();
        let child = Command::new("docker")
            .args([
                "exec",
                "-e",
                "DEBIAN_FRONTEND=noninteractive",
                &self.container,
            ])
            .arg(&command.program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(runtime_error)?;
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
                .map_err(runtime_error)?;
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
        // Executor requests process-tree cleanup even after a successful wait.
        // An exited docker exec has no live apt children; the disposable OS
        // must remain alive so installed files and uninstall can be verified.
        if let Some(child) = self.children.lock().unwrap().get_mut(&id)
            && child.try_wait().map_err(runtime_error)?.is_none()
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
#[ignore = "native package delegate profile: Docker + Debian apt network access; never invokes host pkexec"]
async fn deb_missing_dep_falls_back_to_apt_inside_disposable_debian() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .canonicalize()
        .unwrap();
    let package = fixtures.join("bin/packages/deb-missing-dep.deb");
    let container = Debian::start(&fixtures);
    let update = container.command(&[
        "apt",
        "-o",
        "Acquire::Retries=0",
        "-o",
        "Acquire::http::Timeout=20",
        "update",
    ]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    let runtime = Arc::new(DockerRuntime::new(&container, fixtures));
    let mut host: HostCaps = caskade_installer::pipeline::input::host(Some("linux")).unwrap();
    host.packages.root = true;
    host.packages.managers = BTreeMap::from([
        ("apt".into(), "/usr/bin/apt".into()),
        ("dpkg-query".into(), "/usr/bin/dpkg-query".into()),
    ]);
    let mut probe = ProbeEngine::default();
    probe.register(Box::new(PackageProbe));
    let node = probe.probe_path(&package).unwrap().root;
    let info: PackageInfo = serde_json::from_value(node.facts["linux.package"].clone()).unwrap();
    let root = TestRoot::new().unwrap();
    let dependencies = deb::check_dependencies(&*runtime, &host, &info, root.path()).unwrap();
    assert_eq!(dependencies.missing, ["hello"]);
    let mut planner = PlanEngine::new();
    planner.register(Box::new(DebHandler)).unwrap();
    let plans = planner
        .plan(std::slice::from_ref(&node), &host, &Policy::default())
        .unwrap();
    assert!(plans.question.is_none());
    assert_eq!(
        plans.candidates[plans.selected.unwrap() as usize].strategy,
        Strategy::Delegate
    );
    let report = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .execute(
        ExecRequest {
            job_id: "deb-container".into(),
            root: root.path().into(),
            relative_dir: "Delegated Game".into(),
            plans,
            sources: BTreeMap::from([(node.id, package)]),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    root.assert_sentinel_untouched();
    let installed =
        container.command(&["dpkg-query", "--status", "hello", "caskade-deb-missing-dep"]);
    assert!(
        installed.status.success(),
        "status={} stdout={} stderr={} commands={:?}",
        installed.status,
        String::from_utf8_lossy(&installed.stdout),
        String::from_utf8_lossy(&installed.stderr),
        runtime.commands.lock().unwrap()
    );
    assert!(
        container
            .command(&["test", "-f", "/opt/caskade-fixture/maintainer-ran"])
            .status
            .success()
    );
    let launch = container.command(&[
        "/opt/caskade-fixture/start.sh",
        "--token",
        "/tmp/caskade-package-token",
    ]);
    assert!(
        launch.status.success(),
        "{}",
        String::from_utf8_lossy(&launch.stderr)
    );
    let token = container.command(&["cat", "/tmp/caskade-package-token"]);
    assert!(
        String::from_utf8_lossy(&token.stdout)
            .trim()
            .ends_with("/FixtureGame.x86_64")
    );
    let caskade_installer::Step::Delegate { uninstall, .. } = &report.candidate.steps[0] else {
        panic!("expected delegate")
    };
    let undo = &uninstall[0];
    let id = runtime
        .spawn(&ProcessCommand {
            program: undo.tool.clone().into(),
            args: undo.args.clone(),
            env: BTreeMap::new(),
            cwd: root.path().into(),
        })
        .unwrap();
    assert_eq!(runtime.wait(id, Duration::from_secs(120)).unwrap().code, 0);
    assert!(
        !container
            .command(&["test", "-e", "/opt/caskade-fixture/start.sh"])
            .status
            .success()
    );
    assert!(
        runtime
            .commands
            .lock()
            .unwrap()
            .iter()
            .all(|c| c.program.file_name().unwrap() != "pkexec")
    );
    root.assert_sentinel_untouched();
}
