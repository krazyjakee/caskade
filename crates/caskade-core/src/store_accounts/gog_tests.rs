//! GOG against its documented sign-in and library answers, served by
//! wiremock.

use super::*;
use crate::{
    bus::Bus,
    db::LibraryStore,
    error::{ErrorCode, ErrorReport},
    model::{Game, GameEntry, GameFilter, InstallLocation, InstallState, ListGames, StorageRoot},
    store_accounts::{RefreshError, StoreAccounts, StoreConnectionState},
};
use std::sync::Arc;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

const USER: &str = "48628349957132247";

fn fixture(name: &str) -> serde_json::Value {
    let text = match name {
        "token" => include_str!("fixtures/gog/token.json"),
        "token_refreshed" => include_str!("fixtures/gog/token_refreshed.json"),
        "user_data" => include_str!("fixtures/gog/user_data.json"),
        "page1" => include_str!("fixtures/gog/products_page1.json"),
        "page2" => include_str!("fixtures/gog/products_page2.json"),
        "page3" => include_str!("fixtures/gog/products_page3.json"),
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
    accounts.register_provider(Arc::new(GogProvider::with_bases(
        base.join("auth/").unwrap(),
        base.join("embed/").unwrap(),
    )));
    World {
        server,
        store,
        accounts,
    }
}

async fn serve_sign_in(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/auth/token"))
        .and(query_param("grant_type", "authorization_code"))
        .and(query_param("code", "fixture-code"))
        .and(query_param("client_id", CLIENT_ID))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("token")))
        .mount(server)
        .await;
    Mock::given(path("/embed/userData.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("user_data")))
        .mount(server)
        .await;
}

async fn serve_pages(server: &MockServer, token: &str, last: ResponseTemplate) {
    for (page, body) in [("1", fixture("page1")), ("2", fixture("page2"))] {
        Mock::given(path("/embed/account/getFilteredProducts"))
            .and(query_param("mediaType", "1"))
            .and(query_param("page", page))
            .and(header("authorization", format!("Bearer {token}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }
    Mock::given(path("/embed/account/getFilteredProducts"))
        .and(query_param("page", "3"))
        .respond_with(last)
        .mount(server)
        .await;
}

async fn signed_in(world: &World) -> StoreAccount {
    serve_sign_in(&world.server).await;
    let flow = world.accounts.begin_connect(ID, None).await.unwrap();
    world
        .accounts
        .complete_connect(
            &flow.flow_id,
            &AuthInput::Code {
                code: "https://embed.gog.com/on_login_success?origin=client&code=fixture-code"
                    .into(),
            },
            None,
        )
        .await
        .unwrap()
}

async fn refresh(
    world: &World,
    account: &StoreAccount,
) -> Result<crate::store_accounts::RefreshOutcome, RefreshError> {
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
}

#[tokio::test]
async fn signing_in_takes_the_pasted_address_and_names_the_account() {
    let challenge = GogProvider::new().begin_auth("flow").await.unwrap();
    let url = Url::parse(challenge.url.as_deref().unwrap()).unwrap();
    assert_eq!(url.host_str(), Some("auth.gog.com"));
    assert!(
        url.query_pairs()
            .any(|(k, v)| k == "client_id" && v == CLIENT_ID)
    );
    assert!(challenge.instructions.unwrap().contains("code="));
    let world = world().await;
    let account = signed_in(&world).await;
    assert_eq!(
        (account.external_id.as_str(), account.display_name.as_str()),
        (USER, "fixtureplayer")
    );
    // A bare code works too, and a wrong one is said plainly.
    assert_eq!(code(" fixture-code "), Some("fixture-code".into()));
    Mock::given(path("/auth/token"))
        .and(query_param("code", "stale"))
        .respond_with(ResponseTemplate::new(400).set_body_string("{\"error\":\"invalid_grant\"}"))
        .mount(&world.server)
        .await;
    let flow = world.accounts.begin_connect(ID, None).await.unwrap();
    let error = world
        .accounts
        .complete_connect(
            &flow.flow_id,
            &AuthInput::Code {
                code: "stale".into(),
            },
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(ErrorReport::from_error(&error).code, ErrorCode::Auth);
}

#[tokio::test]
async fn every_page_of_games_imports_with_gog_ids_and_no_install_location() {
    let world = world().await;
    let account = signed_in(&world).await;
    serve_pages(
        &world.server,
        "gog-access-1",
        ResponseTemplate::new(200).set_body_json(fixture("page3")),
    )
    .await;
    let outcome = refresh(&world, &account).await.unwrap();
    // Four games: the movie is left out, a repeated product counted once.
    assert_eq!((outcome.added, outcome.partial), (4, false));
    let owned = world.store.entitlements(&account.id).await.unwrap();
    let product = |id: &str| {
        owned
            .iter()
            .find(|e| e.product.product_id == id)
            .unwrap()
            .product
            .clone()
    };
    assert_eq!(
        product("1207658924").ids,
        [ExternalId::Gog {
            product_id: 1_207_658_924
        }]
    );
    assert_eq!(product("1207658924").systems, ["windows"]);
    assert_eq!(product("1207666393").systems, ["windows", "mac", "linux"]);
    assert_eq!(product("1435848814").title, "GOG 1435848814");
    assert!(owned.iter().all(|e| e.product.playtime_minutes.is_none()));
    assert!(!owned.iter().any(|e| e.product.product_id == "1207666394"));
    for entitlement in &owned {
        let entry = world
            .store
            .get_entry(entitlement.entry_id.as_deref().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert!(entry.location.is_none() && entry.launch.is_none());
    }
}

#[tokio::test]
async fn an_interrupted_last_page_keeps_the_last_library() {
    let world = world().await;
    let account = signed_in(&world).await;
    serve_pages(
        &world.server,
        "gog-access-1",
        ResponseTemplate::new(200).set_body_json(fixture("page3")),
    )
    .await;
    refresh(&world, &account).await.unwrap();
    // Page 3 now fails, then is cut short.
    world.server.reset().await;
    serve_pages(&world.server, "gog-access-1", ResponseTemplate::new(503)).await;
    let error = refresh(&world, &account).await.unwrap_err();
    assert_eq!(ErrorReport::from_error(&error).code, ErrorCode::Offline);
    world.server.reset().await;
    serve_pages(
        &world.server,
        "gog-access-1",
        ResponseTemplate::new(200).set_body_string("{\"page\":3,\"totalPages\":3,\"produ"),
    )
    .await;
    refresh(&world, &account).await.unwrap_err();
    let owned = world.store.entitlements(&account.id).await.unwrap();
    assert!(owned.len() == 4 && owned.iter().all(|e| e.lost_at_ms.is_none()));
    // Cancelled before the first page: nothing asked, nothing changed.
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(world.accounts.refresh(&account.id, &cancel).await.is_err());
    assert_eq!(
        world.store.entitlements(&account.id).await.unwrap().len(),
        4
    );
}

#[tokio::test]
async fn an_expired_token_is_renewed_and_a_refused_renewal_asks_to_reconnect() {
    let world = world().await;
    let account = signed_in(&world).await;
    // The hour is up.
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
    Mock::given(path("/auth/token"))
        .and(query_param("grant_type", "refresh_token"))
        .and(query_param("refresh_token", "gog-refresh-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("token_refreshed")))
        .up_to_n_times(1)
        .mount(&world.server)
        .await;
    serve_pages(
        &world.server,
        "gog-access-2",
        ResponseTemplate::new(200).set_body_json(fixture("page3")),
    )
    .await;
    assert_eq!(refresh(&world, &account).await.unwrap().added, 4);
    let renewed = world
        .accounts
        .credentials(&account.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(renewed.refresh_token.as_deref(), Some("gog-refresh-2"));
    assert_eq!(renewed.extra.get("user_id").map(String::as_str), Some(USER));

    // Expired again, and GOG refuses the refresh token.
    let mut credentials = renewed;
    credentials.expires_at_ms = Some(0);
    world
        .accounts
        .inner
        .credentials
        .set(&account.id, &credentials)
        .await
        .unwrap();
    Mock::given(path("/auth/token"))
        .and(query_param("grant_type", "refresh_token"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&world.server)
        .await;
    refresh(&world, &account).await.unwrap_err();
    let summary = world.accounts.accounts().await.unwrap().remove(0);
    assert_eq!(summary.state, StoreConnectionState::Expired);
    assert_eq!(summary.owned, 4);
}

#[tokio::test]
async fn copies_from_galaxy_heroic_and_an_offline_installer_are_one_game() {
    let world = world().await;
    let root = StorageRoot {
        id: "01a1aaaa-0000-7000-8000-000000000001".into(),
        path: "/games".into(),
        is_default: true,
        label: None,
    };
    world.store.upsert_root(&root).await.unwrap();
    let copy = |source: &str, location: InstallLocation| {
        let game = Game::new("Stardew Valley");
        let mut entry = GameEntry::new(&game.id, source.to_owned().into(), "stardew");
        entry.ids = vec![ExternalId::Gog {
            product_id: 1_207_666_393,
        }];
        entry.state = InstallState::Installed;
        entry.location = Some(location);
        (game, entry)
    };
    for (game, entry) in [
        copy(
            "gog",
            InstallLocation::External {
                path: "C:/GOG Games/Stardew Valley".into(),
            },
        ),
        copy(
            "heroic",
            InstallLocation::External {
                path: "/home/p/Games/Heroic/Stardew Valley".into(),
            },
        ),
        copy(
            "caskade-install",
            InstallLocation::Managed {
                root_id: root.id.clone(),
                rel_dir: "Stardew Valley".into(),
            },
        ),
    ] {
        world
            .store
            .import_group(&game, &[entry], None)
            .await
            .unwrap();
    }
    let account = signed_in(&world).await;
    serve_pages(
        &world.server,
        "gog-access-1",
        ResponseTemplate::new(200).set_body_json(fixture("page3")),
    )
    .await;
    let outcome = refresh(&world, &account).await.unwrap();
    assert_eq!(outcome.merged, 2);
    let stardew = world
        .store
        .entitlements(&account.id)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.product.product_id == "1207666393")
        .unwrap()
        .game_id
        .unwrap();
    let entries = world.store.entries_for_game(&stardew).await.unwrap();
    assert_eq!(entries.len(), 4);
    let managed = entries
        .iter()
        .find(|e| e.source_id.as_str() == "caskade-install")
        .unwrap();
    assert_eq!(
        managed.location,
        Some(InstallLocation::Managed {
            root_id: root.id.clone(),
            rel_dir: "Stardew Valley".into()
        })
    );
    let installed = world
        .store
        .list_games(&ListGames {
            filter: GameFilter::Installed,
            ..ListGames::default()
        })
        .await
        .unwrap();
    assert_eq!(installed.total, 1);
}

/// A real account: `CASKADE_GOG_CODE` (the code from GOG's login page,
/// see `begin_auth`'s URL; it works once, within minutes).
#[tokio::test]
#[ignore = "needs a GOG account and a fresh login code"]
async fn live() {
    let code = std::env::var("CASKADE_GOG_CODE").expect("set CASKADE_GOG_CODE");
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store, Bus::default());
    accounts.register_provider(Arc::new(GogProvider::new()));
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
