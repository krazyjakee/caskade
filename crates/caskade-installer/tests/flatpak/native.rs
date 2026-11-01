use super::*;
use caskade_installer::runtime::{
    NativeRuntime, ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError,
};
use std::{process::Command, time::Duration};
/// Every delegated process receives a private installation and XDG roots.
/// No process-global environment variables are mutated by this test.
struct Isolated {
    native: NativeRuntime,
    environment: BTreeMap<String, String>,
}
impl Runtime for Isolated {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        let mut command = command.clone();
        command.env.extend(self.environment.clone());
        self.native.spawn(&command)
    }
    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        self.native.wait(id, timeout)
    }
    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        self.native.kill(id)
    }
}
fn flatpak(env: &BTreeMap<String, String>, args: &[&str]) -> std::process::Output {
    let output = Command::new("flatpak")
        .envs(env)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "flatpak {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
async fn cycle(launch: bool) {
    if std::env::var_os("CASKADE_TEST_NATIVE").is_none() {
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path();
    let environment: BTreeMap<_, _> = [
        ("FLATPAK_USER_DIR", path.join("installation")),
        ("XDG_DATA_HOME", path.join("data")),
        ("XDG_CONFIG_HOME", path.join("config")),
        ("XDG_CACHE_HOME", path.join("cache")),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_string_lossy().into_owned()))
    .collect();
    assert!(!path.join("installation").exists());
    let initial = flatpak(&environment, &["list", "--user", "--columns=ref"]);
    assert!(initial.stdout.is_empty());
    let runtime = path.join("runtime.flatpak");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/flatpak-fixtures.py");
    let built = Command::new("python3")
        .arg(script)
        .args(["--runtime-output", runtime.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    flatpak(
        &environment,
        &[
            "install",
            "--user",
            "--noninteractive",
            "--no-deps",
            runtime.to_str().unwrap(),
        ],
    );
    let root = TestRoot::new().unwrap();
    let worker = executor(Arc::new(Isolated {
        native: NativeRuntime::new(),
        environment: environment.clone(),
    }));
    let node = probe();
    let installed = FlatpakInstaller::new(worker.clone())
        .install(
            request(&root, &node),
            node,
            ManifestSource::from_input(&Input::Path(source()), None, None).unwrap(),
            "fixture-root".into(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(matches!(
        installed.outcome,
        InstallOutcome::HandOff(HandOff::Delegated { .. })
    ));
    assert_eq!(
        installed.manifest.record().relocation.verdict,
        RelocationVerdict::Pinned
    );
    flatpak(&environment, &["info", "--user", "io.caskade.FixtureGame"]);
    let location = flatpak(
        &environment,
        &[
            "info",
            "--user",
            "--show-location",
            "io.caskade.FixtureGame",
        ],
    );
    let deployed = PathBuf::from(String::from_utf8(location.stdout).unwrap().trim());
    let payload = deployed.join("files/bin/fixture-game");
    assert_eq!(
        fs::read(&payload).unwrap(),
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/bin/fixture-game-linux-x86_64")
        )
        .unwrap()
    );
    if launch {
        let token = path.join("launch-token");
        let access = format!("--filesystem={}", path.display());
        flatpak(
            &environment,
            &[
                "run",
                "--user",
                "--branch=1",
                &access,
                "io.caskade.FixtureGame",
                "--token",
                token.to_str().unwrap(),
            ],
        );
        assert!(
            fs::read_to_string(&token)
                .unwrap()
                .trim()
                .ends_with("/bin/fixture-game")
        );
    }
    Uninstaller::new(worker)
        .uninstall(
            UninstallRequest {
                job_id: "native-flatpak-uninstall".into(),
                root: root.path().into(),
                relative_dir: installed.execution.relative_dir,
                manifest: installed.manifest,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    root.assert_clean();
    let absent = Command::new("flatpak")
        .envs(&environment)
        .args(["info", "--user", "io.caskade.FixtureGame"])
        .output()
        .unwrap();
    assert!(!absent.status.success());
    flatpak(
        &environment,
        &[
            "uninstall",
            "--user",
            "--noninteractive",
            "runtime/io.caskade.FixtureRuntime/x86_64/1",
        ],
    );
    let final_state = flatpak(&environment, &["list", "--user", "--columns=ref"]);
    assert_eq!(final_state.stdout, initial.stdout);
}

#[tokio::test]
async fn genuine_flatpak_bundle_installs_and_uninstalls_in_a_private_user_installation() {
    cycle(false).await;
}
#[tokio::test]
#[ignore = "native Flatpak sandbox launch: needs bubblewrap/user namespaces on the runner"]
async fn flatpak_bundle_launches_inside_its_sandbox_and_uninstalls_without_user_state() {
    cycle(true).await;
}
