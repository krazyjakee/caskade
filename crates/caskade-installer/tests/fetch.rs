#![cfg(feature = "fetch")]

use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use caskade_installer::{
    events::{EventSink, JobEvent, Phase},
    exec::CancellationToken,
    fetch::{DownloadRequest, FetchConfig, FetchEngine, FetchError, disposition_name},
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
};
use sha2::{Digest, Sha256};
use url::Url;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{header, path},
};

fn config() -> FetchConfig {
    FetchConfig {
        retries: 0,
        disk_margin_bytes: 0,
        initial_backoff: Duration::ZERO,
        max_backoff: Duration::ZERO,
        stall_timeout: Duration::from_secs(5),
        ..FetchConfig::default()
    }
}
fn engine(config: FetchConfig, sink: Arc<dyn EventSink>) -> FetchEngine {
    FetchEngine::new(config, Arc::new(FakeDiskSpace::fixed(u64::MAX)), sink).unwrap()
}
fn request(server: &MockServer, path: &str) -> DownloadRequest {
    DownloadRequest::new(Url::parse(&format!("{}{path}", server.uri())).unwrap())
}
fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

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
        include_str!("regressions/21-content-disposition-trailing-separator.txt").trim();
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

struct AbortAtProgress {
    handle: Mutex<Option<tokio::task::AbortHandle>>,
    bytes: AtomicU64,
}
impl EventSink for AbortAtProgress {
    fn emit(&self, event: JobEvent) {
        if event.message == "progress" {
            self.bytes.store(event.bytes, Ordering::SeqCst);
            if let Some(handle) = self.handle.lock().unwrap().as_ref() {
                handle.abort();
            }
        }
    }
}

