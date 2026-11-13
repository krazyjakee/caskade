//! Hosting on and off against a fake release (a shell script standing in
//! for the host), the administrator step injected, the real child process
//! runner, and the API behind a TLS front.

use super::*;
use crate::{
    elevation::{Change, Prompt, Request, Runner, Step},
    testing::{Apps, cert, tls_front},
};
use caskade_core::{
    AppPaths,
    bus::{Bus, BusOptions},
    db::LibraryStore,
    managed::{Hosts, ToolCatalog, ToolDefinition},
    tracking::ProcessIdentity,
    updates::GitHub,
};
use std::{collections::BTreeSet, io::Write, time::Instant};
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
        caskade_core::lock(&self.present).contains(&change)
    }

    fn run(&self, request: &Request) -> Result<Prompt> {
        caskade_core::lock(&self.prompts).push(request.clone());
        let answer = caskade_core::lock(&self.answer).unwrap_or(Prompt::Done);
        if answer == Prompt::Done {
            let mut present = caskade_core::lock(&self.present);
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
    let machine = caskade_core::managed::Host {
        os: OperatingSystem::Linux,
        arch: caskade_core::managed::Arch::X86_64,
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
        look_every: Duration::from_millis(50),
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

mod checks;
mod existing;
mod lifecycle;
mod update_repair;
