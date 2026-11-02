//! The API client against wiremock behind a TLS front with a certificate
//! made per test: the right one, and another.

use super::*;
use crate::streaming::testing::{Cert, cert, tls_front};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{basic_auth, body_partial_json, header_exists, method, path},
};

fn credentials() -> Credentials {
    Credentials {
        user: "caskade-1".into(),
        password: "secret".into(),
    }
}

async fn host(served: &Cert) -> (MockServer, Url) {
    let server = MockServer::start().await;
    // Anything carrying an Origin (Sunshine's CSRF check) is refused.
    Mock::given(header_exists("origin"))
        .respond_with(ResponseTemplate::new(400))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/config"))
        .and(basic_auth("caskade-1", "secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": true, "version": "2026.914.233613", "platform": "linux"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/apps"))
        .and(basic_auth("caskade-1", "secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "env": {},
            "apps": [{"name": "Their Desktop", "image-path": "desktop.png", "prep-cmd": []}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/apps"))
        .and(basic_auth("caskade-1", "secret"))
        .and(body_partial_json(serde_json::json!({
            "name": "Hades", "cmd": "caskade --launch g1", "index": -1, "wait-all": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": true})))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/apps/0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": true})))
        .mount(&server)
        .await;
    // A wrong password.
    Mock::given(basic_auth("caskade-1", "wrong"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let front = tls_front(*server.address(), served).await;
    let base = Url::parse(&format!("https://{front}/")).unwrap();
    (server, base)
}

#[tokio::test]
async fn the_pinned_host_answers_its_version_and_apps() {
    let served = cert();
    let (server, base) = host(&served).await;
    let temp = tempfile::tempdir().unwrap();
    let pem = temp.path().join("cacert.pem");
    std::fs::write(&pem, &served.pem).unwrap();
    let api = HostApi::with_cert_file(base.clone(), credentials(), &pem).unwrap();
    assert_eq!(api.version().await.unwrap(), "2026.914.233613");
    let apps = api.apps().await.unwrap();
    assert_eq!(apps[0].name, "Their Desktop");
    // Fields Caskade doesn't know come back as they were.
    assert_eq!(apps[0].rest["image-path"], "desktop.png");
    api.add_app(&App::command("Hades", "caskade --launch g1"))
        .await
        .unwrap();
    api.delete_app(0).await.unwrap();
    let methods: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| format!("{} {}", request.method, request.url.path()))
        .collect();
    assert_eq!(
        methods,
        [
            "GET /api/config",
            "GET /api/apps",
            "POST /api/apps",
            "DELETE /api/apps/0"
        ]
    );

    let wrong = HostApi::new(
        base,
        Credentials {
            password: "wrong".into(),
            ..credentials()
        },
        served.der.clone(),
    )
    .unwrap();
    assert!(matches!(wrong.version().await, Err(Error::Forbidden(_))));
}

#[tokio::test]
async fn any_other_certificate_is_refused_before_a_request() {
    let served = cert();
    let (server, base) = host(&served).await;
    let other = cert();
    let api = HostApi::new(base, credentials(), other.der).unwrap();
    assert!(api.version().await.is_err());
    assert!(api.apps().await.is_err());
    // Nothing reached the host: not even the credentials.
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn an_unreadable_certificate_is_said_plainly() {
    let temp = tempfile::tempdir().unwrap();
    let pem = temp.path().join("cacert.pem");
    std::fs::write(&pem, "not a certificate").unwrap();
    let error = HostApi::with_cert_file(Url::parse(LOCAL).unwrap(), credentials(), &pem)
        .unwrap_err()
        .to_string();
    assert!(error.contains("certificate is unreadable"), "{error}");
}
