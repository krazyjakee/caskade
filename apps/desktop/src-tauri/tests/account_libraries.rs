//! Playnite milestone acceptance (#194): from a clean profile, connect Steam,
//! GOG and Epic accounts (their documented APIs answered by wiremock),
//! import owned games that aren't installed, install one through its
//! launcher (a fake launcher: nothing opens on the desktop), let discovery
//! find the copy, launch it and read accurate play time, all through the
//! app's commands on Tauri's mock runtime, a file library, the real
//! discovery scan of an authored machine and the real launch service.
#![cfg(unix)]

use crate::ipc::invoke;
use caskade_core::{
    AppPaths,
    bus::Bus,
    db::LibraryStore,
    launch::LaunchService,
    romm::secrets::MemorySecrets,
    settings::SettingsService,
    store_accounts::{StoreAccounts, epic::EpicProvider, gog::GogProvider, steam::SteamProvider},
};
use caskade_desktop::{
    commands::{library, store_accounts as commands},
    discovery::DiscoveryController,
};
use caskade_discovery::{
    DetectedGame, ExternalId, GameGroup, HostEnv, InstallState, LaunchTarget, ScanReport, SourceId,
    SourceReport,
};
use caskade_sources::{DiscoveryService, ImportOptions};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    Manager,
    test::{MockRuntime, mock_builder, mock_context, noop_assets},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const FIXTURES: &str = "../../../crates/caskade-core/src/store_accounts/fixtures";

fn fixture(name: &str) -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURES)
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

