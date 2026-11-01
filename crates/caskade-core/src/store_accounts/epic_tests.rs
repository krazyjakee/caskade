//! Epic Games against its sign-in, library and catalogue answers, served by
//! wiremock.

use super::*;
use crate::{
    bus::Bus,
    db::LibraryStore,
    error::{ErrorCode, ErrorReport},
    model::{Game, GameEntry, GameFilter, InstallLocation, InstallState, ListGames},
    store_accounts::{RefreshError, RefreshOutcome, StoreAccounts, StoreConnectionState},
};
use std::sync::Arc;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, header, header_exists, method, path, query_param},
};

const ACCOUNT_ID: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";

fn fixture(name: &str) -> serde_json::Value {
    let text = match name {
        "token" => include_str!("fixtures/epic/token.json"),
        "token_refreshed" => include_str!("fixtures/epic/token_refreshed.json"),
        "page1" => include_str!("fixtures/epic/items_page1.json"),
        "page2" => include_str!("fixtures/epic/items_page2.json"),
        "fn" => include_str!("fixtures/epic/catalog_fn.json"),
        "a1b2c3" => include_str!("fixtures/epic/catalog_a1b2c3.json"),
        "d4e5f6" => include_str!("fixtures/epic/catalog_d4e5f6.json"),
        "pr1v4t" => include_str!("fixtures/epic/catalog_pr1v4t.json"),
        "x9y8z7" => include_str!("fixtures/epic/catalog_empty.json"),
        "playtime" => include_str!("fixtures/epic/playtime.json"),
        _ => unreachable!(),
    };
    serde_json::from_str(text).unwrap()
}

struct World {
    server: MockServer,
    store: LibraryStore,
    accounts: StoreAccounts,
}

async fn world() -> World {
    let server = MockServer::start().await;
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    accounts.register_provider(Arc::new(EpicProvider::with_bases(
        base.join("account/").unwrap(),
        base.join("library/").unwrap(),
        base.join("catalog/").unwrap(),
    )));
    World {
        server,
        store,
        accounts,
    }
}

