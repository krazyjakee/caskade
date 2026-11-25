//! Live tests for the real streaming host and client on Linux (ignored):
//! downloaded, unpacked, configured, paired, streamed, and stopped. The
//! administrator step is stood in for (nobody types a password in a test); run
//! them under Xvfb:
//! `xvfb-run -a cargo test -p caskade-sources --features live-tests --test it streaming:: -- --ignored --nocapture`

#![cfg(all(feature = "live-tests", target_os = "linux"))]

mod host_and_client;
mod pairing;
mod playback;

use caskade_core::{
    AppPaths,
    db::{LibraryStore, StreamApp},
    emulation::OperatingSystem,
    launch::LaunchService,
    managed::{CancellationToken, ManagedTools},
    model::{GameEntry, LaunchSpec},
    settings::SettingsService,
};
use caskade_streaming::{
    HostOutcome, HostState, StreamingClient, StreamingHost,
    client::{SETTINGS_DIR, SETTINGS_FILE},
    elevation::{Change, Elevation, Prompt, Request, Runner},
    host::{
        Parts, Timing,
        process::{ChildRunner, HostRunner},
    },
    hosts::{ask_server_info, paired_hosts},
    pairing::Paired,
    play::Streams,
    secrets::HostSecrets,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

pub(super) struct Allowed;

impl Runner for Allowed {
    fn present(&self, _: Change) -> bool {
        false
    }
    fn run(&self, _: &Request) -> caskade_core::Result<Prompt> {
        Ok(Prompt::Done)
    }
}

pub(super) async fn wait_host_ready(host: &StreamingHost) {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while host.status().state != HostState::Ready {
        assert!(
            std::time::Instant::now() < deadline
                && host.status().state != HostState::NeedsAttention,
            "{:?}",
            host.status()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

pub(super) async fn start_host(
    tools: ManagedTools,
    settings: SettingsService,
    paths: &AppPaths,
    name: &str,
    runner: Option<Arc<dyn HostRunner>>,
    secrets: HostSecrets,
) -> StreamingHost {
    let runner =
        runner.unwrap_or_else(|| Arc::new(ChildRunner::new(paths.data.join("streaming/run.json"))));
    let host = StreamingHost::with(
        tools,
        Elevation::with_runner(Arc::new(Allowed), OperatingSystem::Linux, paths),
        settings,
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner,
            secrets,
            timing: Timing::default(),
            api_base: caskade_streaming::api::LOCAL.parse().unwrap(),
            http_port: caskade_streaming::host::HTTP_PORT,
            checks: Arc::new(caskade_streaming::host::HostChecks::here),
        },
    );
    host.set_device_name(name);
    assert_eq!(host.enable().await.unwrap(), HostOutcome::On);
    wait_host_ready(&host).await;
    host
}

pub(super) async fn attempt_pairing(
    host: &StreamingHost,
    client: &StreamingClient,
    code: &'static str,
    accept: Option<&'static str>,
) -> Paired {
    let pairing = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .pair(
                    "127.0.0.1",
                    code,
                    Duration::from_secs(60),
                    &CancellationToken::new(),
                )
                .await
        }
    });
    let request = loop {
        if let Some(request) = host.pair_requests().await.ok().and_then(|mut r| r.pop()) {
            break request;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    match accept {
        Some(pin) => host
            .accept_pairing(&request.id, pin, "Living room")
            .await
            .unwrap(),
        None => host.decline_pairing(&request.id).await.unwrap(),
    }
    pairing.await.unwrap().unwrap()
}

pub(super) async fn pair_client(host: &StreamingHost, client: &StreamingClient) {
    assert!(matches!(
        attempt_pairing(host, client, "4831", Some("4831")).await,
        Paired::Paired { .. }
    ));
}

pub(super) async fn paired_host_id(client: &StreamingClient, host_name: &str) -> (PathBuf, String) {
    let state = client.state().await.unwrap().unwrap();
    let text = std::fs::read_to_string(state.join(SETTINGS_DIR).join(SETTINGS_FILE)).unwrap();
    let host_id = paired_hosts(&text)
        .into_iter()
        .find(|h| h.name == host_name)
        .unwrap()
        .id;
    (state, host_id)
}

pub(super) async fn stream_entry(store: &LibraryStore, title: &str) -> GameEntry {
    let games = store.list_games(&Default::default()).await.unwrap();
    let game = games.items.iter().find(|g| g.title == title).unwrap();
    let entry = store.entries_for_game(&game.id).await.unwrap().remove(0);
    assert!(matches!(entry.launch, Some(LaunchSpec::Stream { .. })));
    entry
}

pub(super) async fn setup_stream_launch(
    store: &LibraryStore,
    client: &StreamingClient,
    settings: SettingsService,
    host_id: &str,
    title: &str,
) -> (GameEntry, LaunchService) {
    store
        .sync_stream_apps(
            host_id,
            &[StreamApp {
                id: "1".into(),
                name: title.into(),
            }],
        )
        .await
        .unwrap();
    let entry = stream_entry(store, title).await;
    let launch = LaunchService::new(store.clone());
    launch.set_streamer(Arc::new(Streams::new(client.clone(), None, settings)));
    (entry, launch)
}

pub(super) async fn is_host_busy() -> bool {
    ask_server_info("127.0.0.1", 47989)
        .await
        .is_some_and(|info| info.busy)
}

pub(super) fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_default()
}

/// Every process whose working directory is `dir`: the client runs in its
/// state folder, so none left there means no client left behind.
pub(super) fn running_in(dir: &std::path::Path) -> Vec<u32> {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_owned());
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|entry| {
            let pid: u32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
            (std::fs::read_link(format!("/proc/{pid}/cwd")).ok()? == dir).then_some(pid)
        })
        .collect()
}

