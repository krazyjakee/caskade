//! The real streaming host on this Linux machine (ignored): downloaded,
//! unpacked, configured, given credentials, started, answering through
//! the pinned certificate, and stopped. The administrator step is stood in
//! for (nobody types a password in a test); run it under Xvfb:
//! `xvfb-run -a cargo test -p caskade-sources --features live-tests --test it streaming::live_ -- --ignored --nocapture`

#[cfg(all(feature = "live-tests", target_os = "linux"))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host on ports 47984-48010"]
async fn live_linux_host_answers_pinned_and_stops_with_caskade() {
    use caskade_core::{
        AppPaths,
        bus::Bus,
        db::LibraryStore,
        elevation::{Change, Elevation, Prompt, Request, Runner},
        emulation::OperatingSystem,
        managed::ManagedTools,
        settings::SettingsService,
        streaming::{
            HostOutcome, HostState, StreamingHost,
            api::HostApi,
            host::{Parts, Timing, process::ChildRunner},
            secrets::HostSecrets,
        },
    };
    use std::{sync::Arc, time::Duration};

    struct Allowed;
    impl Runner for Allowed {
        fn present(&self, _: Change) -> bool {
            false
        }
        fn run(&self, _: &Request) -> caskade_core::Result<Prompt> {
            Ok(Prompt::Done)
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let tools = ManagedTools::new(
        store,
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &paths,
    );
    let secrets = HostSecrets::memory();
    let host = StreamingHost::with(
        tools,
        Elevation::with_runner(Arc::new(Allowed), OperatingSystem::Linux, &paths),
        settings,
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner: Arc::new(ChildRunner::new(paths.data.join("streaming/run.json"))),
            secrets: secrets.clone(),
            timing: Timing::default(),
            api_base: caskade_core::streaming::api::LOCAL.parse().unwrap(),
            http_port: caskade_core::streaming::host::HTTP_PORT,
            checks: Arc::new(caskade_core::streaming::host::HostChecks::here),
        },
    );
    host.set_device_name("Caskade live test");
    let home_config = dirs_home().join(".config/sunshine");
    let had_home_config = home_config.exists();

    assert_eq!(host.enable().await.unwrap(), HostOutcome::On);
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
    println!("host ready: {:?}", host.status());
    let api = host.api().await.unwrap();
    assert!(api.apps().await.unwrap().is_empty(), "none of its defaults");
    // Any other certificate is refused.
    let other = std::fs::read_dir("/etc/ssl/certs")
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "pem"))
        .unwrap();
    let wrong = HostApi::with_cert_file(
        caskade_core::streaming::api::LOCAL.parse().unwrap(),
        secrets.get().await.unwrap().unwrap(),
        &other,
    )
    .unwrap();
    assert!(wrong.version().await.is_err());
    // It wrote nothing under the user's own Sunshine folder.
    assert_eq!(home_config.exists(), had_home_config);

    // Another device finds it on the network (#262): mDNS through the
    // system's responder, named by its /serverinfo as Caskade set it.
    let hosts = caskade_core::streaming::hosts::StreamHosts::new(
        LibraryStore::in_memory().await.unwrap(),
        Bus::default(),
        {
            let store = LibraryStore::in_memory().await.unwrap();
            let settings = SettingsService::new(store);
            settings.register_core(&paths).unwrap();
            settings
        },
        caskade_core::streaming::StreamingClient::new(ManagedTools::new(
            LibraryStore::in_memory().await.unwrap(),
            Bus::default(),
            caskade_sources::tools::catalog().unwrap(),
            &AppPaths::under(&temp.path().join("client")),
        )),
    )
    .unwrap();
    let mut found = Vec::new();
    for _ in 0..10 {
        hosts.refresh().await.unwrap();
        found = hosts.hosts();
        if found.iter().any(|host| host.name == "Caskade live test") {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!("found on the network: {found:?}");
    assert!(
        found.iter().any(|host| host.name == "Caskade live test"
            && host.reach == caskade_core::streaming::hosts::HostReach::Ready),
        "{found:?}"
    );

    host.shutdown();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(api.version().await.is_err(), "stopped with Caskade");
    assert_eq!(host.disable().await.unwrap(), HostOutcome::Off);
}

#[cfg(all(feature = "live-tests", target_os = "linux"))]
fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_default()
}

