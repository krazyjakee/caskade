use std::{fs, sync::Arc};

use caskade_installer::{
    events::{EventSink, JobEvent},
    exec::CancellationToken,
    fetch::FetchError,
    testing::{RecordingSink, TestRoot},
};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{header, path},
};

use super::{config, engine, interrupt, request};

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
