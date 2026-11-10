use super::*;
use crate::commands::install_requests::{InputKind, InstallRequested};
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    install::{InstallOptions, InstallService},
    model::StorageRoot,
};
use std::{
    io::ErrorKind,
    net::TcpListener,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    Listener,
    test::{mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;

/// #57 acceptance: a URL deep link never starts a download before the user
/// confirms it; confirming does.
#[test]
fn a_url_deep_link_downloads_nothing_until_confirmed() {
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
    let store = tauri::async_runtime::block_on(async {
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
        store
    });
    // The library's lone admin, signed in: it may install.
    let accounts = tauri::async_runtime::block_on(caskade_core::accounts::AccountService::open(
        store.clone(),
        Bus::default(),
    ))
    .unwrap();
    let service = InstallService::new(
        store,
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .events(tauri_specta::collect_events![InstallRequested]);
    let app = mock_builder()
        .manage(InstallRequests::default())
        .manage(service.clone())
        .manage(accounts)
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let announced = Arc::new(Mutex::new(Vec::new()));
    let seen = announced.clone();
    app.listen(InstallRequested::NAME, move |event| {
        seen.lock().unwrap().push(event.payload().to_owned());
    });

    let link = Url::parse_with_params("caskade://install", [("url", &download)]).unwrap();
    let queued = tauri::async_runtime::block_on(open(app.handle(), [link]));
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].kind, InputKind::Url);
    assert_eq!(queued[0].value, download);
    assert_eq!(app.state::<InstallRequests>().pending(), queued);

    // The UI hears about it; the install queue and the network do not.
    let deadline = Instant::now() + Duration::from_secs(5);
    while announced.lock().unwrap().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(announced.lock().unwrap().len(), 1);
    std::thread::sleep(Duration::from_millis(500));
    assert!(service.jobs().is_empty());
    assert_eq!(
        server.accept().map(|_| ()).unwrap_err().kind(),
        ErrorKind::WouldBlock,
        "nothing may connect before confirmation"
    );

    // Confirming queues the install, which starts the download.
    let requests = app.state::<InstallRequests>();
    tauri::async_runtime::block_on(requests.confirm(&queued[0].request_id, None, &service))
        .unwrap();
    assert_eq!(service.jobs().len(), 1);
    assert!(requests.pending().is_empty());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match server.accept() {
            Ok(_) => break,
            Err(error) if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("the confirmed download never connected: {error}"),
        }
    }
}
