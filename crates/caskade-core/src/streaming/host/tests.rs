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
    streaming::testing::{Apps, cert, tls_front},
    tracking::ProcessIdentity,
    updates::GitHub,
};
use std::{collections::BTreeSet, io::Write};
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{basic_auth, method, path},
};

/// The host stand-in: `--creds` writes the state file where Sunshine
/// does, keeping its paired devices (`root`) as Sunshine does; otherwise it
/// runs until stopped.
const HOST: &str = r#"#!/bin/sh
if [ "$1" = "--creds" ]; then
  mkdir -p "$CONFIGURATION_DIRECTORY/sunshine"
  state="$CONFIGURATION_DIRECTORY/sunshine/sunshine_state.json"
  if [ -f "$state" ] && grep -q '"root"' "$state"; then
    sed -i "s/\"username\":\"[^\"]*\"/\"username\":\"$2\"/" "$state"
  else
    printf '{"username":"%s"}' "$2" > "$state"
  fi
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
    github: MockServer,
    api: MockServer,
    serverinfo: MockServer,
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
    // Its /serverinfo, which says whether a device streams from it.
    let serverinfo = MockServer::start().await;
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
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner: Arc::new(process::ChildRunner::new(
                paths.data.join("streaming/run.json"),
            )),
            secrets: secrets.clone(),
            timing,
            api_base: Url::parse(&format!("https://{front}/")).unwrap(),
            http_port: serverinfo.address().port(),
            checks: Arc::new(HostChecks::fine),
        },
    );
    host.set_device_name("Living\nroom");
    Fixture {
        temp,
        host,
        system,
        settings,
        secrets,
        github,
        api,
        serverinfo,
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
    assert!(f.host.program(false).await.is_err());
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
    assert!(crate::lock(&f.system.prompts).is_empty());
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
    let _stepped = crate::tracking::ClockStep::by(7200);
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

use std::os::unix::process::CommandExt;

#[tokio::test(flavor = "multi_thread")]
async fn a_users_own_host_is_used_with_consent_and_only_caskades_apps_change() {
    use crate::streaming::{
        existing::{Existing, Found},
        publish::{self, Offer},
    };
    let f = fixture(fast()).await;
    // Their Sunshine, its certificate where its install keeps it, and its
    // own app.
    let theirs = f
        .temp
        .path()
        .join("home/.config/sunshine/credentials/cacert.pem");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::copy(HostFiles::linux(&f.staging.join("config")).cert(), &theirs).unwrap();
    let apps = Apps::default();
    crate::lock(&apps.0)
        .push(serde_json::json!({ "name": "Desktop", "image-path": "desktop.png" }));
    let user = Credentials {
        user: "sunshine".into(),
        password: "theirs".into(),
    };
    Mock::given(method("GET"))
        .and(path("/api/config"))
        .and(basic_auth("sunshine", "theirs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": true, "version": "2025.628.4510"
        })))
        .mount(&f.api)
        .await;
    Mock::given(path("/api/config"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&f.api)
        .await;
    Mock::given(path("/api/apps"))
        .and(basic_auth("sunshine", "theirs"))
        .respond_with(apps.clone())
        .mount(&f.api)
        .await;
    Mock::given(wiremock::matchers::path_regex("^/api/apps/[0-9]+$"))
        .and(basic_auth("sunshine", "theirs"))
        .respond_with(apps.clone())
        .mount(&f.api)
        .await;
    let found = Existing {
        found: vec![Found::Package],
        cert: Some(theirs.clone()),
    };
    let host = StreamingHost::with(
        f.host.inner.tools.clone(),
        f.host.inner.elevation.clone(),
        f.settings.clone(),
        Parts {
            existing: Arc::new(move || Some(found.clone())),
            os: OperatingSystem::Linux,
            runner: Arc::new(process::ChildRunner::new(f.temp.path().join("run.json"))),
            secrets: f.secrets.clone(),
            timing: fast(),
            api_base: f.host.inner.api_base.clone(),
            http_port: f.host.inner.http_port,
            checks: Arc::new(HostChecks::fine),
        },
    );

    // Switching on offers theirs: nothing installed, no prompt.
    assert_eq!(host.enable().await.unwrap(), HostOutcome::ExistingFound);
    assert!(f.host.inner.tools.managed(TOOL).await.unwrap().is_none());
    assert!(crate::lock(&f.system.prompts).is_empty());
    assert!(!host.hosting().await);

    // A wrong password changes nothing; theirs works.
    let wrong = Credentials {
        password: "guess".into(),
        ..user.clone()
    };
    assert!(matches!(
        host.use_existing(wrong).await,
        Err(Error::Forbidden(_))
    ));
    assert!(!host.hosting().await);
    assert_eq!(
        host.use_existing(user.clone()).await.unwrap(),
        HostOutcome::On
    );
    assert!(host.hosting().await);
    assert_eq!(host.status().version.as_deref(), Some("2025.628.4510"));
    assert_eq!(f.secrets.existing().await.unwrap(), Some(user));

    // Caskade's games go in beside theirs.
    let caskade = publish::Caskade {
        program: std::env::current_exe().unwrap(),
        flatpak_host: false,
    };
    let api = host.api().await.unwrap();
    let offers = [Offer::Game {
        title: "Hades".into(),
        game_id: "g1".into(),
        image: None,
    }];
    let published = publish::sync(&api, &caskade, &offers).await.unwrap();
    assert_eq!(
        published,
        publish::Published {
            added: 1,
            removed: 0
        }
    );
    // Again: nothing to do.
    assert_eq!(
        publish::sync(&api, &caskade, &offers).await.unwrap(),
        Default::default()
    );
    let names: Vec<String> = crate::lock(&apps.0)
        .iter()
        .map(|app| app["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Desktop", "Hades"]);
    assert_eq!(crate::lock(&apps.0)[1]["cmd"], caskade.launch_command("g1"));

    // Hosting off: only Caskade's app goes; theirs, and their host, stay.
    assert_eq!(host.disable().await.unwrap(), HostOutcome::Off);
    let left = crate::lock(&apps.0).clone();
    assert_eq!(
        left,
        [serde_json::json!({ "name": "Desktop", "image-path": "desktop.png" })]
    );
    assert!(!host.hosting().await);
    assert!(crate::lock(&f.system.prompts).is_empty(), "never a prompt");
}

