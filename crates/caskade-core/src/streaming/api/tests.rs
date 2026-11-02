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

#[tokio::test]
async fn pairing_requests_are_listed_answered_and_gone_once_they_expire() {
    use wiremock::matchers::body_json;
    let served = cert();
    let server = MockServer::start().await;
    // As Sunshine 2026.914 answers: one waiting request, then none once it
    // expired.
    Mock::given(method("GET"))
        .and(path("/api/pin"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pairings": [{ "address": "192.168.1.9", "id": "127922b28dc2366b54c5eb9db336f9a5", "name": "roth" }]
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/pin"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "pairings": [] })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/pin"))
        .and(body_json(serde_json::json!({
            "pairing_id": "127922b28dc2366b54c5eb9db336f9a5", "pin": "4831", "name": "Bedroom"
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": true })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/pin"))
        .and(body_json(
            serde_json::json!({ "pairing_id": "0e8f2506f60f975d8ff814b690a21dcf" }),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": true })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/clients/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "named_certs": [{ "enabled": true, "name": "Bedroom", "uuid": "02E2AB4E-9C07-B42E-098A-B680B3AC3BE2" }],
            "status": true
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/clients/unpair"))
        .and(body_json(
            serde_json::json!({ "uuid": "02E2AB4E-9C07-B42E-098A-B680B3AC3BE2" }),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": true })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let front = tls_front(*server.address(), &served).await;
    let api = HostApi::new(
        Url::parse(&format!("https://{front}/")).unwrap(),
        credentials(),
        served.der.clone(),
    )
    .unwrap();
    let waiting = api.pair_requests().await.unwrap();
    assert_eq!(
        waiting,
        [PairRequest {
            id: "127922b28dc2366b54c5eb9db336f9a5".into(),
            name: "roth".into(),
            address: "192.168.1.9".into(),
        }]
    );
    api.accept_pairing(&waiting[0].id, "4831", "Bedroom")
        .await
        .unwrap();
    api.decline_pairing("0e8f2506f60f975d8ff814b690a21dcf")
        .await
        .unwrap();
    // Expired: gone from the host's list.
    assert!(api.pair_requests().await.unwrap().is_empty());
    let paired = api.paired_clients().await.unwrap();
    assert_eq!(paired[0].name, "Bedroom");
    api.unpair(&paired[0].uuid).await.unwrap();
    server.verify().await;
}
