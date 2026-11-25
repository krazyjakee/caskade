use std::{fs, sync::Arc, time::Duration};

use caskade_installer::{
    exec::CancellationToken,
    fetch::{DownloadRequest, FetchError},
    testing::{RecordingSink, TestRoot},
};
use url::Url;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};

use super::{checksum, config, engine, read_head, request};

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