#[test]
fn caskades_apps_are_told_by_their_command() {
    use crate::streaming::{
        api::App,
        publish::{Caskade, Offer},
    };
    let caskade = Caskade {
        program: "/opt/Caskade Games/caskade".into(),
        flatpak_host: false,
    };
    let ours = caskade.app(&Offer::Game {
        title: "Hades".into(),
        game_id: "g1".into(),
        image: None,
    });
    assert_eq!(
        ours.cmd.as_deref(),
        Some("\"/opt/Caskade Games/caskade\" --launch g1")
    );
    assert!(matches!(caskade.owns(&ours), Some(Offer::Game { game_id, .. }) if game_id == "g1"));
    let console = caskade.app(&Offer::Console);
    assert_eq!(console.cmd, None);
    assert_eq!(caskade.owns(&console), Some(Offer::Console));
    let theirs = App::command("Steam", "setsid steam steam://open/bigpicture");
    assert_eq!(caskade.owns(&theirs), None);
    // Another Caskade's (another install) isn't this one's.
    let other = Caskade {
        program: "/usr/bin/caskade".into(),
        flatpak_host: false,
    };
    assert_eq!(caskade.owns(&other.app(&Offer::Console)), None);
    // A Flatpak host's commands leave its sandbox.
    let flatpak = Caskade {
        flatpak_host: true,
        ..caskade
    };
    assert_eq!(
        flatpak.launch_command("g1"),
        "flatpak-spawn --host \"/opt/Caskade Games/caskade\" --launch g1"
    );
}

