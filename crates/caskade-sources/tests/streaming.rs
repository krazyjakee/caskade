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
