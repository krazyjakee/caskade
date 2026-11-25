use std::{fs, sync::Arc, time::Duration};

use caskade_installer::{
    events::Phase,
    exec::CancellationToken,
    fetch::{FetchConfig, FetchEngine, FetchError, disposition_name},
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

use super::{checksum, config, engine, request};

#[test]
fn content_disposition_names_follow_rfc_precedence_and_path_safety() {
    for (header, expected) in [
        ("attachment; filename=game.zip", "game.zip"),
        ("attachment; filename=\"game; one.zip\"", "game; one.zip"),
        (
            "attachment; filename=old.zip; filename*=UTF-8'en'%E6%B8%B8%E6%88%8F.zip",
            "游戏.zip",
        ),
        ("attachment; filename*=ISO-8859-1''caf%E9.zip", "café.zip"),
    ] {
        assert_eq!(disposition_name(header).unwrap().as_deref(), Some(expected));
    }
    for name in [
        "../../evil.exe",
        "C:\\evil.exe",
        "CON",
        "folder/game.zip",
        "name. ",
    ] {
        assert!(
            disposition_name(&format!("attachment; filename=\"{name}\"")).is_err(),
            "{name}"
        );
    }
    assert!(
        disposition_name("attachment; filename=a.zip; filename=b.zip")
            .unwrap()
            .is_none()
    );
}

#[test]
fn disposition_rejects_trailing_separators_and_nonliteral_leaf_names() {
    let regression =
        include_str!("../regressions/21-content-disposition-trailing-separator.txt").trim();
    assert!(disposition_name(regression).is_err());
    for header in [
        "attachment; filename=p/",
        "attachment; filename=p\\",
        "attachment; filename=p/.",
        "attachment; filename=./p",
        "attachment; filename*=UTF-8''p%2F",
        "attachment; filename*=UTF-8''p%5C",
    ] {
        assert!(disposition_name(header).is_err(), "{header}");
    }
}

#[tokio::test]
async fn relative_redirect_unicode_name_checksum_and_progress_use_the_real_body() {
    let server = MockServer::start().await;
    Mock::given(path("/start"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/final.zip"))
        .mount(&server)
        .await;
    Mock::given(path("/final.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"fixture archive".to_vec())
                .insert_header(
                    "content-disposition",
                    "attachment; filename*=UTF-8''%E6%B8%B8%E6%88%8F.zip",
                ),
        )
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let fetch = engine(config(), sink.clone());
    let mut input = request(&server, "/start");
    input
        .url
        .set_fragment(Some(&format!("sha256={}", checksum(b"fixture archive"))));
    let file = fetch
        .download(root.path(), "job", &input, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(file.path.file_name().unwrap(), "游戏.zip");
    assert_eq!(fs::read(&file.path).unwrap(), b"fixture archive");
    assert_eq!(file.final_url.path(), "/final.zip");
    assert!(file.insecure_transport);
    assert_eq!(file.sha256, checksum(b"fixture archive"));
    assert!(
        sink.events()
            .iter()
            .all(|event| event.phase == Phase::Download)
    );
    fetch.after_install(root.path(), "job").await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn hostile_names_html_and_bad_checksums_never_become_input_files() {
    let server = MockServer::start().await;
    for (path_name, response) in [
        (
            "/evil",
            ResponseTemplate::new(200)
                .set_body_string("payload")
                .insert_header(
                    "content-disposition",
                    "attachment; filename=\"../../sentinel\"",
                ),
        ),
        (
            "/html",
            ResponseTemplate::new(200)
                .set_body_string("<html>error</html>")
                .insert_header("content-type", "text/html"),
        ),
        (
            "/sniff",
            ResponseTemplate::new(200)
                .set_body_bytes(b"  <!DOCTYPE html><html>error</html>".to_vec()),
        ),
        (
            "/checksum",
            ResponseTemplate::new(200).set_body_bytes(b"payload".to_vec()),
        ),
    ] {
        Mock::given(path(path_name))
            .respond_with(response)
            .mount(&server)
            .await;
    }
    let root = TestRoot::new().unwrap();
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    for (id, path_name) in ["/evil", "/html", "/sniff", "/checksum"]
        .into_iter()
        .enumerate()
    {
        let mut input = request(&server, path_name);
        if path_name == "/checksum" {
            input.sha256 = Some("0".repeat(64));
        }
        let result = fetch
            .download(
                root.path(),
                &format!("job-{id}"),
                &input,
                CancellationToken::new(),
            )
            .await;
        assert!(result.is_err(), "{path_name}");
        assert!(
            !root
                .path()
                .join(".caskade/downloads")
                .join(format!("job-{id}"))
                .join(path_name.trim_start_matches('/'))
                .is_file()
        );
        root.assert_sentinel_untouched();
    }
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn redirect_limit_and_cross_origin_header_stripping_are_enforced() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    Mock::given(path("/loop"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/loop"))
        .mount(&first)
        .await;
    Mock::given(path("/cross"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/game.zip", second.uri())),
        )
        .mount(&first)
        .await;
    Mock::given(path("/game.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data".to_vec()))
        .mount(&second)
        .await;
    let root = TestRoot::new().unwrap();
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    assert!(matches!(
        fetch
            .download(
                root.path(),
                "loop",
                &request(&first, "/loop"),
                CancellationToken::new()
            )
            .await,
        Err(FetchError::RedirectLimit)
    ));
    let mut input = request(&first, "/cross");
    input
        .headers
        .insert("authorization".into(), "Bearer secret".into());
    input.headers.insert("x-api-key".into(), "secret".into());
    fetch
        .download(root.path(), "cross", &input, CancellationToken::new())
        .await
        .unwrap();
    let seen = second.received_requests().await.unwrap();
    assert!(!seen[0].headers.contains_key("authorization"));
    assert!(!seen[0].headers.contains_key("x-api-key"));
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn a_plugin_download_and_its_redirects_stay_within_its_network_grant() {
    let server = MockServer::start().await;
    let port = server.address().port();
    Mock::given(path("/game.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data".to_vec()))
        .mount(&server)
        .await;
    Mock::given(path("/away"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("http://localhost:{port}/game.zip")),
        )
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    let granted = |path: &str, hosts: &[&str]| {
        let mut input = request(&server, path);
        input.allowed_hosts = Some(hosts.iter().map(|host| (*host).to_owned()).collect());
        input
    };
    // Loopback only when the grant names the address.
    assert!(matches!(
        fetch
            .download(
                root.path(),
                "unnamed",
                &granted("/game.zip", &["example.com"]),
                CancellationToken::new()
            )
            .await,
        Err(FetchError::NotGranted { .. })
    ));
    fetch
        .download(
            root.path(),
            "named",
            &granted("/game.zip", &["127.0.0.1"]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    // A redirect to a host outside the grant is never followed.
    assert!(matches!(
        fetch
            .download(
                root.path(),
                "away",
                &granted("/away", &["127.0.0.1"]),
                CancellationToken::new()
            )
            .await,
        Err(FetchError::NotGranted { .. })
    ));
    let fetched = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|seen| seen.url.path() == "/game.zip")
        .count();
    assert_eq!(fetched, 1);
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn keep_downloads_and_active_jobs_control_orphan_cleanup() {
    let server = MockServer::start().await;
    Mock::given(path("/game.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"keep".to_vec()))
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let mut keep = config();
    keep.keep_downloads = true;
    let fetch = engine(keep, Arc::new(RecordingSink::default()));
    let file = fetch
        .download(
            root.path(),
            "keep",
            &request(&server, "/game.zip"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    fetch.after_install(root.path(), "keep").await.unwrap();
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    assert!(file.path.exists());
    let normal = engine(config(), Arc::new(RecordingSink::default()));
    normal
        .cleanup_orphans(root.path(), &["keep".into()])
        .await
        .unwrap();
    assert!(file.path.exists());
    normal.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn default_queue_allows_two_downloads() {
    assert_eq!(FetchConfig::default().concurrent_downloads, 2);
    let server = MockServer::start().await;
    Mock::given(path("/game.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"queued".to_vec())
                .set_delay(Duration::from_millis(150)),
        )
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    let mut tasks = vec![];
    for index in 0..3 {
        let fetch = fetch.clone();
        let root = root.path().to_owned();
        let input = request(&server, "/game.zip");
        tasks.push(tokio::spawn(async move {
            fetch
                .download(
                    &root,
                    &format!("job-{index}"),
                    &input,
                    CancellationToken::new(),
                )
                .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if server.received_requests().await.unwrap().len() == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn disk_and_size_caps_are_checked_without_finishing_an_invalid_download() {
    let root = TestRoot::new().unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/game.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1; 100]))
        .mount(&server)
        .await;
    let fetch = FetchEngine::new(
        config(),
        Arc::new(FakeDiskSpace::fixed(10)),
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    assert!(matches!(
        fetch
            .download(
                root.path(),
                "disk",
                &request(&server, "/game.zip"),
                CancellationToken::new()
            )
            .await,
        Err(FetchError::DiskFull { .. })
    ));
    let mut limited = config();
    limited.max_bytes = 10;
    let fetch = engine(limited, Arc::new(RecordingSink::default()));
    assert!(matches!(
        fetch
            .download(
                root.path(),
                "size",
                &request(&server, "/game.zip"),
                CancellationToken::new()
            )
            .await,
        Err(FetchError::TooLarge { .. })
    ));
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
}