/// The real client (ignored): installed, given its own state, and run
/// once; the user's own Moonlight settings stay as they were, and Caskade's
/// state holds what Moonlight wrote (its certificate) beside the quiet
/// settings. Run under Xvfb like the host test.
#[cfg(all(feature = "live-tests", target_os = "linux"))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming client"]
async fn live_linux_client_keeps_its_own_state() {
    use caskade_core::{
        AppPaths,
        bus::Bus,
        db::LibraryStore,
        managed::{CancellationToken, ManagedTools},
        streaming::client::{SETTINGS_DIR, SETTINGS_FILE, StreamingClient},
    };
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let theirs = home.join(".config/Moonlight Game Streaming Project/Moonlight.conf");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, "[hosts]\nsize=1\n1\\hostname=Office\n").unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let tools = ManagedTools::new(
        store,
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &AppPaths::under(temp.path()),
    );
    let client = StreamingClient::new(tools);
    let ready = client.ensure(&CancellationToken::new()).await.unwrap();
    println!("client: {:?}", ready.install);
    let mut command = client
        .command(&ready, &["list".into(), "127.0.0.1".into(), "--csv".into()])
        .unwrap();
    command
        .env("HOME", &home)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().unwrap();
    // It waits for a host that isn't there; asked to quit, it saves its
    // settings (a killed Moonlight saves nothing).
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(
        std::fs::read_to_string(&theirs).unwrap(),
        "[hosts]\nsize=1\n1\\hostname=Office\n"
    );
    let ours = std::fs::read_to_string(ready.state.join(SETTINGS_DIR).join(SETTINGS_FILE)).unwrap();
    assert!(
        ours.contains("certificate="),
        "Moonlight used Caskade's state: {ours}"
    );
    for quiet in [
        "richpresence=false",
        "confwarnings=false",
        "uidisplaymode=2",
    ] {
        assert!(ours.contains(quiet), "{quiet}");
    }
}