/// Hosting switched on and answering; returns once it is Ready.
async fn on(f: &Fixture) {
    let host = f.host.clone();
    let enabling = tokio::spawn(async move { host.enable().await });
    until("credentials made", || at_once(f.secrets.get()).is_some()).await;
    answer(f).await;
    assert_eq!(enabling.await.unwrap().unwrap(), HostOutcome::On);
    until("ready", || f.host.status().state == HostState::Ready).await;
}

/// The host's `/serverinfo`: streaming or not.
async fn streaming(f: &Fixture, busy: bool) {
    f.serverinfo.reset().await;
    let state = if busy {
        "SUNSHINE_SERVER_BUSY"
    } else {
        "SUNSHINE_SERVER_FREE"
    };
    Mock::given(method("GET"))
        .and(path("/serverinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "<root status_code=\"200\"><hostname>Den</hostname><uniqueid>ABC</uniqueid><state>{state}</state><currentgame>{}</currentgame></root>",
            if busy { "881" } else { "0" }
        )))
        .mount(&f.serverinfo)
        .await;
}

/// A newer release, as GitHub serves it.
async fn release(f: &Fixture, version: &str) {
    f.github.reset().await;
    Mock::given(method("HEAD"))
        .and(path("/example/host/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("/example/host/releases/tag/v{version}")),
        )
        .mount(&f.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/example/host/releases/expanded_assets/v{version}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<a href="/example/host/releases/download/v{version}/host-{version}.zip">host</a>"#
        )))
        .mount(&f.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/example/host/releases/download/v{version}/host-{version}.zip"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip(&[
            (
                &format!("host-{version}/usr/bin/sunshine"),
                &format!("{HOST}# {version}\n"),
            ),
            (
                &format!("host-{version}/usr/share/sunshine/apps.json"),
                "{}",
            ),
        ])))
        .mount(&f.github)
        .await;
}

/// The host's own files as they are after pairing a device and publishing
/// a game.
fn paired(files: &HostFiles) {
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(files.state()).unwrap()).unwrap();
    state["root"] = serde_json::json!({
        "uniqueid": "18D1CF0D",
        "named_devices": [{ "name": "Deck", "cert": "PEM", "uuid": "U1" }]
    });
    std::fs::write(files.state(), state.to_string()).unwrap();
    std::fs::write(files.apps(), r#"{"env":{},"apps":[{"name":"Celeste"}]}"#).unwrap();
}

fn kept_pairing(files: &HostFiles) -> bool {
    let state = std::fs::read_to_string(files.state()).unwrap();
    state.contains("Deck") && state.contains("18D1CF0D")
}

