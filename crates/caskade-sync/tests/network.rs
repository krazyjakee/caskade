//! Reaching devices (#225): the local network first, relays only when
//! allowed, and the link state that follows a connection's paths.
//!
//! Relays here are a local relay server (iroh's test relay, self-signed),
//! so nothing reaches the internet. mDNS runs on this machine's real
//! interfaces under a service name unique to the test run; set
//! `CASKADE_TEST_NO_MULTICAST=1` where the platform has no multicast.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use caskade_sync::{
    Admission, Config, DeviceInfo, DeviceKind, DeviceOs, EndpointAddr, EndpointId, Error, Grant,
    Ip, Link, Purpose, Reach, Relays, Session, Sessions, SyncEndpoint,
};
use iroh::{RelayMap, TransportAddr};

/// Admits anyone for sync.
struct Anyone;

impl Admission for Anyone {
    fn admits(&self, _: EndpointId, purpose: Purpose, _: Option<&Grant>) -> bool {
        purpose == Purpose::Sync
    }
}

/// Keeps each session open until the peer closes it.
struct Hold;

#[async_trait]
impl Sessions for Hold {
    async fn opened(&self, session: Session) {
        session.connection().closed().await;
    }
}

fn reach(relays: Relays, ip: Ip) -> Reach {
    Reach {
        relays,
        ip,
        internet_lookup: false,
        lan: None,
    }
}

async fn bind(reach: Reach) -> SyncEndpoint {
    bind_named(reach, "Test").await
}

async fn bind_named(reach: Reach, name: &str) -> SyncEndpoint {
    let config = Config {
        reach,
        device: DeviceInfo::new(name, DeviceKind::Desktop),
        open_timeout: Duration::from_secs(5),
        ..Config::loopback()
    };
    SyncEndpoint::bind(config, Arc::new(Anyone), Arc::new(Hold))
        .await
        .unwrap()
}

/// The server's address with only its relay in it.
async fn relay_only_addr(server: &SyncEndpoint) -> EndpointAddr {
    tokio::time::timeout(Duration::from_secs(10), server.endpoint().online())
        .await
        .expect("the endpoint reached the local relay");
    let addr = server.addr();
    let relays: Vec<TransportAddr> = addr
        .relay_urls()
        .cloned()
        .map(TransportAddr::Relay)
        .collect();
    assert!(!relays.is_empty(), "{addr:?}");
    EndpointAddr::from_parts(server.id(), relays)
}

/// A local relay; it stops when the second value is dropped.
async fn relay() -> (RelayMap, impl Sized) {
    let (map, _url, server) = iroh::test_utils::run_relay_server().await.unwrap();
    (map, server)
}

