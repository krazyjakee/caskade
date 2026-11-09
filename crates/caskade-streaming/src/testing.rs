//! Test helpers: certificates made per test and a TLS front for wiremock,
//! which speaks plain HTTP.

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::{net::SocketAddr, sync::Arc};

pub struct Cert {
    pub der: CertificateDer<'static>,
    pub pem: String,
    key: Vec<u8>,
}

impl Cert {
    /// A certificate and its PKCS#8 key, from PEM.
    pub fn from_pem(cert_pem: &str, key_pem: &str) -> Self {
        use rustls::pki_types::pem::PemObject;
        Self {
            der: CertificateDer::from_pem_slice(cert_pem.as_bytes()).unwrap(),
            pem: cert_pem.to_owned(),
            key: rustls::pki_types::PrivatePkcs8KeyDer::from_pem_slice(key_pem.as_bytes())
                .unwrap()
                .secret_pkcs8_der()
                .to_vec(),
        }
    }
}

/// A self-signed certificate like the host's own.
pub fn cert() -> Cert {
    let made =
        rcgen::generate_simple_self_signed(vec!["Sunshine Gamestream Host".to_owned()]).unwrap();
    Cert {
        der: made.cert.der().clone(),
        pem: made.cert.pem(),
        key: made.signing_key.serialize_der(),
    }
}

/// Terminates TLS with `cert` and passes the bytes to `upstream`.
pub async fn tls_front(upstream: SocketAddr, cert: &Cert) -> SocketAddr {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der.clone()],
            PrivateKeyDer::try_from(cert.key.clone()).unwrap(),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let Ok(mut up) = tokio::net::TcpStream::connect(upstream).await else {
                    return;
                };
                let _ = tokio::io::copy_bidirectional(&mut tls, &mut up).await;
            });
        }
    });
    address
}

/// Sunshine's app list, kept as the real one does: added at the end, then
/// sorted by name; deleted by index.
#[cfg(unix)]
#[derive(Clone, Default)]
pub struct Apps(pub Arc<std::sync::Mutex<Vec<serde_json::Value>>>);

#[cfg(unix)]
impl wiremock::Respond for Apps {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let mut apps = caskade_core::lock(&self.0);
        match request.method.as_str() {
            "GET" => wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "env": {}, "apps": *apps })),
            "POST" => {
                let mut app: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                app.as_object_mut().unwrap().remove("index");
                apps.push(app);
                apps.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": true }))
            }
            "DELETE" => {
                let index: usize = request
                    .url
                    .path()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                apps.remove(index);
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": true }))
            }
            _ => wiremock::ResponseTemplate::new(405),
        }
    }
}