async fn serve(server: &MockServer, at: &str, body: Value) {
    Mock::given(path(at))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// Steam, GOG and Epic as their documented endpoints answer.
async fn stores() -> MockServer {
    let server = MockServer::start().await;
    serve(
        &server,
        "/steam/ISteamUser/GetPlayerSummaries/v2/",
        fixture("steam/player_summaries.json"),
    )
    .await;
    serve(
        &server,
        "/steam/IPlayerService/GetOwnedGames/v1/",
        fixture("steam/owned_games.json"),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/gog-auth/token"))
        .and(query_param("grant_type", "authorization_code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("gog/token.json")))
        .mount(&server)
        .await;
    serve(&server, "/gog/userData.json", fixture("gog/user_data.json")).await;
    for page in ["1", "2", "3"] {
        Mock::given(path("/gog/account/getFilteredProducts"))
            .and(query_param("page", page))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(fixture(&format!("gog/products_page{page}.json"))),
            )
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/epic-account/account/api/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("epic/token.json")))
        .mount(&server)
        .await;
    Mock::given(path("/epic-library/library/api/public/items"))
        .and(query_param("cursor", "cursor-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("epic/items_page2.json")))
        .mount(&server)
        .await;
    serve(
        &server,
        "/epic-library/library/api/public/items",
        fixture("epic/items_page1.json"),
    )
    .await;
    for (namespace, file) in [
        ("fn", "catalog_fn"),
        ("a1b2c3", "catalog_a1b2c3"),
        ("d4e5f6", "catalog_d4e5f6"),
        ("pr1v4t", "catalog_pr1v4t"),
        ("x9y8z7", "catalog_empty"),
    ] {
        serve(
            &server,
            &format!("/epic-catalog/catalog/api/shared/namespace/{namespace}/bulk/items"),
            fixture(&format!("epic/{file}.json")),
        )
        .await;
    }
    serve(
        &server,
        "/epic-library/library/api/public/playtime/account/0f1e2d3c4b5a69788796a5b4c3d2e1f0/all",
        fixture("epic/playtime.json"),
    )
    .await;
    server
}

fn url(server: &MockServer, at: &str) -> url::Url {
    url::Url::parse(&format!("{}/{at}/", server.uri())).unwrap()
}

/// Connects a store through the app's sign-in commands.
fn connect(window: &tauri::WebviewWindow<MockRuntime>, provider: &str, input: Value) -> Value {
    let flow = invoke(
        window,
        "store_connect_begin",
        json!({"provider": provider, "reconnect": null}),
    )
    .unwrap();
    invoke(
        window,
        "store_connect_complete",
        json!({"flowId": flow["flowId"], "input": input}),
    )
    .unwrap()
}

fn titles(window: &tauri::WebviewWindow<MockRuntime>, filter: &str) -> Vec<String> {
    let page = invoke(
        window,
        "list_games",
        json!({"query": {"filter": filter, "sort": "title", "offset": 0, "limit": 100}}),
    )
    .unwrap();
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|game| game["title"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn owned_games_import_install_through_their_launcher_and_play_with_accurate_time() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(&temp.path().join("profile"));
    paths.create().unwrap();
    let runtime = tauri::async_runtime::handle();
    let server = runtime.block_on(stores());
    let store = runtime
        .block_on(LibraryStore::open_default(&paths))
        .unwrap();
    let bus = Bus::default();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    caskade_sources::register_settings(&settings).unwrap();
    let accounts = StoreAccounts::with_secrets(
        store.clone(),
        bus.clone(),
        Arc::new(MemorySecrets::default()),
    );
    accounts.set_settings(settings.clone());
    accounts.register_provider(Arc::new(SteamProvider::with_base(url(&server, "steam"))));
    accounts.register_provider(Arc::new(GogProvider::with_bases(
        url(&server, "gog-auth"),
        url(&server, "gog"),
    )));
    accounts.register_provider(Arc::new(EpicProvider::with_bases(
        url(&server, "epic-account"),
        url(&server, "epic-library"),
        url(&server, "epic-catalog"),
    )));
    // The fake launcher: it records what it's asked to open.
    let opened = Arc::new(Mutex::new(Vec::<String>::new()));
    accounts.set_opener(Arc::new({
        let opened = opened.clone();
        move |uri: &str| {
            opened.lock().unwrap().push(uri.to_owned());
            Ok(())
        }
    }));
    // Owned games reconcile after every library scan.
    let _watch = runtime.block_on(async { Some(accounts.watch()) });
    // Discovery over an authored machine with Steam and Heroic installed.
    let env = HostEnv::fixture(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/caskade-discovery/tests/fixtures/machines/linux-integration"),
    )
    .unwrap();
    let discovery = DiscoveryService::new(store.clone(), bus.clone(), env);
    let controller = DiscoveryController::new(discovery.clone(), settings.clone());
    let specta =
        tauri_specta::Builder::<MockRuntime>::new().commands(tauri_specta::collect_commands![
            commands::store_connect_begin,
            commands::store_connect_complete::<MockRuntime>,
            commands::store_account_refresh,
            commands::store_accounts_list,
            commands::game_owned_releases::<MockRuntime>,
            commands::owned_install::<MockRuntime>,
            commands::game_playtime,
            library::list_games,
        ]);
    let app = mock_builder()
        .manage(
            tauri::async_runtime::block_on(caskade_core::accounts::AccountService::open(
                store.clone(),
                bus.clone(),
            ))
            .unwrap(),
        )
        .manage(accounts.clone())
        .manage(store.clone())
        .manage(controller)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    // Connect the three stores, as Settings → Store accounts does.
    let steam = connect(
        &window,
        "steam",
        json!({"type": "fields", "fields": {"profile": "76561197960435530", "key": "0123456789ABCDEF"}}),
    );
    let gog = connect(
        &window,
        "gog",
        json!({"type": "code", "code": "https://embed.gog.com/on_login_success?origin=client&code=x"}),
    );
    let epic = connect(
        &window,
        "epic",
        json!({"type": "code", "code": "{\"authorizationCode\":\"fixture-code\"}"}),
    );
    for account in [&steam, &gog, &epic] {
        invoke(
            &window,
            "store_account_refresh",
            json!({"id": account["id"]}),
        )
        .unwrap();
    }
    let listed = invoke(&window, "store_accounts_list", json!({})).unwrap();
    let owned: Vec<(String, u64, String)> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|summary| {
            (
                summary["account"]["provider"].as_str().unwrap().to_owned(),
                summary["owned"].as_u64().unwrap(),
                summary["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        owned,
        [
            ("steam".into(), 5, "connected".into()),
            ("gog".into(), 4, "connected".into()),
            ("epic".into(), 4, "connected".into())
        ]
    );
    // Owned, none installed: all ready to install.
    assert!(titles(&window, "installed").is_empty());
    let ready = titles(&window, "owned");
    assert!(ready.contains(&"Portal".to_owned()) && ready.contains(&"Stardew Valley".to_owned()));

    // A library scan finds what Steam and Heroic have installed here: the
    // owned games and those copies become one game each.
    runtime.block_on(controller_scan(&app));
    wait(|| titles(&window, "installed").contains(&"Portal 2".to_owned()));
    let installed = titles(&window, "installed");
    assert!(
        installed.contains(&"Unreal Tournament 2004 Editor's Choice Edition".to_owned()),
        "{installed:?}"
    );
    let all = titles(&window, "all");
    assert_eq!(
        all.iter().filter(|t| t.as_str() == "Portal 2").count(),
        1,
        "{all:?}"
    );

    // Install Portal, owned on Steam, through the Steam client.
    let portal = game_id(&window, "Portal");
    let releases = invoke(&window, "game_owned_releases", json!({"gameId": portal})).unwrap();
    assert_eq!(releases[0]["state"]["type"], "available", "{releases}");
    let handed = invoke(
        &window,
        "owned_install",
        json!({"entryId": releases[0]["entryId"]}),
    )
    .unwrap();
    assert_eq!(handed["state"]["type"], "installing");
    assert_eq!(*opened.lock().unwrap(), ["steam://install/400"]);
    // Nothing claimed until discovery finds the copy the launcher installed.
    assert!(!titles(&window, "installed").contains(&"Portal".to_owned()));
    let game_dir = temp.path().join("steamapps/common/Portal");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::write(game_dir.join("portal"), "#!/bin/sh\nsleep 0.3\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            game_dir.join("portal"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let mut copy = DetectedGame::new(SourceId::from("steam".to_owned()), "400", "Portal");
    copy.ids = vec![ExternalId::Steam { app_id: 400 }];
    copy.state = InstallState::Installed;
    copy.install_dir = Some(game_dir.clone());
    copy.launch.push(LaunchTarget::Exec {
        program: game_dir.join("portal"),
        args: vec![],
        cwd: Some(game_dir.clone()),
        env: BTreeMap::new(),
    });
    runtime
        .block_on(discovery.import_report(
            &ScanReport {
                groups: vec![GameGroup {
                    primary: 0,
                    entries: vec![copy],
                }],
                sources: vec![SourceReport {
                    source: SourceId::from("steam".to_owned()),
                    duration_ms: 0,
                    count: 1,
                    warnings: vec![],
                    error: None,
                }],
                ..ScanReport::default()
            },
            &ImportOptions::default(),
            &caskade_discovery::Cancel::new(),
        ))
        .unwrap();
    wait(|| {
        invoke(&window, "game_owned_releases", json!({"gameId": portal})).unwrap()[0]["state"]["type"]
            == "installed"
    });
    assert_eq!(
        game_id(&window, "Portal"),
        portal,
        "the copy joined the owned game"
    );

    // Play it: the session is Caskade's own; Steam reported 0 minutes.
    let entry = runtime
        .block_on(store.entry_by_source("steam", "400"))
        .unwrap()
        .unwrap();
    let session = runtime
        .block_on(async {
            LaunchService::new(store.clone())
                .launch(&entry.id)
                .await
                .unwrap()
                .wait()
                .await
        })
        .unwrap();
    assert!(session.duration_ms >= 200, "{}", session.duration_ms);
    let playtime = invoke(&window, "game_playtime", json!({"gameId": portal})).unwrap();
    assert_eq!(playtime["trackedMs"], json!(session.duration_ms));
    assert_eq!(playtime["totalMs"], json!(session.duration_ms));
    // Steam refreshed afterwards: still counted once.
    invoke(&window, "store_account_refresh", json!({"id": steam["id"]})).unwrap();
    let again = invoke(&window, "game_playtime", json!({"gameId": portal})).unwrap();
    assert_eq!(again["totalMs"], json!(session.duration_ms));
    // Portal 2's 1534 minutes come from Steam once, beside the scan's.
    let portal2 = game_id(&window, "Portal 2");
    let reported = invoke(&window, "game_playtime", json!({"gameId": portal2})).unwrap();
    assert!(
        reported["totalMs"].as_u64().unwrap() >= 1534 * 60_000,
        "{reported}"
    );
    drop(app);
}

async fn controller_scan(app: &tauri::App<MockRuntime>) {
    app.state::<DiscoveryController>().scan().await.unwrap();
}

fn game_id(window: &tauri::WebviewWindow<MockRuntime>, title: &str) -> String {
    let page = invoke(
        window,
        "list_games",
        json!({"query": {"filter": "all", "query": title, "sort": "title", "offset": 0, "limit": 50}}),
    )
    .unwrap();
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["title"] == title)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Waits for the background reconcile.
fn wait(done: impl Fn() -> bool) {
    for _ in 0..200 {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out");
}
