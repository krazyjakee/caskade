//! Sign-ins: failures, saved passwords that never follow the server
//! elsewhere, redirects, and servers waiting for a sign-in here.

use super::*;

#[tokio::test]
async fn a_failed_sign_in_is_reported() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    let fixture = Fixture::with_server(server, "wrong").await;
    let error = fixture
        .romm
        .test_connection(server_config(&fixture.server.uri()), None)
        .await
        .unwrap_err();
    assert!(matches!(error, RommError::Auth(_)), "{error}");
    assert!(error.to_string().contains("Invalid username or password"));
    assert!(matches!(
        fixture.romm.sync("home").await,
        Err(RommError::Auth(_))
    ));
    // A refused sign-in needs a new one, not a better connection (#73).
    let overview = fixture.romm.overview().await.unwrap();
    assert_eq!(overview.servers[0].state, ConnectionState::SignInNeeded);
    assert!(overview.servers[0].has_secret);
    assert_eq!(
        fixture
            .store
            .list_games(&ListGames::default())
            .await
            .unwrap()
            .total,
        0
    );
}

/// No request to `server` carried the saved password or a bearer token.
async fn assert_no_credentials(server: &MockServer) {
    for request in server.received_requests().await.unwrap_or_default() {
        let body = String::from_utf8_lossy(&request.body);
        assert!(
            !body.contains("password=secret"),
            "{} got the password",
            request.url
        );
        assert!(
            !request.headers.contains_key("authorization"),
            "{} got credentials",
            request.url
        );
    }
}

/// A stored password goes only to the address it was saved for (#157): a
/// test of an unsaved address, or settings that point the server elsewhere,
/// send it nowhere else.
#[tokio::test]
async fn a_saved_password_never_follows_the_server_to_another_address() {
    let fixture = Fixture::new().await;
    let other = MockServer::builder().start().await;
    mount(&other, default_snes()).await;

    // Testing another address without typing the password again.
    let error = fixture
        .romm
        .test_connection(server_config(&other.uri()), None)
        .await
        .unwrap_err();
    assert!(!matches!(error, RommError::Unreachable(_)), "{error}");
    assert_no_credentials(&other).await;

    // Settings changed underneath `save_server`.
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.servers[0].url = other.uri();
    fixture.romm.save_settings(settings).await.unwrap();
    assert!(fixture.romm.sync("home").await.is_err());
    assert_no_credentials(&other).await;
    let status = fixture.romm.overview().await.unwrap().servers;
    assert!(!status[0].has_secret, "the password isn't for this address");

    // Back at the address it was saved for, it works again.
    let mut settings = fixture.romm.settings().await.unwrap();
    settings.servers[0].url = fixture.server.uri();
    fixture.romm.save_settings(settings).await.unwrap();
    fixture.romm.sync("home").await.unwrap();
}

/// A redirect to another host never takes the sign-in form with it (#157).
#[tokio::test]
async fn sign_in_does_not_follow_redirects_to_another_host() {
    let other = MockServer::builder().start().await;
    mount(&other, default_snes()).await;
    let server = MockServer::builder().start().await;
    Mock::given(method("POST"))
        .and(path("/api/token"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/api/token", other.uri()).as_str()),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    mount(&server, default_snes()).await;
    // Another port on the same host is another origin.
    let fixture = Fixture::with_server(server, "secret").await;
    assert!(fixture.romm.sync("home").await.is_err());
    assert_no_credentials(&other).await;
}

/// A server that signs in with an API token and syncs in the background.
fn token_server(url: &str) -> RommServerConfig {
    RommServerConfig {
        auth: RommAuth::Token,
        username: String::new(),
        sync_on_startup: true,
        sync_interval: "1h".into(),
        ..RommServerConfig::new("home", "Home RomM", url)
    }
}

/// Listings without the right bearer token get RomM's 401.
async fn refuse_unsigned(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path_regex("^/api/(platforms|collections|roms)"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({ "detail": "Not authenticated" })),
        )
        .with_priority(10)
        .mount(server)
        .await;
}

/// The requests `server` got for `route`.
async fn requests_to(server: &MockServer, route: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path() == route)
        .count()
}

