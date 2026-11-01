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

fn parse(text: &str) -> Result<String, String> {
    install_input(&Url::parse(text).unwrap())
}

#[test]
fn a_link_names_one_path_or_url() {
    assert_eq!(
        parse("caskade://install?url=https%3A%2F%2Fexample.com%2Fgame.zip%3Ft%3D1").unwrap(),
        "https://example.com/game.zip?t=1"
    );
    assert_eq!(
        parse("caskade://install/?path=%2Fhome%2Fme%2FGame%20Setup.exe").unwrap(),
        "/home/me/Game Setup.exe"
    );
    assert_eq!(
        parse("caskade:install?path=/tmp/game").unwrap(),
        "/tmp/game"
    );
    // A magnet link (#159), percent-encoded inside the deep link.
    assert_eq!(
        parse("caskade://install?url=magnet%3A%3Fxt%3Durn%3Abtih%3A0123456789abcdef0123456789abcdef01234567").unwrap(),
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
    );
    for bad in [
        "caskade://install",
        "caskade://uninstall?path=/tmp/game",
        "caskade://install/extra?path=/tmp/game",
        "caskade://install?url=/tmp/game",
        "caskade://install?url=file:///etc/passwd",
        "caskade://install?path=https://example.com/game.zip",
        "caskade://install?url=https://example.com/a&path=/tmp/game",
        "caskade://install?url=https://example.com/a&url=https://example.com/b",
        "https://install?url=https://example.com/game.zip",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

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
