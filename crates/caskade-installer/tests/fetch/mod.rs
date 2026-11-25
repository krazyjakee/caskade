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
    events::{EventSink, JobEvent},
    exec::CancellationToken,
    fetch::{DownloadRequest, FetchConfig, FetchEngine},
    testing::{FakeDiskSpace, TestRoot},
};
use sha2::{Digest, Sha256};
use url::Url;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

mod basics;
#[cfg(feature = "cli")]
mod cli;
mod resume;
mod transport;

pub(super) fn config() -> FetchConfig {
    FetchConfig {
        retries: 0,
        disk_margin_bytes: 0,
        initial_backoff: Duration::ZERO,
        max_backoff: Duration::ZERO,
        stall_timeout: Duration::from_secs(5),
        ..FetchConfig::default()
    }
}

pub(super) fn engine(config: FetchConfig, sink: Arc<dyn EventSink>) -> FetchEngine {
    FetchEngine::new(config, Arc::new(FakeDiskSpace::fixed(u64::MAX)), sink).unwrap()
}

pub(super) fn request(server: &MockServer, path: &str) -> DownloadRequest {
    DownloadRequest::new(Url::parse(&format!("{}{path}", server.uri())).unwrap())
}

pub(super) fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) struct AbortAtProgress {
    pub(super) handle: Mutex<Option<tokio::task::AbortHandle>>,
    pub(super) bytes: AtomicU64,
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

pub(super) async fn interrupt(
    root: &TestRoot,
    server: &MockServer,
    etag: &str,
) -> (DownloadRequest, u64) {
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

/// Reads one request's head from a raw test connection.
pub(super) async fn read_head(stream: &mut tokio::net::TcpStream) -> String {
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
