//! On, off, a crash and a host left running.

use super::*;
use std::os::unix::process::CommandExt;

#[tokio::test(flavor = "multi_thread")]
async fn hosting_on_installs_configures_asks_once_and_runs_until_quit() {
    let f = fixture(fast()).await;
    // Credentials are made during setup; the API answers them once known.
    let host = f.host.clone();
    let enabling = tokio::spawn(async move { host.enable().await });
    until("credentials made", || at_once(f.secrets.get()).is_some()).await;
    answer(&f).await;
    assert_eq!(enabling.await.unwrap().unwrap(), HostOutcome::On);
    assert_eq!(caskade_core::lock(&f.system.prompts).len(), 1, "one prompt");
    assert!(f.host.hosting().await);

    // Configuration written before the first start, Caskade's own.
    let files = HostFiles::linux(&f.staging.join("config"));
    let conf = std::fs::read_to_string(files.conf()).unwrap();
    assert!(conf.contains("sunshine_name = Livingroom"), "{conf}");
    for key in [
        "system_tray = disabled",
        "origin_web_ui_allowed = pc",
        "upnp = disabled",
        "notify_pre_releases = disabled",
    ] {
        assert!(conf.contains(key), "{key}");
    }
    assert_eq!(
        std::fs::read_to_string(files.apps()).unwrap(),
        config::EMPTY_APPS
    );
    let credentials = f.secrets.get().await.unwrap().unwrap();
    assert!(
        std::fs::read_to_string(files.state())
            .unwrap()
            .contains(&credentials.user)
    );
    // Never anywhere a backup or the library reaches.
    assert!(!conf.contains(&credentials.password));

    until("ready", || f.host.status().state == HostState::Ready).await;
    assert_eq!(f.host.status().version.as_deref(), Some("2026.914.233613"));
    let started = std::fs::read_to_string(f.staging.join("config/started")).unwrap();
    assert!(
        started.starts_with(&*f.staging.to_string_lossy()),
        "{started}"
    );
    assert!(f.temp.path().join("data/streaming/run.json").exists());

    // Quitting Caskade stops it.
    f.host.shutdown();
    assert!(!f.host.inner.runner.running());
    assert!(!f.temp.path().join("data/streaming/run.json").exists());
    // The next start runs it again, with no prompt.
    f.host.start().await;
    until("ready again", || f.host.status().state == HostState::Ready).await;
    assert_eq!(caskade_core::lock(&f.system.prompts).len(), 1);

    // Off: stopped, undone with one prompt, program gone; the
    // configuration and credentials stay for next time.
    assert_eq!(f.host.disable().await.unwrap(), HostOutcome::Off);
    assert!(!f.host.inner.runner.running());
    assert_eq!(caskade_core::lock(&f.system.prompts).len(), 2);
    assert!(caskade_core::lock(&f.system.present).is_empty());
    assert!(f.host.program(false).await.is_err());
    assert!(files.conf().exists() && files.state().exists());
    assert!(f.secrets.get().await.unwrap().is_some());
    assert!(!f.host.hosting().await);
    assert_eq!(f.host.status().state, HostState::Off);
    let _ = &f.settings;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_prompt_leaves_hosting_off_and_nothing_running() {
    let f = fixture(fast()).await;
    *caskade_core::lock(&f.system.answer) = Some(Prompt::Cancelled);
    assert_eq!(f.host.enable().await.unwrap(), HostOutcome::Cancelled);
    assert!(!f.host.hosting().await);
    assert!(!f.host.inner.runner.running());
    assert_eq!(f.host.status().state, HostState::Off);
    assert!(caskade_core::lock(&f.system.present).is_empty());
    // Refused, the same.
    *caskade_core::lock(&f.system.answer) = Some(Prompt::Refused);
    assert_eq!(f.host.enable().await.unwrap(), HostOutcome::Refused);
    assert!(!f.host.hosting().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crashed_host_restarts_and_a_refused_sign_in_needs_attention() {
    let f = fixture(fast()).await;
    let host = f.host.clone();
    let enabling = tokio::spawn(async move { host.enable().await });
    until("credentials made", || at_once(f.secrets.get()).is_some()).await;
    answer(&f).await;
    enabling.await.unwrap().unwrap();
    until("ready", || f.host.status().state == HostState::Ready).await;

    // The host dies: it is started again.
    let record: serde_json::Value = serde_json::from_slice(
        &std::fs::read(f.temp.path().join("data/streaming/run.json")).unwrap(),
    )
    .unwrap();
    let pid = record["pid"].as_u64().unwrap().to_string();
    std::process::Command::new("kill")
        .args(["-KILL", &pid])
        .status()
        .unwrap();
    until("restarted", || {
        std::fs::read(f.temp.path().join("data/streaming/run.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|record| record["pid"].as_u64().unwrap().to_string() != pid)
    })
    .await;
    until("ready after the crash", || {
        f.host.status().state == HostState::Ready
    })
    .await;
    f.host.shutdown();

    // The credentials no longer work (someone changed them).
    f.api.reset().await;
    Mock::given(method("GET"))
        .and(path("/api/config"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&f.api)
        .await;
    f.host.start().await;
    until("needs attention", || {
        f.host.status().state == HostState::NeedsAttention
    })
    .await;
    assert_eq!(
        f.host.status().problem.as_deref(),
        Some("Streaming isn't working on this device")
    );
    assert!(!f.host.inner.runner.running());
}

#[tokio::test]
async fn macos_does_not_host_in_this_milestone() {
    let f = fixture(fast()).await;
    let host = StreamingHost::with(
        f.host.inner.tools.clone(),
        f.host.inner.elevation.clone(),
        f.settings.clone(),
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Macos,
            runner: Arc::new(process::ChildRunner::new(f.temp.path().join("run.json"))),
            secrets: HostSecrets::memory(),
            timing: fast(),
            api_base: Url::parse("https://127.0.0.1:1/").unwrap(),
            http_port: 1,
            checks: Arc::new(HostChecks::fine),
        },
    );
    assert!(host.enable().await.is_err());
    assert_eq!(host.status().state, HostState::Unsupported);
    assert!(caskade_core::lock(&f.system.prompts).is_empty());
}

/// A host an earlier Caskade started and never stopped, and the file naming it.
fn left_behind(
    temp: &std::path::Path,
    record: impl FnOnce(ProcessIdentity) -> ProcessIdentity,
) -> (std::process::Child, process::ChildRunner) {
    let file = temp.join("run.json");
    let orphan = std::process::Command::new("sh")
        .args(["-c", "exec sleep 60"])
        .process_group(0)
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let identity = ProcessIdentity::lookup(orphan.id()).unwrap();
    std::fs::write(&file, serde_json::to_string(&record(identity)).unwrap()).unwrap();
    (orphan, process::ChildRunner::new(file))
}

fn next_host(temp: &std::path::Path) -> HostLaunch {
    HostLaunch {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "exec sleep 60".into()],
        env: Vec::new(),
        cwd: temp.into(),
    }
}

#[test]
fn a_host_left_running_by_a_crash_is_stopped_at_the_next_start() {
    let temp = tempfile::tempdir().unwrap();
    let (mut orphan, runner) = left_behind(temp.path(), |identity| identity);
    runner.start(&next_host(temp.path())).unwrap();
    assert!(orphan.wait().is_ok(), "the orphan was ended");
    assert!(runner.running());
    runner.stop().unwrap();
    assert!(!runner.running());
}

#[cfg(target_os = "linux")]
#[test]
fn a_host_left_running_is_still_known_after_the_clock_steps() {
    let temp = tempfile::tempdir().unwrap();
    let (mut orphan, runner) = left_behind(temp.path(), |identity| identity);
    let _stepped = caskade_core::tracking::ClockStep::by(7200);
    runner.start(&next_host(temp.path())).unwrap();
    assert!(orphan.wait().is_ok(), "the orphan was ended");
    runner.stop().unwrap();
}

#[test]
fn a_process_reusing_a_left_host_s_id_is_left_alone() {
    let temp = tempfile::tempdir().unwrap();
    let (mut bystander, runner) = left_behind(temp.path(), |identity| ProcessIdentity {
        started_at: identity.started_at + 1,
        ..identity
    });
    runner.start(&next_host(temp.path())).unwrap();
    runner.stop().unwrap();
    assert!(bystander.try_wait().unwrap().is_none());
    let _ = bystander.kill();
    let _ = bystander.wait();
}
