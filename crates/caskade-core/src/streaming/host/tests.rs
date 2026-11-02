//! Hosting on and off against a fake release (a shell script standing in
//! for the host), the administrator step injected, the real child process
//! runner, and the API behind a TLS front.

use super::*;
use crate::{
    AppPaths,
    bus::{Bus, BusOptions},
    db::LibraryStore,
    elevation::{Change, Prompt, Request, Runner, Step},
    managed::{Hosts, ToolCatalog, ToolDefinition},
    streaming::testing::{cert, tls_front},
    updates::GitHub,
};
use std::{collections::BTreeSet, io::Write};
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{basic_auth, method, path},
};

/// The host stand-in: `--creds` writes the state file where Sunshine
/// does; otherwise it runs until stopped.
const HOST: &str = r#"#!/bin/sh
if [ "$1" = "--creds" ]; then
  mkdir -p "$CONFIGURATION_DIRECTORY/sunshine"
  printf '{"username":"%s"}' "$2" > "$CONFIGURATION_DIRECTORY/sunshine/sunshine_state.json"
  exit 0
fi
echo "$PWD $CONFIGURATION_DIRECTORY" > "$CONFIGURATION_DIRECTORY/started"
exec sleep 60
"#;

const TOOL_TOML: &str = r#"
id = "sunshine"
name = "Sunshine"
dir = "streaming/host"

[detect.linux]
executable = "^sunshine$"

[[install.linux]]
source = "github"
repo = "example/host"
asset = '^host-[\d.]+\.zip$'
"#;

#[derive(Default)]
struct System {
    present: Mutex<BTreeSet<Change>>,
    prompts: Mutex<Vec<Request>>,
    answer: Mutex<Option<Prompt>>,
}

impl Runner for System {
    fn present(&self, change: Change) -> bool {
        crate::lock(&self.present).contains(&change)
    }

    fn run(&self, request: &Request) -> Result<Prompt> {
        crate::lock(&self.prompts).push(request.clone());
        let answer = crate::lock(&self.answer).unwrap_or(Prompt::Done);
        if answer == Prompt::Done {
            let mut present = crate::lock(&self.present);
            for step in &request.steps {
                match step {
                    Step::Add(change) => present.insert(*change),
                    Step::Remove(change) => present.remove(change),
                };
            }
        }
        Ok(answer)
    }
}

struct Fixture {
    temp: tempfile::TempDir,
    host: StreamingHost,
    system: Arc<System>,
    settings: SettingsService,
    secrets: HostSecrets,
    _github: MockServer,
    api: MockServer,
    staging: PathBuf,
}

fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default().unix_permissions(0o755),
            )
            .unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

async fn fixture(timing: Timing) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();

    // The release, as GitHub serves it.
    let github = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/example/host/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "/example/host/releases/tag/v1.0"),
        )
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/example/host/releases/expanded_assets/v1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<a href="/example/host/releases/download/v1.0/host-1.0.zip">host</a>"#,
        ))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/example/host/releases/download/v1.0/host-1.0.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip(&[
            ("host-1.0/usr/bin/sunshine", HOST),
            ("host-1.0/usr/share/sunshine/apps.json", "{}"),
        ])))
        .mount(&github)
        .await;
    let hosts = Hosts {
        github: GitHub::new(Url::parse(&format!("{}/", github.uri())).unwrap()),
        dolphin: Url::parse(&format!("{}/dolphin/", github.uri())).unwrap(),
    };
    let catalog = ToolCatalog::new(vec![ToolDefinition::parse(TOOL_TOML).unwrap()]).unwrap();
    let machine = crate::managed::Host {
        os: OperatingSystem::Linux,
        arch: crate::managed::Arch::X86_64,
        wayland: false,
        gamescope: false,
    };
    let tools = ManagedTools::new(store, Bus::new(BusOptions::default()), catalog, &paths)
        .with_host(machine, hosts, temp.path().join("no-flatpak"));

    // The host's API, answering only Caskade's credentials, behind the
    // certificate the host makes at its first start.
    let api = MockServer::start().await;
    let served = cert();
    let front = tls_front(*api.address(), &served).await;
    let staging = paths.data.join("streaming").join("host");
    let files = HostFiles::linux(&staging.join("config"));
    std::fs::create_dir_all(files.cert().parent().unwrap()).unwrap();
    std::fs::write(files.cert(), &served.pem).unwrap();

    let system = Arc::new(System::default());
    let elevation = Elevation::with_runner(system.clone(), OperatingSystem::Linux, &paths);
    let secrets = HostSecrets::memory();
    let host = StreamingHost::with(
        tools,
        elevation,
        settings.clone(),
        Parts {
            os: OperatingSystem::Linux,
            runner: Arc::new(process::ChildRunner::new(
                paths.data.join("streaming/run.json"),
            )),
            secrets: secrets.clone(),
            timing,
            api_base: Url::parse(&format!("https://{front}/")).unwrap(),
        },
    );
    host.set_device_name("Living\nroom");
    Fixture {
        temp,
        host,
        system,
        settings,
        secrets,
        _github: github,
        api,
        staging,
    }
}