#[tokio::test(flavor = "multi_thread")]
async fn an_update_waits_for_the_stream_then_keeps_pairings_and_settings() {
    let f = fixture(fast()).await;
    streaming(&f, false).await;
    on(&f).await;
    let files = HostFiles::linux(&f.staging.join("config"));
    paired(&files);
    let prompts = crate::lock(&f.system.prompts).len();
    release(&f, "1.1").await;

    // Never during a stream: the host keeps running the old version.
    streaming(&f, true).await;
    assert!(f.host.update().await.is_err());
    assert!(f.host.inner.runner.running());
    assert_eq!(
        f.host
            .inner
            .tools
            .managed(TOOL)
            .await
            .unwrap()
            .unwrap()
            .installed
            .as_deref(),
        Some("v1.0")
    );

    // Once the game ends: stopped, swapped, configured and started again.
    streaming(&f, false).await;
    assert_eq!(f.host.update().await.unwrap(), HostOutcome::On);
    let managed = f.host.inner.tools.managed(TOOL).await.unwrap().unwrap();
    assert_eq!(managed.installed.as_deref(), Some("v1.1"));
    assert!(
        std::fs::read_to_string(f.host.program(false).await.unwrap())
            .unwrap()
            .contains("# 1.1")
    );
    assert!(kept_pairing(&files), "paired devices stay");
    assert!(
        std::fs::read_to_string(files.apps())
            .unwrap()
            .contains("Celeste"),
        "published games stay"
    );
    assert!(
        std::fs::read_to_string(files.conf())
            .unwrap()
            .contains("sunshine_name = Livingroom")
    );
    until("ready after the update", || {
        f.host.status().state == HostState::Ready
    })
    .await;
    // The rules were all there: no prompt.
    assert_eq!(crate::lock(&f.system.prompts).len(), prompts);
    f.host.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn repair_fixes_each_breakage() {
    let f = fixture(fast()).await;
    streaming(&f, false).await;
    on(&f).await;
    let files = HostFiles::linux(&f.staging.join("config"));
    paired(&files);
    let prompts = || crate::lock(&f.system.prompts).len();

    // A program file gone.
    let shipped = f
        .host
        .program(false)
        .await
        .unwrap()
        .parent()
        .unwrap()
        .join("../share/sunshine/apps.json");
    std::fs::remove_file(&shipped).unwrap();
    assert_eq!(f.host.repair().await.unwrap(), HostOutcome::On);
    assert!(shipped.exists());
    until("ready after files", || {
        f.host.status().state == HostState::Ready
    })
    .await;
    assert!(kept_pairing(&files));

    // The configuration drifted.
    std::fs::write(
        files.conf(),
        "sunshine_name = Someone else\nupnp = enabled\n",
    )
    .unwrap();
    f.host.repair().await.unwrap();
    let conf = std::fs::read_to_string(files.conf()).unwrap();
    assert!(conf.contains("sunshine_name = Livingroom") && conf.contains("upnp = disabled"));
    until("ready after config", || {
        f.host.status().state == HostState::Ready
    })
    .await;

    // The OS removed the rules: one prompt puts back only those.
    let before = prompts();
    crate::lock(&f.system.present).remove(&Change::UdevRules);
    f.host.repair().await.unwrap();
    assert_eq!(prompts(), before + 1);
    assert_eq!(
        crate::lock(&f.system.prompts).last().unwrap().steps,
        [Step::Add(Change::UdevRules)]
    );
    assert!(crate::lock(&f.system.present).contains(&Change::UdevRules));
    until("ready after rules", || {
        f.host.status().state == HostState::Ready
    })
    .await;

    // The keychain lost the credentials: new ones, set on the host.
    let old = f.secrets.get().await.unwrap().unwrap();
    f.secrets.forget().await.unwrap();
    let host = f.host.clone();
    let repairing = tokio::spawn(async move { host.repair().await });
    until("new credentials", || {
        at_once(f.secrets.get()).is_some_and(|new| new != old)
    })
    .await;
    f.api.reset().await;
    answer(&f).await;
    assert_eq!(repairing.await.unwrap().unwrap(), HostOutcome::On);
    let new = f.secrets.get().await.unwrap().unwrap();
    assert!(
        std::fs::read_to_string(files.state())
            .unwrap()
            .contains(&new.user)
    );
    until("ready with new credentials", || {
        f.host.status().state == HostState::Ready
    })
    .await;

    // A refused sign-in (the host's copy drifted): set again.
    f.host.shutdown();
    std::fs::write(files.state(), r#"{"username":"someone-else"}"#).unwrap();
    f.host.repair().await.unwrap();
    assert!(
        std::fs::read_to_string(files.state())
            .unwrap()
            .contains(&new.user)
    );
    until("ready after the sign-in", || {
        f.host.status().state == HostState::Ready
    })
    .await;
    f.host.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn off_leaves_no_program_or_rules_and_on_again_pairs_with_no_new_code() {
    let f = fixture(fast()).await;
    streaming(&f, false).await;
    on(&f).await;
    let files = HostFiles::linux(&f.staging.join("config"));
    paired(&files);
    let credentials = f.secrets.get().await.unwrap().unwrap();

    assert_eq!(f.host.disable().await.unwrap(), HostOutcome::Off);
    // No program file, rule or record of the administrator step remains.
    let program = f.host.program(false).await;
    assert!(program.is_err(), "{program:?}");
    assert!(crate::lock(&f.system.present).is_empty());
    assert!(!f.staging.join("host-1.0").exists());
    assert!(!f.staging.join(".caskade/shipped.json").exists());
    assert!(
        f.host
            .inner
            .elevation
            .record(Plan::StreamingHost)
            .unwrap()
            .added
            .is_empty()
    );
    assert!(f.host.inner.tools.managed(TOOL).await.unwrap().is_none());
    // Its paired devices and sign-in stay.
    assert!(kept_pairing(&files));

    // On again: installed again, the same identity, nothing to pair.
    assert_eq!(f.host.enable().await.unwrap(), HostOutcome::On);
    assert!(f.host.program(false).await.unwrap().is_file());
    assert!(kept_pairing(&files));
    assert_eq!(f.secrets.get().await.unwrap().unwrap(), credentials);
    until("ready again", || f.host.status().state == HostState::Ready).await;
    f.host.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ready_host_says_when_pads_or_a_screen_wont_work() {
    let f = fixture(fast()).await;
    let host = |checks: fn(OperatingSystem) -> HostChecks| {
        StreamingHost::with(
            f.host.inner.tools.clone(),
            f.host.inner.elevation.clone(),
            f.settings.clone(),
            Parts {
                existing: Arc::new(|| None),
                os: OperatingSystem::Linux,
                runner: Arc::new(process::ChildRunner::new(
                    f.temp.path().join("data/streaming/run.json"),
                )),
                secrets: f.secrets.clone(),
                timing: fast(),
                api_base: f.host.inner.api_base.clone(),
                http_port: f.host.inner.http_port,
                checks: Arc::new(checks),
            },
        )
    };
    let no_pads = host(|_| HostChecks {
        screen: true,
        pads: false,
        reachable: true,
    });
    let enabling = tokio::spawn({
        let host = no_pads.clone();
        async move { host.enable().await }
    });
    until("credentials made", || at_once(f.secrets.get()).is_some()).await;
    answer(&f).await;
    enabling.await.unwrap().unwrap();
    until("ready", || no_pads.status().state == HostState::Ready).await;
    assert_eq!(
        no_pads.status().warning.as_deref(),
        Some("Controllers on other devices won't work here until Repair sets them up.")
    );
    no_pads.shutdown();

    let headless = host(|_| HostChecks {
        screen: false,
        pads: true,
        reachable: true,
    });
    headless.start().await;
    until("ready", || headless.status().state == HostState::Ready).await;
    assert!(headless.status().warning.unwrap().contains("no screen"));
    headless.shutdown();

    let fine = host(HostChecks::fine);
    fine.start().await;
    until("ready", || fine.status().state == HostState::Ready).await;
    assert_eq!(fine.status().warning, None);
    fine.shutdown();
    assert!(
        HostChecks {
            screen: true,
            pads: false,
            reachable: true,
        }
        .warning(OperatingSystem::Windows)
        .unwrap()
        .contains("controller driver")
    );
}

#[test]
fn a_windows_pc_only_on_public_networks_is_told_nobody_can_reach_it() {
    assert!(network_lets_others_in("Public\r\nPrivate\r\n"));
    assert!(network_lets_others_in("DomainAuthenticated\r\n"));
    assert!(!network_lets_others_in("Public\r\nPublic\r\n"));
    let checks = HostChecks {
        reachable: false,
        ..HostChecks::fine(OperatingSystem::Windows)
    };
    let warning = checks.warning(OperatingSystem::Windows).unwrap();
    assert!(warning.contains("public"), "{warning}");
}
