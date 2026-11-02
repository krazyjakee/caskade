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

    host.shutdown();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(api.version().await.is_err(), "stopped with Caskade");
    assert_eq!(host.disable().await.unwrap(), HostOutcome::Off);
}

#[cfg(all(feature = "live-tests", target_os = "linux"))]
fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_default()
}