async fn interrupt(root: &TestRoot, server: &MockServer, etag: &str) -> (DownloadRequest, u64) {
    Mock::given(path("/game.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![1; 2 * 1024 * 1024])
                .insert_header("etag", etag),
        )
        .mount(server)
        .await;
    let sink = Arc::new(AbortAtProgress {
        handle: Mutex::new(None),
        bytes: AtomicU64::new(0),
    });
    let fetch = engine(config(), sink.clone());
    let input = request(server, "/game.zip");
    let job_input = input.clone();
    let job_root = root.path().to_owned();
    let start = Arc::new(tokio::sync::Notify::new());
    let job_start = start.clone();
    let task = tokio::spawn(async move {
        job_start.notified().await;
        fetch
            .download(&job_root, "resume", &job_input, CancellationToken::new())
            .await
    });
    *sink.handle.lock().unwrap() = Some(task.abort_handle());
    start.notify_one();
    assert!(task.await.unwrap_err().is_cancelled());
    let bytes = fs::metadata(root.path().join(".caskade/downloads/resume/game.zip.part"))
        .unwrap()
        .len();
    assert!(bytes > 0 && bytes < 2 * 1024 * 1024);
    assert_eq!(bytes, sink.bytes.load(Ordering::SeqCst));
    server.reset().await;
    (input, bytes)
}

#[tokio::test]
async fn range_resume_survives_a_dropped_future_and_a_new_engine() {
    let root = TestRoot::new().unwrap();
    let server = MockServer::start().await;
    let (input, offset) = interrupt(&root, &server, "\"v1\"").await;
    Mock::given(path("/game.zip"))
        .and(header("range", format!("bytes={offset}-")))
        .and(header("if-range", "\"v1\""))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(vec![1; 2 * 1024 * 1024 - offset as usize])
                .insert_header("etag", "\"v1\"")
                .insert_header(
                    "content-range",
                    format!("bytes {offset}-{}/{}", 2 * 1024 * 1024 - 1, 2 * 1024 * 1024),
                ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    assert!(
        root.path()
            .join(".caskade/downloads/resume/game.zip.part")
            .exists()
    );
    let file = fetch
        .download(root.path(), "resume", &input, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(fs::read(&file.path).unwrap(), vec![1; 2 * 1024 * 1024]);
    fetch.after_install(root.path(), "resume").await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn changed_etag_restarts_instead_of_appending_a_different_entity() {
    let root = TestRoot::new().unwrap();
    let server = MockServer::start().await;
    let (input, offset) = interrupt(&root, &server, "\"v1\"").await;
    Mock::given(path("/game.zip"))
        .respond_with(move |request: &Request| {
            if request.headers.contains_key("range") {
                ResponseTemplate::new(206)
                    .set_body_bytes(vec![2; 2 * 1024 * 1024 - offset as usize])
                    .insert_header("etag", "\"v2\"")
                    .insert_header(
                        "content-range",
                        format!("bytes {offset}-{}/{}", 2 * 1024 * 1024 - 1, 2 * 1024 * 1024),
                    )
            } else {
                ResponseTemplate::new(200)
                    .set_body_bytes(vec![2; 2 * 1024 * 1024])
                    .insert_header("etag", "\"v2\"")
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    let file = fetch
        .download(root.path(), "resume", &input, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(fs::read(file.path).unwrap(), vec![2; 2 * 1024 * 1024]);
    fetch.after_install(root.path(), "resume").await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn slow_response_can_be_cancelled_or_timed_out() {
    let server = MockServer::start().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let observed = entered.clone();
    Mock::given(path("/slow.zip"))
        .respond_with(move |_: &Request| {
            observed.notify_one();
            ResponseTemplate::new(200)
                .set_body_bytes(vec![1; 10])
                .set_delay(Duration::from_secs(3))
        })
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    let input = request(&server, "/slow.zip");
    let token = CancellationToken::new();
    let cancel = token.clone();
    let job_root = root.path().to_owned();
    let job_fetch = fetch.clone();
    let task =
        tokio::spawn(async move { job_fetch.download(&job_root, "cancel", &input, token).await });
    entered.notified().await;
    cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap(),
        Err(FetchError::Cancelled)
    ));
    let mut short = config();
    short.stall_timeout = Duration::from_millis(20);
    let timeout = engine(short, Arc::new(RecordingSink::default()))
        .download(
            root.path(),
            "timeout",
            &request(&server, "/slow.zip"),
            CancellationToken::new(),
        )
        .await;
    assert!(matches!(timeout, Err(FetchError::Http { .. })));
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
async fn weak_etags_restart_and_unvalidated_partial_orphans_are_removed() {
    let root = TestRoot::new().unwrap();
    let server = MockServer::start().await;
    let (input, _) = interrupt(&root, &server, "W/\"weak\"").await;
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
    Mock::given(path("/game.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new".to_vec()))
        .mount(&server)
        .await;
    let file = fetch
        .download(root.path(), "resume", &input, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(fs::read(file.path).unwrap(), b"new");
    assert!(
        !server.received_requests().await.unwrap()[0]
            .headers
            .contains_key("range")
    );
    fetch.after_install(root.path(), "resume").await.unwrap();
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
async fn a_connection_dropped_mid_body_retries_from_the_validated_prefix() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = vec![];
        for index in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![];
            loop {
                let mut chunk = [0; 1024];
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
                    break;
                }
            }
            requests.push(String::from_utf8(request).unwrap());
            let response = if index == 0 {
                b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nabcd".as_slice()
            } else {
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 6\r\nContent-Range: bytes 4-9/10\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nefghij".as_slice()
            };
            stream.write_all(response).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        requests
    });
    let root = TestRoot::new().unwrap();
    let mut retry = config();
    retry.retries = 1;
    let fetch = engine(retry, Arc::new(RecordingSink::default()));
    let input = DownloadRequest::new(Url::parse(&format!("http://{address}/game.zip")).unwrap());
    let file = tokio::time::timeout(
        Duration::from_secs(3),
        fetch.download(root.path(), "drop", &input, CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(fs::read(file.path).unwrap(), b"abcdefghij");
    let requests = server.await.unwrap();
    assert!(requests[1].to_ascii_lowercase().contains("range: bytes=4-"));
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("if-range: \"v1\"")
    );
    fetch.after_install(root.path(), "drop").await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn missing_content_length_is_streamed_and_checksum_verified() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).await.unwrap() > 0);
        stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/zip\r\n\r\nunknown length").await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let root = TestRoot::new().unwrap();
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    let mut input = DownloadRequest::new(Url::parse(&format!("http://{address}/")).unwrap());
    input.sha256 = Some(checksum(b"unknown length"));
    let file = fetch
        .download(root.path(), "unknown", &input, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(file.path.file_name().unwrap(), "download.zip");
    assert_eq!(file.size, 14);
    server.await.unwrap();
    fetch.after_install(root.path(), "unknown").await.unwrap();
    root.assert_clean();
}

/// Reads one request's head from a raw test connection.
async fn read_head(stream: &mut tokio::net::TcpStream) -> String {
    use tokio::io::AsyncReadExt;
    let mut request = vec![];
    while !request.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
        let mut chunk = [0; 1024];
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        request.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(request).unwrap().to_ascii_lowercase()
}

#[tokio::test]
async fn a_slow_download_that_keeps_sending_outlives_the_stall_timeout() {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_head(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 25\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        for _ in 0..25 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            stream.write_all(b"x").await.unwrap();
            stream.flush().await.unwrap();
        }
        stream.shutdown().await.unwrap();
    });
    let root = TestRoot::new().unwrap();
    let mut slow = config();
    slow.stall_timeout = Duration::from_millis(300);
    let fetch = engine(slow, Arc::new(RecordingSink::default()));
    let input = DownloadRequest::new(Url::parse(&format!("http://{address}/slow.bin")).unwrap());
    let started = std::time::Instant::now();
    let file = fetch
        .download(root.path(), "slow", &input, CancellationToken::new())
        .await
        .unwrap();
    // The whole transfer took several stall timeouts; no read waited one.
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert_eq!(fs::read(&file.path).unwrap(), vec![b'x'; 25]);
    server.await.unwrap();
    fetch.after_install(root.path(), "slow").await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn attempts_that_get_further_into_the_file_do_not_use_up_the_retries() {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = vec![];
        for response in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nabcd".as_slice(),
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 6\r\nContent-Range: bytes 4-9/10\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nef",
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 6-9/10\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nghij",
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_head(&mut stream).await);
            stream.write_all(response).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        requests
    });
    let root = TestRoot::new().unwrap();
    let mut retry = config();
    // Three dropped connections, but each one got further than the last.
    retry.retries = 1;
    let fetch = engine(retry, Arc::new(RecordingSink::default()));
    let input = DownloadRequest::new(Url::parse(&format!("http://{address}/game.zip")).unwrap());
    let file = tokio::time::timeout(
        Duration::from_secs(5),
        fetch.download(root.path(), "flaky", &input, CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(fs::read(file.path).unwrap(), b"abcdefghij");
    let requests = server.await.unwrap();
    assert!(requests[1].contains("range: bytes=4-"));
    assert!(requests[2].contains("range: bytes=6-"));
    fetch.after_install(root.path(), "flaky").await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn a_web_page_in_place_of_the_file_keeps_the_partial_for_resume() {
    let root = TestRoot::new().unwrap();
    let server = MockServer::start().await;
    let (input, offset) = interrupt(&root, &server, "\"v1\"").await;
    let part = root.path().join(".caskade/downloads/resume/game.zip.part");
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    // A captive portal or login page, labelled as HTML or only sniffed.
    for page in [
        ResponseTemplate::new(200)
            .set_body_string("<html>Sign in</html>")
            .insert_header("content-type", "text/html"),
        ResponseTemplate::new(200).set_body_bytes(b"<!DOCTYPE html><html>Sign in</html>".to_vec()),
    ] {
        Mock::given(path("/game.zip"))
            .respond_with(page)
            .mount(&server)
            .await;
        assert!(matches!(
            fetch
                .download(root.path(), "resume", &input, CancellationToken::new())
                .await,
            Err(FetchError::Html)
        ));
        assert_eq!(fs::metadata(&part).unwrap().len(), offset);
        server.reset().await;
    }
    Mock::given(path("/game.zip"))
        .and(header("range", format!("bytes={offset}-")))
        .and(header("if-range", "\"v1\""))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(vec![1; 2 * 1024 * 1024 - offset as usize])
                .insert_header("etag", "\"v1\"")
                .insert_header(
                    "content-range",
                    format!("bytes {offset}-{}/{}", 2 * 1024 * 1024 - 1, 2 * 1024 * 1024),
                ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let file = fetch
        .download(root.path(), "resume", &input, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(fs::read(&file.path).unwrap(), vec![1; 2 * 1024 * 1024]);
    fetch.after_install(root.path(), "resume").await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn a_server_that_stops_answering_is_named_with_the_cause_but_not_the_url() {
    // Connections queue in the backlog and are never answered.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let root = TestRoot::new().unwrap();
    let mut short = config();
    short.stall_timeout = Duration::from_millis(100);
    let fetch = engine(short, Arc::new(RecordingSink::default()));
    let input =
        DownloadRequest::new(Url::parse(&format!("http://{address}/token-123/game.zip")).unwrap());
    let result = fetch
        .download(root.path(), "offline", &input, CancellationToken::new())
        .await;
    let Err(FetchError::Http { message }) = result else {
        panic!("a silent server is an HTTP failure: {result:?}");
    };
    assert!(
        message.starts_with("the server timed out (error sending request: "),
        "{message}"
    );
    assert!(!message.contains("token-123"), "{message}");
    drop(listener);
    fetch.cleanup_orphans(root.path(), &[]).await.unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn strong_last_modified_validator_resumes_without_an_etag() {
    let root = TestRoot::new().unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/dated.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![3; 2 * 1024 * 1024])
                .insert_header("last-modified", "Mon, 01 Jan 2024 00:00:00 GMT")
                .insert_header("date", "Mon, 01 Jan 2024 00:02:00 GMT"),
        )
        .mount(&server)
        .await;
    let token = CancellationToken::new();
    struct CancelProgress(CancellationToken);
    impl EventSink for CancelProgress {
        fn emit(&self, event: JobEvent) {
            if event.message == "progress" {
                self.0.cancel();
            }
        }
    }
    let fetch = engine(config(), Arc::new(CancelProgress(token.clone())));
    let input = request(&server, "/dated.zip");
    assert!(matches!(
        fetch.download(root.path(), "dated", &input, token).await,
        Err(FetchError::Cancelled)
    ));
    let offset = fs::metadata(root.path().join(".caskade/downloads/dated/dated.zip.part"))
        .unwrap()
        .len();
    server.reset().await;
    Mock::given(path("/dated.zip"))
        .and(|request: &Request| {
            request
                .headers
                .get("if-range")
                .is_some_and(|value| value == "Mon, 01 Jan 2024 00:00:00 GMT")
        })
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(vec![3; 2 * 1024 * 1024 - offset as usize])
                .insert_header("last-modified", "Mon, 01 Jan 2024 00:00:00 GMT")
                .insert_header("date", "Mon, 01 Jan 2024 00:02:00 GMT")
                .insert_header(
                    "content-range",
                    format!("bytes {offset}-{}/{}", 2 * 1024 * 1024 - 1, 2 * 1024 * 1024),
                ),
        )
        .mount(&server)
        .await;
    let fetch = engine(config(), Arc::new(RecordingSink::default()));
    let result = fetch
        .download(root.path(), "dated", &input, CancellationToken::new())
        .await;
    let file = result.unwrap();
    assert_eq!(fs::read(file.path).unwrap(), vec![3; 2 * 1024 * 1024]);
    fetch.after_install(root.path(), "dated").await.unwrap();
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

#[cfg(feature = "cli")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_cli_downloads_a_real_zipped_fixture_and_writes_a_verified_manifest() {
    let server = MockServer::start().await;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/unity.zip");
    let body = fs::read(fixture).unwrap();
    Mock::given(path("/game.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.clone())
                .insert_header("content-disposition", "attachment; filename=unity.zip"),
        )
        .mount(&server)
        .await;
    let root = TestRoot::new().unwrap();
    let target_root = root.path().to_owned();
    let url = format!("{}/game.zip#sha256={}", server.uri(), checksum(&body));
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .envs(caskade_installer::testing::windows_runner_path().map(|path| ("PATH", path)))
            .arg("--json")
            .arg("install")
            .arg(url)
            .arg("--root")
            .arg(target_root)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(outcome["type"], "installed");
    let game = root.path().join(outcome["value"]["dir"].as_str().unwrap());
    assert!(game.join("FixtureGame.exe").is_file());
    let manifest = caskade_installer::manifest::InstallManifest::read(&game).unwrap();
    assert_eq!(
        manifest.record().source.checksum.as_ref().unwrap().as_str(),
        checksum(&body)
    );
    assert!(matches!(
        manifest.record().source.input,
        caskade_installer::Input::Urls(_)
    ));
    let verification = manifest.verify(&game).unwrap();
    assert!(verification.missing.is_empty() && verification.changed.is_empty());
    assert!(!root.path().join(".caskade/downloads").exists());
    root.assert_sentinel_untouched();
}

#[cfg(feature = "cli")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_cli_accepts_a_local_archive_without_deleting_the_source() {
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/unity.zip");
    let root = TestRoot::new().unwrap();
    let source = fixture.clone();
    let target_root = root.path().to_owned();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .envs(caskade_installer::testing::windows_runner_path().map(|path| ("PATH", path)))
            .arg("--json")
            .arg("install")
            .arg(source)
            .arg("--root")
            .arg(target_root)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.exists());
    root.assert_sentinel_untouched();
}
