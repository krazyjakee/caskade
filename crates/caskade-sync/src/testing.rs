//! For tests (`test-utils`): a relay on this machine, so relay paths are
//! tested without n0's servers or the internet (#250).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use iroh::{RelayConfig, RelayMap, RelayUrl};
use iroh_relay::RelayQuicConfig;
use iroh_relay::server::{
    CertConfig, QuicConfig, RelayConfig as Relay, Server, ServerConfig, TlsConfig,
};

use crate::endpoint::Relays;

/// A local relay for [`crate::Reach::relays`]; it stops when the second
/// value is dropped.
pub async fn local_relay() -> (Relays, impl Sized) {
    let (map, _url, server) = iroh::test_utils::run_relay_server()
        .await
        .expect("a local relay starts");
    (Relays::Local(map), server)
}

/// A relay listening on `ip` (iroh's own, with QUIC address discovery, as
/// n0's are), for devices in other network namespaces. The text says where
/// it is, for [`relays_at`] in another process; the relay stops when the
/// second value is dropped.
pub async fn relay_on(ip: Ipv4Addr) -> (String, impl Sized) {
    let bind = SocketAddr::new(IpAddr::V4(ip), 0);
    let (_certs, server_config) = iroh_relay::server::testing::self_signed_tls_certs_and_config();
    let mut relay = Relay::new(bind);
    relay.tls = Some(TlsConfig::new(bind, CertConfig::Manual { server_config }));
    relay.key_cache_capacity = Some(1024);
    let mut config = ServerConfig::default();
    config.relay = Some(relay);
    config.quic = Some(QuicConfig::new(bind));
    let server = Server::spawn(config).await.expect("a relay starts");
    let https = server.https_addr().expect("configured");
    let quic = server.quic_addr().expect("configured").port();
    (format!("https://{https} {quic}"), server)
}

/// The relay [`relay_on`] described as `text`.
pub fn relays_at(text: &str) -> Relays {
    let (url, quic) = text.split_once(' ').expect("written by relay_on");
    let url: RelayUrl = url.parse().expect("written by relay_on");
    let quic = quic.parse().expect("written by relay_on");
    let config = RelayConfig::new(url, Some(RelayQuicConfig::new(quic)));
    Relays::Local(RelayMap::from_iter([config]))
}
