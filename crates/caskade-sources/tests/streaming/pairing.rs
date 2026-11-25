use super::{
    attempt_pairing, is_host_busy, pair_client, paired_host_id, setup_stream_launch, start_host,
    wait_host_ready,
};
use caskade_core::{
    AppPaths,
    accounts::AccountService,
    bus::Bus,
    db::LibraryStore,
    devices::{Devices, MemoryKeyStore, Reach},
    managed::{CancellationToken, ManagedTools},
    settings::SettingsService,
};
use caskade_streaming::{
    HostOutcome, StreamingClient,
    api::App,
    client::{SETTINGS_DIR, SETTINGS_FILE},
    hosts::{ask_server_info, paired_hosts},
    linked::{HostHandover, LinkedPairing},
    pairing::Paired,
    secrets::HostSecrets,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// The real client pairs with the real host natively (#263): Caskade's
/// own exchange with Moonlight's identity, the code accepted through the
/// host's API, no window of Moonlight's. Moonlight then lists the host's
/// apps from the record Caskade wrote, so it counts the host as paired. A
/// wrong code and a declined request pair nothing. Run under Xvfb.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client"]
async fn live_linux_pairing_needs_no_window() {
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
    let host = start_host(
        tools.clone(),
        settings,
        &paths,
        "Caskade pairing test",
        None,
        HostSecrets::memory(),
    )
    .await;
    let api = host.api().await.unwrap();
    api.add_app(&App::command("Live Game", "true"))
        .await
        .unwrap();
    let client = StreamingClient::new(tools);

    assert_eq!(
        attempt_pairing(&host, &client, "1111", Some("2222")).await,
        Paired::WrongCode
    );
    assert_eq!(
        attempt_pairing(&host, &client, "3333", None).await,
        Paired::Declined
    );
    pair_client(&host, &client).await;
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

/// #266: pairings survive switching hosting off and on with the real host
/// and client. Off removes the host program (its configuration and paired
/// devices stay); on again installs it, and the device paired before
/// streams with no new code. Run under Xvfb.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client"]
async fn live_linux_pairing_survives_hosting_off_and_on() {
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
    let host = start_host(
        tools.clone(),
        settings.clone(),
        &paths,
        "Caskade off and on",
        None,
        HostSecrets::memory(),
    )
    .await;
    host.api()
        .await
        .unwrap()
        .add_app(&App::command("Live Game", "/bin/sleep 600"))
        .await
        .unwrap();
    let client = StreamingClient::new(tools.clone());
    pair_client(&host, &client).await;

    // Off: the program goes, nothing answers.
    assert_eq!(host.disable().await.unwrap(), HostOutcome::Off);
    assert!(tools.find("sunshine").await.unwrap().is_none());
    assert!(ask_server_info("127.0.0.1", 47989).await.is_none());

    // On again: installed again, and the pairing still holds.
    assert_eq!(host.enable().await.unwrap(), HostOutcome::On);
    wait_host_ready(&host).await;
    let devices = host.paired_devices().await.unwrap();
    assert!(
        devices.iter().any(|device| device.name == "Living room"),
        "{devices:?}"
    );
    let (_, host_id) = paired_host_id(&client, "Caskade off and on").await;
    let (entry, launch) =
        setup_stream_launch(&store, &client, settings, &host_id, "Live Game").await;
    // No code: the stream starts.
    let handle = launch.launch(&entry.id).await.unwrap();
    assert!(is_host_busy().await);
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
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client"]
async fn live_linux_sync_paired_devices_stream_with_no_code() {
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
    let host = start_host(
        host_tools,
        host_settings,
        &host_paths,
        "Den",
        None,
        HostSecrets::memory(),
    )
    .await;
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
    let code = caskade_streaming::pairing::new_code().unwrap();
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
    let (entry, launch) = setup_stream_launch(
        &client_store,
        &client,
        client_settings,
        &host_id,
        "Live Game",
    )
    .await;
    let handle = launch.launch(&entry.id).await.unwrap();
    assert!(is_host_busy().await);
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