/// The real client pairs with the real host natively (#263): Caskade's
/// own exchange with Moonlight's identity, the code accepted through the
/// host's API, no window of Moonlight's. Moonlight then lists the host's
/// apps from the record Caskade wrote, so it counts the host as paired. A
/// wrong code and a declined request pair nothing. Run under Xvfb.
#[cfg(all(feature = "live-tests", target_os = "linux"))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client"]
async fn live_linux_pairing_needs_no_window() {
    use caskade_core::{
        AppPaths,
        bus::Bus,
        db::LibraryStore,
        elevation::{Change, Elevation, Prompt, Request, Runner},
        emulation::OperatingSystem,
        managed::{CancellationToken, ManagedTools},
        settings::SettingsService,
        streaming::{
            HostState, StreamingClient, StreamingHost,
            api::App,
            client::{SETTINGS_DIR, SETTINGS_FILE},
            host::{Parts, Timing, process::ChildRunner},
            hosts::paired_hosts,
            pairing::Paired,
            secrets::HostSecrets,
        },
    };
    use std::{sync::Arc, time::Duration};

    struct Allowed;
    impl Runner for Allowed {
        fn present(&self, _: Change) -> bool {
            false
        }
        fn run(&self, _: &Request) -> caskade_core::Result<Prompt> {
            Ok(Prompt::Done)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let tools = ManagedTools::new(
        store,
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &paths,
    );
    let host = StreamingHost::with(
        tools.clone(),
        Elevation::with_runner(Arc::new(Allowed), OperatingSystem::Linux, &paths),
        settings,
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner: Arc::new(ChildRunner::new(paths.data.join("streaming/run.json"))),
            secrets: HostSecrets::memory(),
            timing: Timing::default(),
            api_base: caskade_core::streaming::api::LOCAL.parse().unwrap(),
            http_port: caskade_core::streaming::host::HTTP_PORT,
            checks: Arc::new(caskade_core::streaming::host::HostChecks::here),
        },
    );
    host.set_device_name("Caskade pairing test");
    host.enable().await.unwrap();
    while host.status().state != HostState::Ready {
        assert_ne!(host.status().state, HostState::NeedsAttention);
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let api = host.api().await.unwrap();
    api.add_app(&App::command("Live Game", "true"))
        .await
        .unwrap();
    let client = StreamingClient::new(tools);

    // Pairs, accepting `accept` on the host for a client showing `code`.
    let attempt = |code: &'static str, accept: Option<&'static str>| {
        let (client, host) = (client.clone(), host.clone());
        async move {
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
    };
    assert_eq!(attempt("1111", Some("2222")).await, Paired::WrongCode);
    assert_eq!(attempt("3333", None).await, Paired::Declined);
    assert!(matches!(
        attempt("4831", Some("4831")).await,
        Paired::Paired { .. }
    ));
    let devices = host.paired_devices().await.unwrap();
    println!("host's paired devices: {devices:?}");
    assert!(devices.iter().any(|device| device.name == "Living room"));

    // Moonlight lists the paired host's apps from Caskade's record.
    client.list("127.0.0.1").await.unwrap();
    let state = client.state().await.unwrap().unwrap();
    let text = std::fs::read_to_string(state.join(SETTINGS_DIR).join(SETTINGS_FILE)).unwrap();
    let hosts = paired_hosts(&text);
    println!(
        "client's hosts: {:?}",
        hosts.iter().map(|h| (&h.name, &h.apps)).collect::<Vec<_>>()
    );
    let paired = hosts
        .iter()
        .find(|h| h.name == "Caskade pairing test")
        .unwrap();
    assert!(
        paired.apps.iter().any(|app| app.name == "Live Game"),
        "{hosts:?}"
    );
    host.shutdown();
}

/// Every process whose working directory is `dir`: the client runs in its
/// state folder, so none left there means no client left behind.
#[cfg(all(feature = "live-tests", target_os = "linux"))]
fn running_in(dir: &std::path::Path) -> Vec<u32> {
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
#[cfg(all(feature = "live-tests", target_os = "linux"))]
fn visible_windows() -> Vec<String> {
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

/// #264: Play streams a game from the real host through the launch
/// pipeline. The host checks come first; the client shows only its stream
/// window; ending the stream stops the game on the host and leaves no
/// client; a game the host doesn't offer and a host that's gone fail in
/// Caskade's words, with no client left. Nothing is stored on the client.
/// Run under Xvfb (`xvfb-run -a -s "-screen 0 1280x720x24"`).
#[cfg(all(feature = "live-tests", target_os = "linux"))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client"]
async fn live_linux_streaming_plays_from_the_host() {
    use caskade_core::{
        AppPaths,
        bus::Bus,
        db::{LibraryStore, StreamApp},
        elevation::{Change, Elevation, Prompt, Request, Runner},
        emulation::OperatingSystem,
        launch::{LaunchError, LaunchService, SessionStatus},
        managed::{CancellationToken, ManagedTools},
        model::LaunchSpec,
        settings::SettingsService,
        streaming::{
            HostState, StreamingClient, StreamingHost,
            api::App,
            client::{SETTINGS_DIR, SETTINGS_FILE},
            host::{Parts, Timing, process::ChildRunner},
            hosts::{ask_server_info, paired_hosts},
            pairing::Paired,
            play::Streams,
            secrets::HostSecrets,
        },
    };
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    struct Allowed;
    impl Runner for Allowed {
        fn present(&self, _: Change) -> bool {
            false
        }
        fn run(&self, _: &Request) -> caskade_core::Result<Prompt> {
            Ok(Prompt::Done)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let tools = ManagedTools::new(
        store.clone(),
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &paths,
    );
    let host = StreamingHost::with(
        tools.clone(),
        Elevation::with_runner(Arc::new(Allowed), OperatingSystem::Linux, &paths),
        settings.clone(),
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner: Arc::new(ChildRunner::new(paths.data.join("streaming/run.json"))),
            secrets: HostSecrets::memory(),
            timing: Timing::default(),
            api_base: caskade_core::streaming::api::LOCAL.parse().unwrap(),
            http_port: caskade_core::streaming::host::HTTP_PORT,
            checks: Arc::new(caskade_core::streaming::host::HostChecks::here),
        },
    );
    host.set_device_name("Caskade streaming test");
    host.enable().await.unwrap();
    while host.status().state != HostState::Ready {
        assert_ne!(host.status().state, HostState::NeedsAttention);
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let api = host.api().await.unwrap();
    // The host's apps run absolute programs, as Caskade's own do.
    api.add_app(&App::command("Live Game", "/bin/sleep 600"))
        .await
        .unwrap();
    let client = StreamingClient::new(tools);
    let pairing = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .pair(
                    "127.0.0.1",
                    "4831",
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
    host.accept_pairing(&request.id, "4831", "Living room")
        .await
        .unwrap();
    assert!(matches!(
        pairing.await.unwrap().unwrap(),
        Paired::Paired { .. }
    ));
    let state = client.state().await.unwrap().unwrap();
    let text = std::fs::read_to_string(state.join(SETTINGS_DIR).join(SETTINGS_FILE)).unwrap();
    let host_id = paired_hosts(&text)
        .into_iter()
        .find(|h| h.name == "Caskade streaming test")
        .unwrap()
        .id;

    // The host's game, and one it doesn't offer, as the library has them.
    let apps = [
        StreamApp {
            id: "1".into(),
            name: "Live Game".into(),
        },
        StreamApp {
            id: "2".into(),
            name: "Gone Game".into(),
        },
    ];
    store.sync_stream_apps(&host_id, &apps).await.unwrap();
    let entry = |name: &'static str| {
        let store = store.clone();
        async move {
            let games = store.list_games(&Default::default()).await.unwrap();
            let game = games.items.iter().find(|g| g.title == name).unwrap();
            let entry = store.entries_for_game(&game.id).await.unwrap().remove(0);
            assert!(matches!(entry.launch, Some(LaunchSpec::Stream { .. })));
            entry
        }
    };
    let live = entry("Live Game").await;
    let gone = entry("Gone Game").await;

    let launch = LaunchService::new(store.clone());
    // Caskade's window fills the screen, as in console mode.
    launch.set_streamer(Arc::new(
        Streams::new(client.clone(), None, settings).with_placement(|| {
            Some(caskade_core::launch::WindowPlacement {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            })
        }),
    ));
    let busy = || async {
        ask_server_info("127.0.0.1", 47989)
            .await
            .is_some_and(|info| info.busy)
    };

    // Windows seen while the stream starts, plays and ends.
    let seen = Arc::new(Mutex::new(Vec::<(Duration, Vec<String>)>::new()));
    let watching = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let start = Instant::now();
    let sampler = std::thread::spawn({
        let (seen, watching) = (seen.clone(), watching.clone());
        move || {
            while watching.load(std::sync::atomic::Ordering::SeqCst) {
                let windows = visible_windows();
                seen.lock().unwrap().push((start.elapsed(), windows));
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });

    let handle = launch.launch(&live.id).await.unwrap();
    let streaming_at = start.elapsed();
    println!("streaming after {streaming_at:?}");
    assert!(busy().await, "the host plays the game");
    assert_eq!(launch.running_streams(None).len(), 1);
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!running_in(&state).is_empty(), "the client streams");
    let streaming = visible_windows();
    println!("windows while streaming: {streaming:?}");
    assert!(
        streaming
            .iter()
            .any(|name| name.starts_with("Caskade streaming test - Moonlight")),
        "the stream shows: {streaming:?}"
    );
    // End stream: the host's game stops and no client is left.
    assert_eq!(
        launch
            .stop(&handle.ticket.session_id, Duration::from_secs(5))
            .await
            .unwrap(),
        1
    );
    let (_, status) = handle.wait_status().await.unwrap();
    assert_eq!(status, SessionStatus::Exited);
    let deadline = Instant::now() + Duration::from_secs(10);
    while busy().await && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(!busy().await, "the game stopped on the host");
    assert!(running_in(&state).is_empty(), "no client is left");
    tokio::time::sleep(Duration::from_secs(1)).await;
    watching.store(false, std::sync::atomic::Ordering::SeqCst);
    sampler.join().unwrap();
    let seen = seen.lock().unwrap().clone();
    let windows: std::collections::BTreeSet<&String> =
        seen.iter().flat_map(|(_, names)| names).collect();
    println!("windows seen: {windows:?}");

    // Only the stream's own window shows: the client's start window is
    // one pixel (under Caskade's), and none is left after.
    assert!(
        windows.iter().all(|name| {
            name.starts_with("Caskade streaming test - Moonlight") || name.ends_with(" 1x1")
        }),
        "{windows:?}"
    );
    assert!(seen.last().unwrap().1.is_empty(), "{:?}", seen.last());
    assert!(
        store
            .sessions_for_game(&live.game_id, 10)
            .await
            .unwrap()
            .is_empty(),
        "playtime is the host's"
    );

    // A game the host doesn't offer.
    let Err(LaunchError::Refused(text)) = launch.launch(&gone.id).await else {
        panic!("a game the host doesn't offer can't stream");
    };
    assert_eq!(text, "Caskade streaming test no longer offers Gone Game.");
    assert!(running_in(&state).is_empty());

    // The host forgot this device.
    let device = host
        .paired_devices()
        .await
        .unwrap()
        .into_iter()
        .find(|device| device.name == "Living room")
        .unwrap();
    host.unpair(&device.uuid).await.unwrap();
    let Err(LaunchError::Refused(text)) = launch.launch(&live.id).await else {
        panic!("a host that forgot this device can't stream");
    };
    assert_eq!(
        text,
        "Caskade streaming test no longer knows this device. Pair them again in Settings → Streaming."
    );
    assert!(running_in(&state).is_empty());

    // The host gone.
    host.shutdown();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let Err(LaunchError::Refused(text)) = launch.launch(&live.id).await else {
        panic!("a host that's gone can't stream");
    };
    println!("host gone: {text}");
    assert!(text.contains("isn't answering"), "{text}");
    assert!(running_in(&state).is_empty());
}

/// #266: pairings survive switching hosting off and on with the real host
/// and client. Off removes the host program (its configuration and paired
/// devices stay); on again installs it, and the device paired before
/// streams with no new code. Run under Xvfb.
#[cfg(all(feature = "live-tests", target_os = "linux"))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client"]
async fn live_linux_pairing_survives_hosting_off_and_on() {
    use caskade_core::{
        AppPaths,
        bus::Bus,
        db::{LibraryStore, StreamApp},
        elevation::{Change, Elevation, Prompt, Request, Runner},
        emulation::OperatingSystem,
        launch::LaunchService,
        managed::{CancellationToken, ManagedTools},
        settings::SettingsService,
        streaming::{
            HostOutcome, HostState, StreamingClient, StreamingHost,
            api::App,
            client::{SETTINGS_DIR, SETTINGS_FILE},
            host::{Parts, Timing, process::ChildRunner},
            hosts::{ask_server_info, paired_hosts},
            pairing::Paired,
            play::Streams,
            secrets::HostSecrets,
        },
    };
    use std::{sync::Arc, time::Duration};

    struct Allowed;
    impl Runner for Allowed {
        fn present(&self, _: Change) -> bool {
            false
        }
        fn run(&self, _: &Request) -> caskade_core::Result<Prompt> {
            Ok(Prompt::Done)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let tools = ManagedTools::new(
        store.clone(),
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &paths,
    );
    let host = StreamingHost::with(
        tools.clone(),
        Elevation::with_runner(Arc::new(Allowed), OperatingSystem::Linux, &paths),
        settings.clone(),
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner: Arc::new(ChildRunner::new(paths.data.join("streaming/run.json"))),
            secrets: HostSecrets::memory(),
            timing: Timing::default(),
            api_base: caskade_core::streaming::api::LOCAL.parse().unwrap(),
            http_port: caskade_core::streaming::host::HTTP_PORT,
            checks: Arc::new(caskade_core::streaming::host::HostChecks::here),
        },
    );
    host.set_device_name("Caskade off and on");
    let ready = |host: StreamingHost| async move {
        while host.status().state != HostState::Ready {
            assert_ne!(host.status().state, HostState::NeedsAttention);
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    assert_eq!(host.enable().await.unwrap(), HostOutcome::On);
    ready(host.clone()).await;
    host.api()
        .await
        .unwrap()
        .add_app(&App::command("Live Game", "/bin/sleep 600"))
        .await
        .unwrap();
    let client = StreamingClient::new(tools.clone());
    let pairing = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .pair(
                    "127.0.0.1",
                    "4831",
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
    host.accept_pairing(&request.id, "4831", "Living room")
        .await
        .unwrap();
    assert!(matches!(
        pairing.await.unwrap().unwrap(),
        Paired::Paired { .. }
    ));

    // Off: the program goes, nothing answers.
    assert_eq!(host.disable().await.unwrap(), HostOutcome::Off);
    assert!(tools.find("sunshine").await.unwrap().is_none());
    assert!(ask_server_info("127.0.0.1", 47989).await.is_none());

    // On again: installed again, and the pairing still holds.
    assert_eq!(host.enable().await.unwrap(), HostOutcome::On);
    ready(host.clone()).await;
    let devices = host.paired_devices().await.unwrap();
    assert!(
        devices.iter().any(|device| device.name == "Living room"),
        "{devices:?}"
    );
    let state = client.state().await.unwrap().unwrap();
    let text = std::fs::read_to_string(state.join(SETTINGS_DIR).join(SETTINGS_FILE)).unwrap();
    let host_id = paired_hosts(&text)
        .into_iter()
        .find(|h| h.name == "Caskade off and on")
        .unwrap()
        .id;
    store
        .sync_stream_apps(
            &host_id,
            &[StreamApp {
                id: "1".into(),
                name: "Live Game".into(),
            }],
        )
        .await
        .unwrap();
    let games = store.list_games(&Default::default()).await.unwrap();
    let game = games.items.iter().find(|g| g.title == "Live Game").unwrap();
    let entry = store.entries_for_game(&game.id).await.unwrap().remove(0);
    let launch = LaunchService::new(store.clone());
    launch.set_streamer(Arc::new(Streams::new(client.clone(), None, settings)));
    // No code: the stream starts.
    let handle = launch.launch(&entry.id).await.unwrap();
    assert!(
        ask_server_info("127.0.0.1", 47989)
            .await
            .is_some_and(|info| info.busy)
    );
    launch
        .stop(&handle.ticket.session_id, Duration::from_secs(5))
        .await
        .unwrap();
    handle.wait_status().await.unwrap();
    host.shutdown();
}

/// #267: two devices paired through sync stream with no code. The client
/// starts pairing with the real host and hands its code over sync; the
/// host's device enters it. The device then streams a game. Run under Xvfb.
#[cfg(all(feature = "live-tests", target_os = "linux"))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client"]
async fn live_linux_sync_paired_devices_stream_with_no_code() {
    use caskade_core::{
        AppPaths,
        accounts::AccountService,
        bus::Bus,
        db::{LibraryStore, StreamApp},
        devices::{Devices, MemoryKeyStore, Reach},
        elevation::{Change, Elevation, Prompt, Request, Runner},
        emulation::OperatingSystem,
        launch::LaunchService,
        managed::{CancellationToken, ManagedTools},
        settings::SettingsService,
        streaming::{
            HostOutcome, HostState, StreamingClient, StreamingHost,
            api::App,
            host::{Parts, Timing, process::ChildRunner},
            hosts::ask_server_info,
            linked::{HostHandover, LinkedPairing},
            pairing::Paired,
            play::Streams,
            secrets::HostSecrets,
        },
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    struct Allowed;
    impl Runner for Allowed {
        fn present(&self, _: Change) -> bool {
            false
        }
        fn run(&self, _: &Request) -> caskade_core::Result<Prompt> {
            Ok(Prompt::Done)
        }
    }
    async fn device(name: &str, dir: &std::path::Path) -> (Devices, LibraryStore, SettingsService) {
        let store = LibraryStore::in_memory().await.unwrap();
        let accounts = AccountService::open(store.clone(), Bus::default())
            .await
            .unwrap();
        let settings = SettingsService::new(store.clone());
        settings.register_core(&AppPaths::under(dir)).unwrap();
        let devices = Devices::with_reach(
            store.clone(),
            accounts,
            Arc::new(MemoryKeyStore::default()),
            Reach::loopback(),
        );
        devices.set_settings(settings.clone());
        devices.start();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !devices.status().await.this_device.listening {
            assert!(Instant::now() < deadline, "the endpoint starts");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        devices.rename(name).await.unwrap();
        (devices, store, settings)
    }

    // The host's device.
    let host_dir = tempfile::tempdir().unwrap();
    let (host_devices, host_store, host_settings) = device("Den", host_dir.path()).await;
    let host_paths = AppPaths::under(host_dir.path());
    let host_tools = ManagedTools::new(
        host_store,
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &host_paths,
    );
    let host = StreamingHost::with(
        host_tools,
        Elevation::with_runner(Arc::new(Allowed), OperatingSystem::Linux, &host_paths),
        host_settings,
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner: Arc::new(ChildRunner::new(host_paths.data.join("streaming/run.json"))),
            secrets: HostSecrets::memory(),
            timing: Timing::default(),
            api_base: caskade_core::streaming::api::LOCAL.parse().unwrap(),
            http_port: caskade_core::streaming::host::HTTP_PORT,
            checks: Arc::new(caskade_core::streaming::host::HostChecks::here),
        },
    );
    host.set_device_name("Den");
    assert_eq!(host.enable().await.unwrap(), HostOutcome::On);
    while host.status().state != HostState::Ready {
        assert_ne!(host.status().state, HostState::NeedsAttention);
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    host.api()
        .await
        .unwrap()
        .add_app(&App::command("Live Game", "/bin/sleep 600"))
        .await
        .unwrap();
    host_devices.set_stream_handler(Arc::new(HostHandover::new(Arc::new(host.clone()))));

    // The client's device, paired with the host's through sync.
    let client_dir = tempfile::tempdir().unwrap();
    let (client_devices, client_store, client_settings) =
        device("Steam Deck", client_dir.path()).await;
    let link = host_devices.pairing_link().await.unwrap();
    client_devices.accept_pairing_link(&link.url).await.unwrap();
    let client_id = client_devices.status().await.this_device.id.unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !host_devices
        .status()
        .await
        .devices
        .iter()
        .any(|d| d.id == client_id)
    {
        assert!(Instant::now() < deadline, "paired through sync");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let client_paths = AppPaths::under(client_dir.path());
    let client = StreamingClient::new(ManagedTools::new(
        client_store.clone(),
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &client_paths,
    ));
    let linked = LinkedPairing::new(client_devices.clone());
    let name = linked.device_name().await;
    assert_eq!(name, "Steam Deck");
    let host_id = ask_server_info("127.0.0.1", 47989).await.unwrap().id;

    // Pairing: the code goes over sync; nobody types it.
    let code = caskade_core::streaming::pairing::new_code().unwrap();
    let pairing = tokio::spawn({
        let (client, code, name) = (client.clone(), code.clone(), name.clone());
        async move {
            client
                .pair_as(
                    "127.0.0.1",
                    &code,
                    &name,
                    Duration::from_secs(60),
                    &CancellationToken::new(),
                )
                .await
        }
    });
    assert!(
        linked.hand_over(&host_id, &name, &code, &name).await,
        "the host took the code"
    );
    assert!(matches!(
        pairing.await.unwrap().unwrap(),
        Paired::Paired { .. }
    ));
    let paired = host.paired_devices().await.unwrap();
    assert!(paired.iter().any(|d| d.name == "Steam Deck"), "{paired:?}");

    // And it streams.
    client_store
        .sync_stream_apps(
            &host_id,
            &[StreamApp {
                id: "1".into(),
                name: "Live Game".into(),
            }],
        )
        .await
        .unwrap();
    let games = client_store.list_games(&Default::default()).await.unwrap();
    let game = games.items.iter().find(|g| g.title == "Live Game").unwrap();
    let entry = client_store
        .entries_for_game(&game.id)
        .await
        .unwrap()
        .remove(0);
    let launch = LaunchService::new(client_store.clone());
    launch.set_streamer(Arc::new(Streams::new(
        client.clone(),
        None,
        client_settings,
    )));
    let handle = launch.launch(&entry.id).await.unwrap();
    assert!(
        ask_server_info("127.0.0.1", 47989)
            .await
            .is_some_and(|info| info.busy)
    );
    launch
        .stop(&handle.ticket.session_id, Duration::from_secs(5))
        .await
        .unwrap();
    handle.wait_status().await.unwrap();

    // A device not paired through sync still needs the code.
    let stranger_dir = tempfile::tempdir().unwrap();
    let (stranger, _, _) = device("Stranger", stranger_dir.path()).await;
    assert!(
        !LinkedPairing::new(stranger)
            .hand_over(&host_id, "Stranger", "1234", "Stranger")
            .await
    );
    host.shutdown();
}
