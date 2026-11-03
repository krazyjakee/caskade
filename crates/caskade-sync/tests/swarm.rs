//! One collection from several providers at once (#249): a sharer and two
//! recipients that already have it serve a third, each within its own gate;
//! once every gate revokes it, nobody can fetch it.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use caskade_sync::copy::{CopyGate, CopyStore};
use caskade_sync::swarm::{Provider, SwarmDialer, fetch_from};
use caskade_sync::{
    Admission, Config, EndpointId, Grant, Purpose, Session, Sessions, SyncEndpoint,
};

struct Nobody;

impl Admission for Nobody {
    fn admits(&self, _: EndpointId, _: Purpose, _: Option<&Grant>) -> bool {
        false
    }
}

struct Ignore;

#[async_trait]
impl Sessions for Ignore {
    async fn opened(&self, session: Session) {
        session.end().await;
    }
}

/// A game of `files` files, so a split fetch has requests to spread.
fn game(dir: &Path, files: usize) {
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("Game.exe"), b"MZ fixture program").unwrap();
    for n in 0..files {
        let bytes: Vec<u8> = (0..256 * 1024).map(|i| ((i + n * 7) % 251) as u8).collect();
        std::fs::write(dir.join(format!("data/{n:02}.pak")), bytes).unwrap();
    }
}

/// A store, its gate and an endpoint serving it.
struct Peer {
    store: CopyStore,
    gate: CopyGate,
    endpoint: SyncEndpoint,
}

impl Peer {
    async fn new(dir: &Path) -> Self {
        let store = CopyStore::open(dir).await.unwrap();
        let gate = CopyGate::default();
        let config = Config {
            blobs: Some(store.protocol(&gate)),
            ..Config::loopback()
        };
        let endpoint = SyncEndpoint::bind(config, Arc::new(Nobody), Arc::new(Ignore))
            .await
            .unwrap();
        Self {
            store,
            gate,
            endpoint,
        }
    }

    fn provider(&self) -> Provider {
        let addrs: Vec<SocketAddr> = self.endpoint.addr().ip_addrs().copied().collect();
        (self.endpoint.id(), addrs)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_third_recipient_fetches_from_the_sharer_and_the_first_two_at_once() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Celeste");
    game(&folder, 30);
    let sharer = Peer::new(&temp.path().join("sharer")).await;
    let offered = sharer
        .store
        .offer(&folder, "share/s1", |_, _| {})
        .await
        .unwrap();
    let first = Peer::new(&temp.path().join("first")).await;
    let second = Peer::new(&temp.path().join("second")).await;
    let third = Peer::new(&temp.path().join("third")).await;

    // The first two fetch from the sharer alone.
    for recipient in [&first, &second] {
        let dialer = SwarmDialer::bind(None, true).await.unwrap();
        sharer.gate.allow(dialer.id(), &offered.root).unwrap();
        fetch_from(
            &recipient.store,
            &dialer,
            &[sharer.provider()],
            &offered.root,
            "share/s1",
            |_| {},
        )
        .await
        .unwrap();
        assert!(recipient.store.local_bytes(&offered.root).await.unwrap() >= offered.size);
        dialer.close().await;
    }

    // The third from all three, each letting it have this collection only.
    let dialer = SwarmDialer::bind(None, true).await.unwrap();
    for provider in [&sharer, &first, &second] {
        provider.gate.allow(dialer.id(), &offered.root).unwrap();
    }
    let providers = [sharer.provider(), first.provider(), second.provider()];
    fetch_from(
        &third.store,
        &dialer,
        &providers,
        &offered.root,
        "share/s1",
        |_| {},
    )
    .await
    .unwrap();
    let target = temp.path().join("out/Celeste");
    third.store.export(&offered.root, &target).await.unwrap();
    for n in 0..30 {
        let name = format!("data/{n:02}.pak");
        assert_eq!(
            std::fs::read(folder.join(&name)).unwrap(),
            std::fs::read(target.join(&name)).unwrap()
        );
    }
    for (name, provider) in [("sharer", &sharer), ("first", &first), ("second", &second)] {
        let sent = provider.gate.sent(dialer.id(), &offered.root);
        // Whole files, not just the collection's list of them.
        assert!(sent >= 256 * 1024, "the {name} sent the third {sent} bytes");
        assert!(sent < offered.size, "the {name} sent the third everything");
    }
    dialer.close().await;

    // Stopped everywhere: a fourth gets nothing from anyone.
    let fourth = Peer::new(&temp.path().join("fourth")).await;
    let dialer = SwarmDialer::bind(None, true).await.unwrap();
    let late = dialer.id();
    for provider in [&sharer, &first, &second, &third] {
        provider.gate.allow(late, &offered.root).unwrap();
        provider.gate.revoke(late, &offered.root);
    }
    let providers = [
        sharer.provider(),
        first.provider(),
        second.provider(),
        third.provider(),
    ];
    let fetched = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        fetch_from(
            &fourth.store,
            &dialer,
            &providers,
            &offered.root,
            "share/s1",
            |_| {},
        ),
    )
    .await
    .expect("a refused fetch ends");
    assert!(fetched.is_err());
    assert_eq!(fourth.store.local_bytes(&offered.root).await.unwrap(), 0);
}

#[tokio::test]
async fn providers_without_a_direct_address_are_not_asked() {
    let temp = tempfile::tempdir().unwrap();
    let store = CopyStore::open(&temp.path().join("store")).await.unwrap();
    let dialer = SwarmDialer::bind(None, true).await.unwrap();
    let nobody = iroh::SecretKey::generate().public();
    let root = "ab".repeat(32);
    let error = fetch_from(&store, &dialer, &[(nobody, Vec::new())], &root, "t", |_| {})
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no direct address"), "{error}");
}
