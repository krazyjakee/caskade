//! Explicit native profile. Only the dedicated fixture's directory and receipt
//! are touched, on the disposable macOS runner; no host command runs on Linux.
#![cfg(all(target_os = "macos", feature = "macos-formats"))]
use caskade_installer::{
    Step, Strategy,
    exec::{CancellationToken, ExecRequest, Executor},
    formats::macos::pkg::{PkgHandler, PkgProbe},
    host::HostCaps,
    plan::{PlanEngine, Policy},
    probe::ProbeEngine,
    runtime::{NativeRuntime, ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError},
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};
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
        let mut root = command.clone();
        root.program = "/usr/bin/sudo".into();
        root.args
            .insert(0, command.program.to_string_lossy().into_owned());
        root.args.insert(0, "-n".into());
        self.native.spawn(&root)
    }
    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        self.native.wait(id, timeout)
    }
    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        self.native.kill(id)
    }
}
struct Cleanup {
    dir: PathBuf,
    id: String,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = Command::new("sudo")
            .args(["-n", "rm", "-rf", "--"])
            .arg(&self.dir)
            .status();
        let _ = Command::new("sudo")
            .args(["-n", "/usr/sbin/pkgutil", "--forget", &self.id])
            .status();
    }
}
#[tokio::test]
#[ignore = "native macOS runner (#22): sudo -n installer on disposable runner, verifies real system payload and receipt"]
async fn system_targeting_pkg_uses_installer_delegate_on_native_macos() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/macos-pkg/mac-pkg-system.pkg");
    let directory = PathBuf::from("/Library/CaskadePkgFixture");
    let identifier = "org.caskade.fixture.mac-pkg-system";
    assert!(
        matches!(std::fs::symlink_metadata(&directory), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
        "refuse to replace existing path"
    );
    assert!(
        !Command::new("/usr/sbin/pkgutil")
            .args(["--pkg-info", identifier])
            .output()
            .unwrap()
            .status
            .success(),
        "refuse existing receipt"
    );
    assert!(
        Command::new("sudo")
            .args(["-n", "true"])
            .status()
            .unwrap()
            .success()
    );
    let _cleanup = Cleanup {
        dir: directory.clone(),
        id: identifier.into(),
    };
    let mut host = HostCaps::detect();
    host.packages.root = true;
    let mut probe = ProbeEngine::default();
    probe.register(Box::new(PkgProbe));
    let node = probe.probe_path(&package).unwrap().root;
    let mut plan = PlanEngine::new();
    plan.register(Box::new(PkgHandler)).unwrap();
    let plans = plan
        .plan(std::slice::from_ref(&node), &host, &Policy::default())
        .unwrap();
    assert!(plans.question.is_none());
    assert_eq!(
        plans.candidates[plans.selected.unwrap() as usize].strategy,
        Strategy::Delegate
    );
    // Compile the real administrator script without invoking authentication,
    // then exercise its exact argv quoting with a harmless printf target.
    let mut unprivileged = host.clone();
    unprivileged.packages.root = false;
    let admin = plan
        .plan(
            std::slice::from_ref(&node),
            &unprivileged,
            &Policy::default(),
        )
        .unwrap();
    let Step::Delegate { args, .. } = &admin.candidates[0].steps[0] else {
        panic!()
    };
    let root = TestRoot::new().unwrap();
    let compiled = root.path().join("admin.scpt");
    let compile = Command::new("/usr/bin/osacompile")
        .arg("-o")
        .arg(&compiled)
        .args(["-e", &args[1]])
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let dry = args[1].replace(
        "do shell script commandText with administrator privileges",
        "return commandText",
    );
    let value = "a 'quoted' filename; $(touch sentinel)\nnext line.pkg";
    let quoted = Command::new("/usr/bin/osascript")
        .args(["-e", &dry, "--", "/usr/bin/printf", "%s", value])
        .output()
        .unwrap();
    assert!(
        quoted.status.success(),
        "{}",
        String::from_utf8_lossy(&quoted.stderr)
    );
    let command = String::from_utf8(quoted.stdout).unwrap();
    let echoed = Command::new("/bin/sh")
        .args(["-c", command.trim_end()])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(echoed.status.success());
    assert_eq!(echoed.stdout, value.as_bytes());
    root.assert_sentinel_untouched();

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
            job_id: "native-mac-pkg".into(),
            root: root.path().into(),
            relative_dir: "System Package".into(),
            plans,
            sources: BTreeMap::from([(node.id, package)]),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let calls = runtime.commands.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args[0], "-pkg");
    assert_eq!(&calls[0].args[2..], ["-target", "/"]);
    assert!(
        Command::new("/usr/sbin/pkgutil")
            .args(["--pkg-info", identifier])
            .output()
            .unwrap()
            .status
            .success()
    );
    let app = directory.join("FixtureGame.app");
    let token = root.path().join("system-token");
    let output = Command::new(app.join("Contents/MacOS/FixtureGame"))
        .args(["--token", token.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(token.is_file());
    assert!(
        std::fs::symlink_metadata(app.join("Contents/Frameworks/X.framework/Versions/Current"))
            .unwrap()
            .is_symlink()
    );
    let Step::Delegate { uninstall, .. } = &report.candidate.steps[0] else {
        panic!()
    };
    assert_eq!(uninstall[0].args, ["--forget", identifier]);
    root.assert_sentinel_untouched();
}
