//! `caskade://install` deep links (#57) as the shells queue them.
use caskade_app::{
    commands::install_requests::{InputKind, InstallRequests},
    deep_link::{install_input, queue},
    events::Event,
};
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    db::LibraryStore,
    install::{InstallOptions, InstallService},
    model::StorageRoot,
};
use std::{
    io::ErrorKind,
    net::TcpListener,
    time::{Duration, Instant},
};
use url::Url;

/// #57 acceptance: a URL deep link never starts a download before the user
/// confirms it; confirming does.
#[tokio::test(flavor = "multi_thread")]
async fn a_url_deep_link_downloads_nothing_until_confirmed() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let download = format!("http://{}/game.zip", server.local_addr().unwrap());

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Games");
    std::fs::create_dir_all(root.join(".caskade")).unwrap();
    std::fs::write(
        root.join(".caskade/root.json"),
        br#"{"schema_version":1,"root_id":"link-root"}"#,
    )
    .unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    store
        .upsert_root(&StorageRoot {
            id: "link-root".into(),
            path: root.clone(),
            is_default: true,
            label: None,
        })
        .await
        .unwrap();
    // The library's lone admin, signed in: it may install.
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let service = InstallService::new(
        store,
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let requests = InstallRequests::default();
    let sink = crate::Sink::default();

    let link = Url::parse_with_params("caskade://install", [("url", &download)]).unwrap();
    let (queued, raise) = queue(
        &sink.events(),
        Some(&requests),
        Some(&accounts),
        vec![install_input(&link)],
    )
    .await;
    assert!(raise, "the window comes up to confirm it");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].kind, InputKind::Url);
    assert_eq!(queued[0].value, download);
    assert_eq!(requests.pending(), queued);

    // The UI hears about it; the install queue and the network do not.
    let announced = sink.wait_for(Duration::from_secs(5), |event| match event {
        Event::InstallRequested(_) => Some(()),
        _ => None,
    });
    assert!(announced.is_some());
    let announcements = sink
        .taken()
        .iter()
        .filter(|event| matches!(event, Event::InstallRequested(_)))
        .count();
    assert_eq!(announcements, 1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(service.jobs().is_empty());
    assert_eq!(
        server.accept().map(|_| ()).unwrap_err().kind(),
        ErrorKind::WouldBlock,
        "nothing may connect before confirmation"
    );

    // Confirming queues the install, which starts the download.
    requests
        .confirm(&queued[0].request_id, None, &service)
        .await
        .unwrap();
    assert_eq!(service.jobs().len(), 1);
    assert!(requests.pending().is_empty());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match server.accept() {
            Ok(_) => break,
            Err(error) if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => panic!("the confirmed download never connected: {error}"),
        }
    }
}