async fn signed_in(world: &World) -> StoreAccount {
    Mock::given(method("POST"))
        .and(path("/account/account/api/oauth/token"))
        .and(header_exists("authorization"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=fixture-code"))
        .and(body_string_contains("token_type=eg1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("token")))
        .mount(&world.server)
        .await;
    let flow = world.accounts.begin_connect(ID, None).await.unwrap();
    world
        .accounts
        .complete_connect(
            &flow.flow_id,
            &AuthInput::Code {
                code: r#"{"redirectUrl":"https://localhost/launcher/authorized","authorizationCode":"fixture-code","sid":null}"#.into(),
            },
            None,
        )
        .await
        .unwrap()
}

/// The library (`last` for its second page), catalogue and play time.
async fn serve_library(server: &MockServer, token: &str, last: ResponseTemplate) {
    Mock::given(path("/library/library/api/public/items"))
        .and(query_param("cursor", "cursor-2"))
        .respond_with(last)
        .mount(server)
        .await;
    Mock::given(path("/library/library/api/public/items"))
        .and(header("authorization", format!("Bearer {token}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("page1")))
        .mount(server)
        .await;
    for namespace in ["fn", "a1b2c3", "d4e5f6", "pr1v4t", "x9y8z7"] {
        Mock::given(path(format!(
            "/catalog/catalog/api/shared/namespace/{namespace}/bulk/items"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture(namespace)))
        .mount(server)
        .await;
    }
    Mock::given(path(format!(
        "/library/library/api/public/playtime/account/{ACCOUNT_ID}/all"
    )))
    .respond_with(ResponseTemplate::new(200).set_body_json(fixture("playtime")))
    .mount(server)
    .await;
}

async fn refresh(world: &World, account: &StoreAccount) -> Result<RefreshOutcome, RefreshError> {
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
}

fn epic(namespace: &str, catalog_item: &str, app_name: &str) -> ExternalId {
    ExternalId::Epic {
        namespace: namespace.into(),
        catalog_item: catalog_item.into(),
        app_name: app_name.into(),
    }
}

#[tokio::test]
async fn the_shown_code_signs_in_as_the_epic_account() {
    let challenge = EpicProvider::new().begin_auth("flow").await.unwrap();
    assert!(
        challenge
            .url
            .unwrap()
            .contains("clientId%3D34a02cf8f4414e29b15921876da36f9a")
    );
    assert_eq!(code(" abc "), Some("abc".into()));
    assert_eq!(code(r#"{"authorizationCode":null}"#), None);
    let world = world().await;
    let account = signed_in(&world).await;
    assert_eq!(
        (account.external_id.as_str(), account.display_name.as_str()),
        (ACCOUNT_ID, "FixturePlayer")
    );
    Mock::given(path("/account/account/api/oauth/token"))
        .and(body_string_contains("code=old"))
        .respond_with(ResponseTemplate::new(400).set_body_string(
            r#"{"errorCode":"errors.com.epicgames.account.oauth.authorization_code_not_found"}"#,
        ))
        .mount(&world.server)
        .await;
    let flow = world.accounts.begin_connect(ID, None).await.unwrap();
    let error = world
        .accounts
        .complete_connect(&flow.flow_id, &AuthInput::Code { code: "old".into() }, None)
        .await
        .unwrap_err();
    assert_eq!(ErrorReport::from_error(&error).code, ErrorCode::Auth);
}

#[tokio::test]
async fn owned_and_claimed_games_import_and_add_ons_and_assets_dont() {
    let world = world().await;
    let account = signed_in(&world).await;
    serve_library(
        &world.server,
        "eg1~epic-access-1",
        ResponseTemplate::new(200).set_body_json(fixture("page2")),
    )
    .await;
    let outcome = refresh(&world, &account).await.unwrap();
    assert_eq!(outcome.added, 4);
    let owned = world.store.entitlements(&account.id).await.unwrap();
    let mut titles: Vec<_> = owned.iter().map(|e| e.product.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(
        titles,
        ["A Free Weekly Game", "Control", "Control", "Fortnite"]
    );
    let fortnite = owned
        .iter()
        .find(|e| e.product.title == "Fortnite")
        .unwrap();
    assert_eq!(
        fortnite.product.ids,
        [epic("fn", "4fe75bbc5a674f4f9b356b5c90567da5", "Fortnite")]
    );
    assert_eq!(fortnite.product.playtime_minutes, Some(6000));
    assert_eq!(fortnite.product.systems, ["windows", "mac"]);
    assert_eq!(fortnite.product.last_played_ms, None);
    let claimed = owned
        .iter()
        .find(|e| e.product.title == "A Free Weekly Game")
        .unwrap();
    // Never played: Epic doesn't list it, so its play time is unknown.
    assert_eq!(claimed.product.playtime_minutes, None);
    // Two editions with one title stay two games.
    let control: Vec<_> = owned
        .iter()
        .filter(|e| e.product.title == "Control")
        .map(|e| e.game_id.clone().unwrap())
        .collect();
    assert_ne!(control[0], control[1]);
    assert_eq!(world.accounts.reconcile().await.unwrap(), 0);
}

#[tokio::test]
async fn an_incomplete_page_set_keeps_the_cached_library() {
    let world = world().await;
    let account = signed_in(&world).await;
    serve_library(
        &world.server,
        "eg1~epic-access-1",
        ResponseTemplate::new(200).set_body_json(fixture("page2")),
    )
    .await;
    refresh(&world, &account).await.unwrap();
    world.server.reset().await;
    serve_library(
        &world.server,
        "eg1~epic-access-1",
        ResponseTemplate::new(502),
    )
    .await;
    let error = refresh(&world, &account).await.unwrap_err();
    assert_eq!(ErrorReport::from_error(&error).code, ErrorCode::Offline);
    world.server.reset().await;
    serve_library(
        &world.server,
        "eg1~epic-access-1",
        ResponseTemplate::new(429).insert_header("Retry-After", "30"),
    )
    .await;
    let error = refresh(&world, &account).await.unwrap_err();
    assert!(
        matches!(
            error,
            RefreshError::Provider(ProviderError::RateLimited { retry_after: Some(wait) })
                if wait.as_secs() == 30
        ),
        "{error:?}"
    );
    let owned = world.store.entitlements(&account.id).await.unwrap();
    assert!(owned.len() == 4 && owned.iter().all(|e| e.lost_at_ms.is_none()));
}

#[tokio::test]
async fn an_ended_entitlement_is_marked_lost_by_a_complete_refresh() {
    let world = world().await;
    let account = signed_in(&world).await;
    serve_library(
        &world.server,
        "eg1~epic-access-1",
        ResponseTemplate::new(200).set_body_json(fixture("page2")),
    )
    .await;
    refresh(&world, &account).await.unwrap();
    // The claimed game's record is gone from the library.
    world.server.reset().await;
    let mut page2 = fixture("page2");
    page2["records"]
        .as_array_mut()
        .unwrap()
        .retain(|record| record["catalogItemId"] != "cat-claimed");
    serve_library(
        &world.server,
        "eg1~epic-access-1",
        ResponseTemplate::new(200).set_body_json(page2),
    )
    .await;
    assert_eq!(refresh(&world, &account).await.unwrap().lost, 1);
    let lost: Vec<_> = world
        .store
        .entitlements(&account.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.lost_at_ms.is_some())
        .map(|e| e.product.title)
        .collect();
    assert_eq!(lost, ["A Free Weekly Game"]);
}

#[tokio::test]
async fn an_expired_session_is_renewed_or_asks_to_reconnect() {
    let world = world().await;
    let account = signed_in(&world).await;
    let mut credentials = world
        .accounts
        .credentials(&account.id)
        .await
        .unwrap()
        .unwrap();
    credentials.expires_at_ms = Some(0);
    world
        .accounts
        .inner
        .credentials
        .set(&account.id, &credentials)
        .await
        .unwrap();
    Mock::given(path("/account/account/api/oauth/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=eg1%7Eepic-refresh-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("token_refreshed")))
        .up_to_n_times(1)
        .mount(&world.server)
        .await;
    serve_library(
        &world.server,
        "eg1~epic-access-2",
        ResponseTemplate::new(200).set_body_json(fixture("page2")),
    )
    .await;
    assert_eq!(refresh(&world, &account).await.unwrap().added, 4);
    let mut credentials = world
        .accounts
        .credentials(&account.id)
        .await
        .unwrap()
        .unwrap();
    credentials.expires_at_ms = Some(0);
    world
        .accounts
        .inner
        .credentials
        .set(&account.id, &credentials)
        .await
        .unwrap();
    Mock::given(path("/account/account/api/oauth/token"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&world.server)
        .await;
    refresh(&world, &account).await.unwrap_err();
    let summary = world.accounts.accounts().await.unwrap().remove(0);
    assert_eq!(
        (summary.state, summary.owned),
        (StoreConnectionState::Expired, 4)
    );
}

#[tokio::test]
async fn copies_from_the_epic_launcher_heroic_and_legendary_are_one_game() {
    let world = world().await;
    let copy = |source: &str, id: ExternalId, path: &str| {
        let game = Game::new("Control");
        let mut entry = GameEntry::new(&game.id, source.to_owned().into(), path);
        entry.ids = vec![id];
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External { path: path.into() });
        (game, entry)
    };
    for (game, entry) in [
        copy(
            "epic",
            epic("a1b2c3", "cat-control", "Calluna"),
            "C:/Epic Games/Control",
        ),
        // Another app name for the same item.
        copy(
            "heroic",
            epic("a1b2c3", "cat-control", "Calluna-Linux"),
            "/games/heroic/Control",
        ),
        // Only an app name: it names nothing for sure, so it stays apart.
        copy(
            "legendary",
            epic("", "", "Calluna"),
            "/games/legendary/Control",
        ),
    ] {
        world
            .store
            .import_group(&game, &[entry], None)
            .await
            .unwrap();
    }
    let account = signed_in(&world).await;
    serve_library(
        &world.server,
        "eg1~epic-access-1",
        ResponseTemplate::new(200).set_body_json(fixture("page2")),
    )
    .await;
    let outcome = refresh(&world, &account).await.unwrap();
    assert_eq!(outcome.merged, 1);
    let control = world
        .store
        .entitlements(&account.id)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.product.product_id == "a1b2c3:cat-control")
        .unwrap()
        .game_id
        .unwrap();
    let sources: Vec<String> = world
        .store
        .entries_for_game(&control)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.source_id.as_str().split(':').next().unwrap().to_owned())
        .collect();
    assert_eq!(sources, ["epic", "heroic", "store"]);
    // Installed copies keep their own folders.
    let installed = world
        .store
        .list_games(&ListGames {
            filter: GameFilter::Installed,
            ..ListGames::default()
        })
        .await
        .unwrap();
    assert_eq!(installed.total, 2);
}

/// A real account: `CASKADE_EPIC_CODE` (the authorizationCode the login
/// page shows; it works once, within minutes).
#[tokio::test]
#[ignore = "needs an Epic Games account and a fresh authorization code"]
async fn live() {
    let code = std::env::var("CASKADE_EPIC_CODE").expect("set CASKADE_EPIC_CODE");
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store, Bus::default());
    accounts.register_provider(Arc::new(EpicProvider::new()));
    let flow = accounts.begin_connect(ID, None).await.unwrap();
    println!(
        "login page: {}",
        flow.challenge.url.as_deref().unwrap_or_default()
    );
    let account = accounts
        .complete_connect(&flow.flow_id, &AuthInput::Code { code }, None)
        .await
        .unwrap();
    let outcome = accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    println!("{} signed in; {outcome:?}", account.display_name);
    assert!(outcome.added > 0);
}