#[tokio::test(flavor = "multi_thread")]
async fn never_use_relays_leaves_a_relay_only_device_offline() {
    let (map, _relay) = relay().await;
    // The server can only be reached through the relay.
    let server = bind(reach(Relays::Local(map), Ip::Off)).await;
    let addr = relay_only_addr(&server).await;
    // Never use relays.
    let client = bind(reach(Relays::Never, Ip::Loopback { port: None })).await;
    assert!(!client.relays_allowed());

    let mut links = client.links().subscribe();
    let result = client.connect(addr, Purpose::Sync, None).await;
    assert!(
        matches!(result, Err(Error::Dial { .. } | Error::TimedOut { .. })),
        "{result:?}"
    );
    assert_eq!(client.link(server.id()), Link::Offline);
    assert!(links.try_recv().is_err(), "no connection came up at all");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relay_only_device_is_reached_through_the_relay_when_allowed() {
    let (map, _relay) = relay().await;
    let server = bind(reach(Relays::Local(map.clone()), Ip::Off)).await;
    let addr = relay_only_addr(&server).await;
    let client = bind(reach(Relays::Local(map), Ip::Loopback { port: None })).await;

    let session = client.connect(addr, Purpose::Sync, None).await.unwrap();
    assert_eq!(client.link(server.id()), Link::Relay);
    assert!(
        session
            .connection()
            .paths()
            .iter()
            .all(|path| path.is_relay()),
        "the server has no IP transport"
    );

    session.close();
    let deadline = Instant::now() + Duration::from_secs(10);
    while client.link(server.id()) != Link::Offline {
        assert!(
            Instant::now() < deadline,
            "a closed connection goes offline"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relayed_connection_moves_to_a_direct_path_and_the_state_follows() {
    let (map, _relay) = relay().await;
    let server = bind(reach(
        Relays::Local(map.clone()),
        Ip::Loopback { port: None },
    ))
    .await;
    let addr = relay_only_addr(&server).await;
    let client = bind(reach(Relays::Local(map), Ip::Loopback { port: None })).await;
    let mut links = client.links().subscribe();

    // Dialled through the relay alone: the direct path has to be found.
    let session = client.connect(addr, Purpose::Sync, None).await.unwrap();
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while seen.last() != Some(&Link::Lan) {
        match tokio::time::timeout_at(deadline, links.recv()).await {
            Ok(Ok(change)) => {
                assert_eq!(change.peer, server.id());
                seen.push(change.link);
            }
            other => panic!("no direct path after {seen:?}: {other:?}"),
        }
    }
    assert_eq!(
        seen.first(),
        Some(&Link::Relay),
        "it started relayed: {seen:?}"
    );
    assert_eq!(client.link(server.id()), Link::Lan);
    assert!(
        session
            .connection()
            .paths()
            .iter()
            .any(|path| path.is_selected() && path.is_ip()),
        "the selected path is direct"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_typed_address_connects_without_local_discovery() {
    // No mDNS, no relay, no lookup: the address alone, as when guest Wi-Fi
    // or client isolation blocks multicast.
    let server = bind(Reach::loopback()).await;
    let client = bind(Reach::loopback()).await;
    let addr = server.shareable_addr(Duration::from_secs(2)).await;
    assert!(addr.ip_addrs().next().is_some());
    let session = client.connect(addr, Purpose::Sync, None).await.unwrap();
    assert_eq!(client.link(server.id()), Link::Lan);
    session.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shareable_address_does_not_wait_for_a_relay_that_never_comes() {
    // Relays are allowed, but the relay is gone: like a network with no
    // internet. The local addresses come back within the wait.
    let (map, relay) = relay().await;
    drop(relay);
    let endpoint = bind(reach(Relays::Local(map), Ip::Loopback { port: None })).await;
    let started = Instant::now();
    let addr = endpoint.shareable_addr(Duration::from_millis(500)).await;
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(addr.ip_addrs().next().is_some(), "{addr:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fixed_port_is_the_one_bound() {
    let port = std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let endpoint = bind(reach(Relays::Never, Ip::Loopback { port: Some(port) })).await;
    let bound: Vec<u16> = endpoint
        .endpoint()
        .bound_sockets()
        .iter()
        .map(|addr| addr.port())
        .collect();
    assert!(bound.contains(&port), "{bound:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn devices_on_the_local_network_find_each_other_with_no_internet() {
    if std::env::var_os("CASKADE_TEST_NO_MULTICAST").is_some() {
        eprintln!("skipped: CASKADE_TEST_NO_MULTICAST");
        return;
    }
    // No relays and no internet lookup: mDNS is the only way to find a
    // device. The service name is unique to this run.
    let service = format!("cskt{:08x}", rand_suffix());
    let lan = || Reach {
        relays: Relays::Never,
        ip: Ip::Any { port: None },
        internet_lookup: false,
        lan: Some(service.clone()),
    };
    let server = bind_named(lan(), "Living room").await;
    let client = bind_named(lan(), "Den").await;

    let deadline = Instant::now() + Duration::from_secs(20);
    let found = loop {
        if let Some(found) = client
            .nearby()
            .into_iter()
            .find(|nearby| nearby.id == server.id())
        {
            break found;
        }
        assert!(
            Instant::now() < deadline,
            "no mDNS answer within 20 s (set CASKADE_TEST_NO_MULTICAST=1 where multicast isn't available)"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(found.info.name, "Living room");
    assert_eq!(found.info.kind, DeviceKind::Desktop);
    assert_eq!(found.info.os, DeviceOs::current());
    assert!(!found.addrs.is_empty());

    // Dialled by id alone: the address comes from mDNS.
    let session = client
        .connect(EndpointAddr::new(server.id()), Purpose::Sync, None)
        .await
        .unwrap();
    assert_eq!(client.link(server.id()), Link::Lan);

    // A rename is announced again.
    server
        .lan()
        .unwrap()
        .announce(DeviceInfo::new("Lounge", DeviceKind::Tv));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let renamed = client
            .nearby()
            .into_iter()
            .any(|nearby| nearby.id == server.id() && nearby.info.name == "Lounge");
        if renamed {
            break;
        }
        assert!(Instant::now() < deadline, "the rename wasn't announced");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    session.close();
}

fn rand_suffix() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    nanos ^ std::process::id()
}