fn fast() -> Timing {
    Timing {
        ready_wait: Duration::from_secs(10),
        watch_every: Duration::from_millis(50),
        restart_after: Duration::from_millis(50),
    }
}

/// The API answers the credentials Caskade made.
async fn answer(f: &Fixture) {
    let credentials = f.secrets.get().await.unwrap().unwrap();
    Mock::given(method("GET"))
        .and(path("/api/config"))
        .and(basic_auth(credentials.user, credentials.password))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": true, "version": "2026.914.233613"
        })))
        .mount(&f.api)
        .await;
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hosting_on_installs_configures_asks_once_and_runs_until_quit() {
    let f = fixture(fast()).await;
    // Credentials are made during setup; the API answers them once known.
    let host = f.host.clone();
    let enabling = tokio::spawn(async move { host.enable().await });
    until("credentials made", || at_once(f.secrets.get()).is_some()).await;
    answer(&f).await;
    assert_eq!(enabling.await.unwrap().unwrap(), HostOutcome::On);
    assert_eq!(crate::lock(&f.system.prompts).len(), 1, "one prompt");
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
    assert_eq!(crate::lock(&f.system.prompts).len(), 1);

    // Off: stopped, undone with one prompt, program gone; the
    // configuration and credentials stay for next time.
    assert_eq!(f.host.disable().await.unwrap(), HostOutcome::Off);
    assert!(!f.host.inner.runner.running());
    assert_eq!(crate::lock(&f.system.prompts).len(), 2);
    assert!(crate::lock(&f.system.present).is_empty());
    assert!(!f.staging.join("usr/bin/sunshine").exists());
    assert!(files.conf().exists() && files.state().exists());
    assert!(f.secrets.get().await.unwrap().is_some());
    assert!(!f.host.hosting().await);
    assert_eq!(f.host.status().state, HostState::Off);
    let _ = &f.settings;
}

/// The answer of a future that is ready at once (MemorySecrets), from a
/// synchronous check.
fn at_once<T>(future: impl std::future::Future<Output = Result<T>>) -> T {
    poll_ready(future).unwrap()
}

fn poll_ready<F: std::future::Future>(future: F) -> F::Output {
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_prompt_leaves_hosting_off_and_nothing_running() {
    let f = fixture(fast()).await;
    *crate::lock(&f.system.answer) = Some(Prompt::Cancelled);
    assert_eq!(f.host.enable().await.unwrap(), HostOutcome::Cancelled);
    assert!(!f.host.hosting().await);
    assert!(!f.host.inner.runner.running());
    assert_eq!(f.host.status().state, HostState::Off);
    assert!(crate::lock(&f.system.present).is_empty());
    // Refused, the same.
    *crate::lock(&f.system.answer) = Some(Prompt::Refused);
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
            os: OperatingSystem::Macos,
            runner: Arc::new(process::ChildRunner::new(f.temp.path().join("run.json"))),
            secrets: HostSecrets::memory(),
            timing: fast(),
            api_base: Url::parse("https://127.0.0.1:1/").unwrap(),
        },
    );
    assert!(host.enable().await.is_err());
    assert_eq!(host.status().state, HostState::Unsupported);
    assert!(crate::lock(&f.system.prompts).is_empty());
}

#[test]
fn a_host_left_running_by_a_crash_is_stopped_at_the_next_start() {
    let temp = tempfile::tempdir().unwrap();
    let record = temp.path().join("run.json");
    // A host an earlier Caskade started and never stopped.
    let mut orphan = std::process::Command::new("sh")
        .args(["-c", "exec sleep 60"])
        .process_group(0)
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let started_at = process::start_time(orphan.id()).unwrap();
    std::fs::write(
        &record,
        serde_json::json!({ "pid": orphan.id(), "started_at": started_at }).to_string(),
    )
    .unwrap();
    let runner = process::ChildRunner::new(record.clone());
    runner
        .start(&HostLaunch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "exec sleep 60".into()],
            env: Vec::new(),
            cwd: temp.path().into(),
        })
        .unwrap();
    assert!(orphan.wait().is_ok(), "the orphan was ended");
    assert!(runner.running());
    runner.stop().unwrap();
    assert!(!runner.running());
}

use std::os::unix::process::CommandExt;
