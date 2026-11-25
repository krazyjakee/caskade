use super::{dirs_home, start_host};
use caskade_core::{
    AppPaths,
    bus::Bus,
    db::LibraryStore,
    managed::{CancellationToken, ManagedTools},
    settings::SettingsService,
};
use caskade_streaming::{
    HostOutcome,
    api::HostApi,
    client::{SETTINGS_DIR, SETTINGS_FILE, StreamingClient},
    hosts::{HostReach, StreamHosts},
    secrets::HostSecrets,
};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host on ports 47984-48010"]
async fn live_linux_host_answers_pinned_and_stops_with_caskade() {
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
    let home_config = dirs_home().join(".config/sunshine");
    let had_home_config = home_config.exists();

    let host = start_host(
        tools,
        settings,
        &paths,
        "Caskade live test",
        None,
        secrets.clone(),
    )
    .await;
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
        caskade_streaming::api::LOCAL.parse().unwrap(),
        secrets.get().await.unwrap().unwrap(),
        &other,
    )
    .unwrap();
    assert!(wrong.version().await.is_err());
    // It wrote nothing under the user's own Sunshine folder.
    assert_eq!(home_config.exists(), had_home_config);

    // Another device finds it on the network (#262): mDNS through the
    // system's responder, named by its /serverinfo as Caskade set it.
    let hosts = StreamHosts::new(
        LibraryStore::in_memory().await.unwrap(),
        Bus::default(),
        {
            let store = LibraryStore::in_memory().await.unwrap();
            let settings = SettingsService::new(store);
            settings.register_core(&paths).unwrap();
            settings
        },
        StreamingClient::new(ManagedTools::new(
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
        found
            .iter()
            .any(|host| host.name == "Caskade live test" && host.reach == HostReach::Ready),
        "{found:?}"
    );

    host.shutdown();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(api.version().await.is_err(), "stopped with Caskade");
    assert_eq!(host.disable().await.unwrap(), HostOutcome::Off);
}

/// The real client (ignored): installed, given its own state, and run
/// once; the user's own Moonlight settings stay as they were, and Caskade's
/// state holds what Moonlight wrote (its certificate) beside the quiet
/// settings. Run under Xvfb like the host test.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming client"]
async fn live_linux_client_keeps_its_own_state() {
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
    tokio::time::sleep(Duration::from_secs(8)).await;
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
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
        "uidisplaymode=0",
    ] {
        assert!(ours.contains(quiet), "{quiet}");
    }
}
