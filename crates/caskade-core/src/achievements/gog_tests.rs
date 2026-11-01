//! GOG achievements against a local copy of Galaxy's services (#187), in
//! the shapes GOG Galaxy reads.

use super::*;
use serde_json::json;
use std::io::Write;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

fn credentials() -> StoreCredentials {
    StoreCredentials {
        access_token: "GALAXY".into(),
        refresh_token: Some("REFRESH".into()),
        expires_at_ms: None,
        extra: [("user_id".to_owned(), "4801".to_owned())].into(),
    }
}
fn account() -> StoreAccount {
    StoreAccount {
        id: "gog-account".into(),
        provider: "gog".into(),
        external_id: "4801".into(),
        display_name: "Player".into(),
        owner_id: None,
        enabled: true,
        sync: Default::default(),
        created_at_ms: 0,
    }
}
fn zlib(json: &serde_json::Value) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(json.to_string().as_bytes()).unwrap();
    encoder.finish().unwrap()
}
fn gog(server: &MockServer) -> GogAchievements {
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    GogAchievements::with_bases(
        base.join("content/").unwrap(),
        base.join("auth/").unwrap(),
        base.join("gameplay/").unwrap(),
    )
}
/// Product `product` has a Windows build whose manifest names `client`.
async fn build(server: &MockServer, product: &str, manifest: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!(
            "/content/products/{product}/os/windows/builds"
        )))
        .and(query_param("generation", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "count": 1,
            "items": [{"build_id": "1", "link": format!("{}/manifests/{product}", server.uri())}]
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/manifests/{product}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zlib(&manifest)))
        .mount(server)
        .await;
}
async fn token(server: &MockServer, client: &str, status: u16) {
    Mock::given(method("GET"))
        .and(path("/auth/token"))
        .and(query_param("client_id", client))
        .and(query_param("refresh_token", "REFRESH"))
        .and(query_param("without_new_session", "1"))
        .respond_with(ResponseTemplate::new(status).set_body_json(json!({"access_token": "GAME"})))
        .mount(server)
        .await;
}
fn item(key: &str, visible: bool, unlocked: Option<&str>) -> serde_json::Value {
    json!({"achievement_id": "1", "achievement_key": key, "name": format!("Name {key}"),
           "description": format!("Do {key}"), "visible": visible,
           "image_url_unlocked": format!("https://img.example/{key}.png"),
           "image_url_locked": format!("https://img.example/{key}_l.png"),
           "date_unlocked": unlocked, "rarity": 12.5})
}

