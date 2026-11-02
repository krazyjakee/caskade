use caskade_core::ludusavi::{Game, Ludusavi, Refresh, StoreId, apply_secondary};
use flate2::{Compression, write::GzEncoder};
use serde_json::Value;
use sqlx::{Connection, SqliteConnection};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

const MANIFEST: &str = include_str!("fixtures/ludusavi/manifest.yaml");

fn gzip(body: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(body.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

async fn serve(server: &MockServer, body: &str, etag: &str) {
    Mock::given(method("GET"))
        .and(path("/manifest.yaml"))
        .and(header("accept-encoding", "gzip"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", etag)
                .insert_header("content-encoding", "gzip")
                .set_body_bytes(gzip(body)),
        )
        .expect(1)
        .mount(server)
        .await;
}

fn service(dir: &Path, server: &MockServer) -> Ludusavi {
    Ludusavi::with_url(dir, format!("{}/manifest.yaml", server.uri()))
}

async fn state(dir: &Path) -> Value {
    let mut db =
        SqliteConnection::connect(&format!("sqlite:{}", dir.join("catalog.sqlite").display()))
            .await
            .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM current WHERE id=1")
        .fetch_one(&mut db)
        .await
        .unwrap();
    serde_json::from_str(&state).unwrap()
}

async fn expire(dir: &Path) {
    let mut state = state(dir).await;
    state["checked"] = 0.into();
    let mut db =
        SqliteConnection::connect(&format!("sqlite:{}", dir.join("catalog.sqlite").display()))
            .await
            .unwrap();
    sqlx::query("UPDATE current SET state=? WHERE id=1")
        .bind(state.to_string())
        .execute(&mut db)
        .await
        .unwrap();
}

async fn generation(dir: &Path) -> PathBuf {
    dir.join(state(dir).await["generation"].as_str().unwrap())
}

async fn example(service: &Ludusavi) -> Game {
    let games = service.by_id(StoreId::Steam(123)).await.unwrap();
    assert_eq!(games.len(), 1);
    games.into_iter().next().unwrap()
}

#[tokio::test]
async fn first_download_indexes_ids_normalized_titles_aliases_and_loose_fields() {
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "\"v1\"").await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = service(dir.path(), &server);
    assert_eq!(
        manifest.refresh(true).await.unwrap(),
        Refresh::Updated { games: 2 }
    );
    let game = example(&manifest).await;
    assert_eq!(game.title, "Example: Game");
    assert_eq!(
        game.files.values().next().unwrap().tags,
        ["save", "future-tag"]
    );
    assert_eq!(
        game.files.values().next().unwrap().when[1].os.as_deref(),
        Some("future-os")
    );
    assert_eq!(
        game.files.values().next().unwrap().when[1].extra["futureConstraint"],
        true
    );
    assert_eq!(game.registry.len(), 1);
    assert_eq!(game.extra["futureField"]["retained"], true);
    assert_eq!(game.id.extra["futureId"], "extra");
    for id in [
        StoreId::Steam(456),
        StoreId::Gog(1000000001),
        StoreId::Gog(1000000002),
    ] {
        assert_eq!(
            manifest.by_id(id).await.unwrap(),
            std::slice::from_ref(&game)
        );
    }
    for title in [
        "  EXAMPLE-game!",
        "example alias",
        "Alias Chain",
        "Ｅｘａｍｐｌｅ Ｇａｍｅ",
    ] {
        assert_eq!(
            manifest.by_title(title).await.unwrap(),
            std::slice::from_ref(&game)
        );
    }
    assert!(
        manifest
            .by_id(StoreId::Steam(999))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(manifest.by_title("Cycle One").await.unwrap().is_empty());
    assert!(manifest.by_title("Missing Alias").await.unwrap().is_empty());
    assert!(manifest.by_title("unknown").await.unwrap().is_empty());
    assert_eq!(state(dir.path()).await["etag"], "\"v1\"");
    assert_eq!(
        tokio::fs::read(generation(dir.path()).await.join("manifest.yaml"))
            .await
            .unwrap(),
        MANIFEST.as_bytes()
    );
    assert_eq!(manifest.refresh(true).await.unwrap(), Refresh::Fresh);
    // The persisted daily deadline also prevents a request after restart.
    assert_eq!(
        service(dir.path(), &server).refresh(true).await.unwrap(),
        Refresh::Fresh
    );
    server.verify().await;
}

#[tokio::test]
async fn offline_restart_uses_index_without_reading_yaml_or_expiring_entries() {
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "\"v1\"").await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = service(dir.path(), &server);
    manifest.refresh(true).await.unwrap();
    let expected = example(&manifest).await;
    expire(dir.path()).await;
    tokio::fs::remove_file(generation(dir.path()).await.join("manifest.yaml"))
        .await
        .unwrap();
    let url = format!("{}/manifest.yaml", server.uri());
    drop(manifest);
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .expect(0)
        .mount(&server)
        .await;
    for _ in 0..3 {
        let offline = Ludusavi::with_url(dir.path(), &url);
        assert_eq!(offline.refresh(false).await.unwrap(), Refresh::Offline);
        assert_eq!(example(&offline).await, expected);
        let start = Instant::now();
        for _ in 0..100 {
            assert_eq!(
                offline.by_title("Example Alias").await.unwrap(),
                std::slice::from_ref(&expected)
            );
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "100 indexed lookups: {:?}",
            start.elapsed()
        );
    }
    server.verify().await;
}

#[tokio::test]
async fn save_manifest_url_setting_defaults_validates_and_persists() {
    use caskade_core::{AppPaths, db::LibraryStore, settings::SettingsService};
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    assert_eq!(
        settings.get("core.saves").await.unwrap().value["manifestUrl"],
        caskade_core::ludusavi::MANIFEST_URL
    );
    for invalid in ["file:///etc/passwd", "", "https://example.test/with space"] {
        assert!(
            settings
                .set("core.saves", json!({"manifestUrl": invalid}), None)
                .await
                .is_err()
        );
    }
    settings
        .set(
            "core.saves",
            json!({"manifestUrl": "http://localhost:1234/manifest.yaml"}),
            None,
        )
        .await
        .unwrap();
    let restarted = SettingsService::new(store);
    restarted
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    assert_eq!(
        restarted.get("core.saves").await.unwrap().value["manifestUrl"],
        "http://localhost:1234/manifest.yaml"
    );
}

#[tokio::test]
async fn background_watcher_uses_configured_source_and_publishes_to_shared_cache() {
    use caskade_core::{AppPaths, db::LibraryStore, settings::SettingsService};
    use serde_json::json;
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "\"watch\"").await;
    let dir = tempfile::tempdir().unwrap();
    let settings = SettingsService::new(LibraryStore::in_memory().await.unwrap());
    settings
        .register_core(&AppPaths::under(dir.path()))
        .unwrap();
    settings
        .set(
            "core.saves",
            json!({"manifestUrl": format!("{}/manifest.yaml", server.uri())}),
            None,
        )
        .await
        .unwrap();
    let manifest = Ludusavi::new(dir.path());
    // Initialize the reader before the background download.
    assert!(
        manifest
            .by_id(StoreId::Steam(123))
            .await
            .unwrap()
            .is_empty()
    );
    let watcher = manifest.watch_settings(settings);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !manifest
                .by_id(StoreId::Steam(123))
                .await
                .unwrap()
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    watcher.abort();
    assert_eq!(example(&manifest).await.title, "Example: Game");
    server.verify().await;
}

#[tokio::test]
async fn not_modified_costs_one_request_and_never_parses_or_rebuilds() {
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "\"v1\"").await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = service(dir.path(), &server);
    manifest.refresh(true).await.unwrap();
    let original = generation(dir.path()).await;
    let index = tokio::fs::read(original.join("index.sqlite"))
        .await
        .unwrap();
    // Poison the cached YAML, proving a 304 and cold lookups don't parse it.
    tokio::fs::write(original.join("manifest.yaml"), "[invalid YAML")
        .await
        .unwrap();
    expire(dir.path()).await;
    server.reset().await;
    Mock::given(method("GET"))
        .and(header("if-none-match", "\"v1\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    let restarted = service(dir.path(), &server);
    assert_eq!(restarted.refresh(true).await.unwrap(), Refresh::Unchanged);
    assert_eq!(generation(dir.path()).await, original);
    assert_eq!(
        tokio::fs::read(original.join("index.sqlite"))
            .await
            .unwrap(),
        index
    );
    assert!(state(dir.path()).await["checked"].as_u64().unwrap() > 0);
    assert_eq!(example(&restarted).await.title, "Example: Game");
    assert_eq!(restarted.refresh(true).await.unwrap(), Refresh::Fresh);
    server.verify().await;
}

#[tokio::test]
async fn changed_etag_swaps_complete_generation_and_removes_obsolete_entries() {
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "\"v1\"").await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = service(dir.path(), &server);
    manifest.refresh(true).await.unwrap();
    let old = generation(dir.path()).await;
    let _ = example(&manifest).await; // Open the old connection pool too.
    expire(dir.path()).await;
    server.reset().await;
    Mock::given(method("GET"))
        .and(header("if-none-match", "\"v1\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v2\"")
                .set_body_string(
                    "New Game:\n  steam: {id: 123}\n  files: {new.sav: {tags: [save]}}\n",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        manifest.refresh(true).await.unwrap(),
        Refresh::Updated { games: 1 }
    );
    assert_ne!(generation(dir.path()).await, old);
    assert_eq!(state(dir.path()).await["etag"], "\"v2\"");
    assert_eq!(example(&manifest).await.title, "New Game");
    assert!(
        manifest
            .by_id(StoreId::Steam(789))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(manifest.by_title("Example Alias").await.unwrap().is_empty());
    assert_eq!(
        example(&service(dir.path(), &server)).await.title,
        "New Game"
    );
    server.verify().await;
}

#[tokio::test]
async fn failures_corrupt_yaml_and_truncated_gzip_preserve_good_cache_and_etag() {
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "\"v1\"").await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = service(dir.path(), &server);
    manifest.refresh(true).await.unwrap();
    expire(dir.path()).await;
    let original = state(dir.path()).await;
    let yaml = tokio::fs::read(generation(dir.path()).await.join("manifest.yaml"))
        .await
        .unwrap();
    let mut truncated = gzip(MANIFEST);
    truncated.truncate(truncated.len() - 8);
    for response in [
        ResponseTemplate::new(503),
        ResponseTemplate::new(200).set_body_string("Broken: [unterminated"),
        ResponseTemplate::new(200).set_body_string("Example:\n  files: [wrong-type]"),
        ResponseTemplate::new(200).set_body_string("{}"),
        ResponseTemplate::new(200)
            .insert_header("content-encoding", "gzip")
            .set_body_bytes(truncated),
    ] {
        server.reset().await;
        Mock::given(method("GET"))
            .and(header("if-none-match", "\"v1\""))
            .respond_with(response.insert_header("etag", "\"bad\""))
            .expect(1)
            .mount(&server)
            .await;
        assert!(manifest.refresh(true).await.is_err());
        assert_eq!(state(dir.path()).await, original);
        assert_eq!(
            tokio::fs::read(generation(dir.path()).await.join("manifest.yaml"))
                .await
                .unwrap(),
            yaml
        );
        assert_eq!(
            example(&service(dir.path(), &server)).await.title,
            "Example: Game"
        );
        server.verify().await;
    }
}

#[tokio::test]
async fn offline_first_start_then_first_connection_downloads() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = service(dir.path(), &server);
    assert!(manifest.by_title("Example").await.unwrap().is_empty());
    assert!(
        manifest
            .by_id(StoreId::Steam(123))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(manifest.refresh(false).await.unwrap(), Refresh::Offline);
    assert!(manifest.refresh(true).await.is_err());
    assert!(
        manifest
            .by_id(StoreId::Steam(123))
            .await
            .unwrap()
            .is_empty()
    );
    server.verify().await;
    server.reset().await;
    serve(&server, MANIFEST, "\"v1\"").await;
    assert_eq!(
        manifest.refresh(true).await.unwrap(),
        Refresh::Updated { games: 2 }
    );
    assert_eq!(example(&manifest).await.title, "Example: Game");
}

#[tokio::test]
async fn source_change_does_not_reuse_another_servers_etag_or_daily_deadline() {
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "\"v1\"").await;
    let dir = tempfile::tempdir().unwrap();
    service(dir.path(), &server).refresh(true).await.unwrap();
    let other = MockServer::start().await;
    serve(&other, MANIFEST, "\"other\"").await;
    service(dir.path(), &other).refresh(true).await.unwrap();
    assert!(
        !other.received_requests().await.unwrap()[0]
            .headers
            .contains_key("if-none-match")
    );
}

#[tokio::test]
async fn secondary_manifest_adds_paths_and_overrides_matching_entries() {
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "v1").await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = service(dir.path(), &server);
    manifest.refresh(true).await.unwrap();
    let primary = example(&manifest).await;
    let merged = apply_secondary(
        &primary,
        br#"'Example: Game':
  files:
    <home>/.local/share/example/*.sav: {tags: [config]}
    ./custom.sav: {tags: [save]}
  registry:
    HKEY_CURRENT_USER/Software/Custom: {}
  steam: {id: 321}
"#,
    )
    .unwrap();
    assert_eq!(merged.files.len(), 2);
    assert_eq!(
        merged.files["<home>/.local/share/example/*.sav"].tags,
        ["config"]
    );
    assert_eq!(merged.registry.len(), 2);
    assert_eq!(merged.steam.id, Some(321));
    assert_eq!(merged.gog, primary.gog);
    let game_dir = tempfile::tempdir().unwrap();
    assert!(
        manifest
            .load_secondary(game_dir.path())
            .await
            .unwrap()
            .is_empty()
    );
    tokio::fs::write(
        game_dir.path().join(".ludusavi.yaml"),
        br#"'Example: Game':
  files:
    ./custom.sav: {tags: [save]}
Brand New Game:
  files:
    ../shared.sav: {}
"#,
    )
    .await
    .unwrap();
    let secondary = manifest.load_secondary(game_dir.path()).await.unwrap();
    assert_eq!(secondary.len(), 2);
    assert_eq!(secondary[1].title, primary.title);
    assert!(secondary[1].files.contains_key("<base>/custom.sav"));
    assert_eq!(secondary[1].registry, primary.registry);
    assert!(secondary[0].files.contains_key("<base>/../shared.sav"));
    assert_eq!(example(&manifest).await, primary); // Overrides are folder-scoped.
}

#[tokio::test]
#[ignore = "downloads the current real Ludusavi manifest from GitHub"]
async fn live_manifest_download_and_offline_restart() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = Ludusavi::new(dir.path());
    let Refresh::Updated { games } = manifest.refresh(true).await.unwrap() else {
        panic!("expected download")
    };
    assert!(games > 1000);
    drop(manifest);
    let offline = Ludusavi::new(dir.path());
    assert_eq!(offline.refresh(false).await.unwrap(), Refresh::Offline);
    assert!(!offline.by_id(StoreId::Steam(400)).await.unwrap().is_empty());
}

#[tokio::test]
async fn update_now_checks_a_fresh_manifest_again_and_status_says_when() {
    let server = MockServer::start().await;
    serve(&server, MANIFEST, "\"v1\"").await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = service(dir.path(), &server);
    let none = manifest.status().await.unwrap();
    assert!(!none.available);
    assert_eq!(none.checked_at_ms, None);
    manifest.refresh(true).await.unwrap();
    let first = manifest.status().await.unwrap();
    assert!(first.available);
    let checked = first.checked_at_ms.unwrap();
    assert!(checked > 0);
    // Checked less than a day ago, so only Update now asks again.
    assert_eq!(manifest.refresh(true).await.unwrap(), Refresh::Fresh);
    server.verify().await;
    server.reset().await;
    Mock::given(method("GET"))
        .and(header("if-none-match", "\"v1\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(manifest.update_now().await.unwrap(), Refresh::Unchanged);
    assert!(manifest.status().await.unwrap().checked_at_ms.unwrap() >= checked);
    // A failed check keeps the manifest.
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    assert!(manifest.update_now().await.is_err());
    assert!(manifest.status().await.unwrap().available);
    assert_eq!(example(&manifest).await.title, "Example: Game");
    // Another source shares the cache.
    let other = manifest.for_url("http://127.0.0.1:9/manifest.yaml");
    assert!(other.status().await.unwrap().available);
    server.verify().await;
}