/// The names of the windows shown on the test's display (`xwininfo`), as
/// `name WxH`.
pub(super) fn visible_windows() -> Vec<String> {
    let Ok(tree) = std::process::Command::new("xwininfo")
        .args(["-root", "-children"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&tree.stdout)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (id, rest) = line.split_once(' ')?;
            id.starts_with("0x").then_some(())?;
            let info = std::process::Command::new("xwininfo")
                .args(["-id", id])
                .output()
                .ok()?;
            let info = String::from_utf8_lossy(&info.stdout);
            info.contains("Map State: IsViewable").then_some(())?;
            let name = rest.split('"').nth(1).unwrap_or("(no name)");
            let size = rest
                .split_whitespace()
                .find(|part| part.contains('x') && part.contains('+'))?;
            Some(format!("{name} {}", size.split('+').next().unwrap_or(size)))
        })
        .collect()
}

/// Waits up to 30 s for `done`, naming `what` when it never comes.
pub(super) async fn wait_until(what: &str, mut done: impl AsyncFnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !done().await {
        assert!(std::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Every process running `caskade --launch <game_id>`.
pub(super) fn launch_callers(game_id: &str) -> Vec<u32> {
    let wanted = format!("--launch\0{game_id}\0");
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|entry| {
            let pid: u32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
            let line = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            String::from_utf8_lossy(&line)
                .contains(&wanted)
                .then_some(pid)
        })
        .collect()
}

/// The real Caskade, on a D-Bus session of its own: on the user's, the
/// single-instance plugin would hand the start to their Caskade. Stopped
/// with everything it started; a failed test prints its output.
pub(super) struct RealCaskade {
    child: std::process::Child,
    log: PathBuf,
}

impl RealCaskade {
    pub(super) fn start(program: &std::path::Path, env: &[(String, PathBuf)]) -> Self {
        use std::os::unix::process::CommandExt;
        let log = env[0].1.join("caskade.log");
        let output = std::fs::File::create(&log).unwrap();
        let mut command = std::process::Command::new("dbus-run-session");
        command
            .arg("--")
            .arg(program)
            .stdin(std::process::Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .process_group(0);
        caskade_core::managed::outside_own_appimage(&mut command);
        // Any WebKit setting stops it restarting itself for the GPU.
        command
            .envs(env.iter().map(|(key, value)| (key, value)))
            .env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        Self {
            child: command.spawn().unwrap(),
            log,
        }
    }
}

impl Drop for RealCaskade {
    fn drop(&mut self) {
        let group = format!("-{}", self.child.id());
        let signal = |name: &str| {
            let _ = std::process::Command::new("kill")
                .args(["-s", name, "--", &group])
                .status();
        };
        signal("TERM");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        signal("KILL");
        let _ = self.child.wait();
        if std::thread::panicking() {
            let text = std::fs::read_to_string(&self.log).unwrap_or_default();
            println!("Caskade's output:\n{text}");
        }
    }
}