#[tokio::test]
async fn definitions_and_unlocks_come_page_by_page_with_the_game_s_own_token() {
    let server = MockServer::start().await;
    build(
        &server,
        "1207658924",
        json!({"clientId": "5048", "clientSecret": "s3cret"}),
    )
    .await;
    token(&server, "5048", 200).await;
    Mock::given(method("GET"))
        .and(path("/gameplay/clients/5048/users/4801/achievements"))
        .and(header("authorization", "Bearer GAME"))
        .and(query_param("page_token", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_count": 3, "items": [item("c", false, None)], "next_page_token": null
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gameplay/clients/5048/users/4801/achievements"))
        .and(header("authorization", "Bearer GAME"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_count": 3,
            "items": [item("a", true, Some("2018-03-10T21:21:51+0000")), item("b", true, None)],
            "next_page_token": "2"
        })))
        .mount(&server)
        .await;
    let fetched = gog(&server)
        .fetch(
            &account(),
            Some(&credentials()),
            "1207658924",
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(fetched.complete);
    let keys: Vec<_> = fetched.definitions.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(keys, ["a", "b", "c"]);
    assert_eq!(fetched.states["a"].unlocked_at_ms, Some(1_520_716_911_000));
    assert!(!fetched.states["b"].unlocked);
    assert!(fetched.definitions[2].hidden, "visible: false is hidden");
    assert_eq!(fetched.definitions[0].rarity_percent, Some(12.5));
    assert!(
        fetched
            .definitions
            .iter()
            .all(|d| d.progress_max.is_none() && d.points.is_none())
    );
}

#[tokio::test]
async fn games_without_galaxy_achievements_are_unsupported_never_all_locked() {
    let server = MockServer::start().await;
    // No build at all, a manifest without a Galaxy client, and a client
    // with no achievements.
    build(&server, "2", json!({"baseProductId": "2"})).await;
    build(&server, "3", json!({"clientId": "c3", "clientSecret": "s"})).await;
    token(&server, "c3", 200).await;
    Mock::given(method("GET"))
        .and(path("/gameplay/clients/c3/users/4801/achievements"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let gog = gog(&server);
    let cancel = CancellationToken::new();
    for product in ["1", "2", "3"] {
        assert!(
            matches!(
                gog.fetch(&account(), Some(&credentials()), product, &cancel)
                    .await,
                Err(FetchError::Unsupported(_))
            ),
            "{product}"
        );
    }
}

#[tokio::test]
async fn an_expired_sign_in_and_a_failed_later_page_differ() {
    let server = MockServer::start().await;
    build(
        &server,
        "10",
        json!({"clientId": "c10", "clientSecret": "s"}),
    )
    .await;
    token(&server, "c10", 401).await;
    build(
        &server,
        "11",
        json!({"clientId": "c11", "clientSecret": "s"}),
    )
    .await;
    token(&server, "c11", 200).await;
    Mock::given(method("GET"))
        .and(path("/gameplay/clients/c11/users/4801/achievements"))
        .and(query_param("page_token", "2"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gameplay/clients/c11/users/4801/achievements"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [item("a", true, Some("2018-03-10T21:21:51Z"))], "next_page_token": "2"
        })))
        .mount(&server)
        .await;
    let gog = gog(&server);
    let cancel = CancellationToken::new();
    assert!(matches!(
        gog.fetch(&account(), Some(&credentials()), "10", &cancel)
            .await,
        Err(FetchError::Provider(ProviderError::Expired(_)))
    ));
    // No refresh token: sign in again; nothing is read from launcher files.
    let mut no_refresh = credentials();
    no_refresh.refresh_token = None;
    assert!(matches!(
        gog.fetch(&account(), Some(&no_refresh), "11", &cancel)
            .await,
        Err(FetchError::Provider(ProviderError::Expired(_)))
    ));
    let partial = gog
        .fetch(&account(), Some(&credentials()), "11", &cancel)
        .await
        .unwrap();
    assert!(
        !partial.complete,
        "a failed page leaves the answer incomplete"
    );
    assert_eq!(partial.definitions.len(), 1);
}

/// Against GOG itself, with a GOG refresh token (`CASKADE_GOG_REFRESH`),
/// user id (`CASKADE_GOG_USER`) and product (`CASKADE_GOG_PRODUCT`). Read
/// only; records what GOG gives for that title.
#[tokio::test]
#[ignore = "needs a GOG sign-in: CASKADE_GOG_REFRESH, CASKADE_GOG_USER, CASKADE_GOG_PRODUCT"]
async fn live() {
    let refresh = std::env::var("CASKADE_GOG_REFRESH").expect("CASKADE_GOG_REFRESH");
    let user = std::env::var("CASKADE_GOG_USER").expect("CASKADE_GOG_USER");
    let product = std::env::var("CASKADE_GOG_PRODUCT").expect("CASKADE_GOG_PRODUCT");
    let credentials = StoreCredentials {
        access_token: String::new(),
        refresh_token: Some(refresh),
        expires_at_ms: None,
        extra: [("user_id".to_owned(), user)].into(),
    };
    let fetched = GogAchievements::new()
        .fetch(
            &account(),
            Some(&credentials),
            &product,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    println!(
        "{} achievements, {} unlocked, complete: {}",
        fetched.definitions.len(),
        fetched.states.values().filter(|s| s.unlocked).count(),
        fetched.complete
    );
}
