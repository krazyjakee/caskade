//! Test helpers: certificates made per test and a TLS front for wiremock,
//! which speaks plain HTTP.

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::{net::SocketAddr, sync::Arc};

pub struct Cert {
    pub der: CertificateDer<'static>,
    pub pem: String,
    key: Vec<u8>,
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