/// Waits up to five seconds for the server's state to become `state`.
async fn wait_for_state(
    fixture: &Fixture,
    state: ConnectionState,
) -> caskade_core::romm::ServerStatus {
    for _ in 0..100 {
        let status = server_status(fixture).await;
        if status.state == state && !status.syncing {
            return status;
        }
        tokio::time::sleep(WATCH_EVERY).await;
    }
    panic!(
        "the server never became {state:?}: {:?}",
        server_status(fixture).await
    );
}

/// A server synced from another device arrives without its API token,
/// which stays in each device's keychain (#73): it needs signing in here,
/// and the background syncs wait for a token without asking the server.
#[tokio::test]
async fn a_server_without_a_sign_in_here_waits_for_one() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    refuse_unsigned(&server).await;
    let fixture = Fixture::with_server(server, "secret").await;
    // A new sign-in method forgets the saved password.
    fixture
        .romm
        .save_server(token_server(&fixture.server.uri()), None)
        .await
        .unwrap();
    let status = server_status(&fixture).await;
    assert_eq!(status.state, ConnectionState::SignInNeeded);
    assert!(!status.has_secret);
    assert_eq!(status.error, None);

    let asked = fixture.server.received_requests().await.unwrap().len();
    let watcher = fixture.romm.watch();
    tokio::time::sleep(WATCH_EVERY * 8).await;
    assert_eq!(
        fixture.server.received_requests().await.unwrap().len(),
        asked,
        "the background syncs asked a server they can't sign in to"
    );
    assert_eq!(
        server_status(&fixture).await.state,
        ConnectionState::SignInNeeded
    );

    // Sync now still asks, and says why it can't.
    let error = fixture.romm.sync("home").await.unwrap_err();
    assert!(matches!(error, RommError::Auth(_)), "{error}");
    let status = server_status(&fixture).await;
    assert_eq!(status.state, ConnectionState::SignInNeeded);
    assert_eq!(status.error.as_deref(), Some(error.to_string().as_str()));

    // A token entered here: the background syncs pick the server up.
    fixture
        .romm
        .save_server(token_server(&fixture.server.uri()), Some("tok".into()))
        .await
        .unwrap();
    let status = wait_for_state(&fixture, ConnectionState::Connected).await;
    assert!(status.has_secret);
    assert!(status.games > 0);
    watcher.abort();
}

/// A token the server refuses (wrong, expired or revoked) is tried once
/// in the background, then waits for a new one instead of being sent
/// again at every check (#73).
#[tokio::test]
async fn a_refused_sign_in_is_not_retried_in_the_background() {
    let server = MockServer::builder().start().await;
    mount(&server, default_snes()).await;
    refuse_unsigned(&server).await;
    let fixture = Fixture::with_server(server, "secret").await;
    fixture
        .romm
        .save_server(token_server(&fixture.server.uri()), Some("expired".into()))
        .await
        .unwrap();
    assert_eq!(
        server_status(&fixture).await.state,
        ConnectionState::Untested
    );

    let watcher = fixture.romm.watch();
    let status = wait_for_state(&fixture, ConnectionState::SignInNeeded).await;
    assert!(status.has_secret);
    assert!(
        status
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Not authenticated")),
        "{status:?}"
    );
    tokio::time::sleep(WATCH_EVERY * 8).await;
    assert_eq!(requests_to(&fixture.server, "/api/platforms").await, 1);

    fixture
        .romm
        .save_server(token_server(&fixture.server.uri()), Some("tok".into()))
        .await
        .unwrap();
    wait_for_state(&fixture, ConnectionState::Connected).await;
    assert_eq!(requests_to(&fixture.server, "/api/platforms").await, 2);
    watcher.abort();
}
