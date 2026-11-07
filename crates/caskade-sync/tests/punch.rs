//! A way through two routers for game files (#17). The routers themselves
//! are in `scripts/two-networks.sh`: here, on loopback with a local relay,
//! the pieces that way is made of.

use std::sync::Arc;

use caskade_sync::copy::{CopyGate, CopyStore, DirectDialer};
use caskade_sync::punch::{FileServer, OpenPort, open_port};
use caskade_sync::testing::local_relay;
use caskade_sync::{Identity, MemoryKeyStore, Relays};

async fn identity() -> Identity {
    match Identity::load(Arc::new(MemoryKeyStore::default()), None).await {
        caskade_sync::Loaded::Ready { identity, .. } => identity,
        _ => panic!("a new key is ready"),
    }
}

/// On loopback a relay sees 127.0.0.1, which isn't a public address: a port
/// of the test's own, as `open_port` opens one on a real network.
fn port() -> OpenPort {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    OpenPort {
        port: addr.port(),
        public: vec![addr],
    }
}

#[tokio::test]
async fn no_relay_to_ask_opens_no_port() {
    assert_eq!(open_port(&Relays::Never).await, None);
}

/// Nothing public seen, as behind a relay that only reaches this machine.
#[tokio::test(flavor = "multi_thread")]
async fn a_relay_that_sees_only_a_private_address_opens_no_port() {
    let (relays, _relay) = local_relay().await;
    assert_eq!(open_port(&relays).await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_arrives_on_ports_of_the_transfers_own() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Celeste");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("game.pak"), vec![9u8; 200_000]).unwrap();
    let offering = CopyStore::open(&temp.path().join("send")).await.unwrap();
    let gate = CopyGate::default();
    let offered = offering
        .offer(&folder, "copy/test", |_, _| {})
        .await
        .unwrap();

    let (desk, friend) = (identity().await, identity().await);
    let (near, far) = (port(), port());
    let (dialer, addrs) = DirectDialer::bind_on(&friend, &near).await.unwrap();
    assert!(addrs.contains(&near.public[0]), "{addrs:?}");
    let server = FileServer::bind(&desk, &far, offering.protocol(&gate), friend.id(), &addrs)
        .await
        .unwrap();
    assert!(server.addrs().contains(&far.public[0]));

    // The gate still decides: refused until allowed.
    let receiving = CopyStore::open(&temp.path().join("receive")).await.unwrap();
    let refused = dialer.connect(desk.id(), &server.addrs()).await.unwrap();
    assert!(
        receiving
            .fetch(refused, &offered.root, "copy/in", |_| {})
            .await
            .is_err(),
        "someone nobody allowed got the files"
    );
    gate.allow(dialer.id(), &offered.root).unwrap();
    let connection = dialer.connect(desk.id(), &server.addrs()).await.unwrap();
    receiving
        .fetch(connection, &offered.root, "copy/in", |_| {})
        .await
        .unwrap();
    let target = temp.path().join("installed");
    receiving.export(&offered.root, &target).await.unwrap();
    assert_eq!(
        std::fs::read(target.join("game.pak")).unwrap(),
        std::fs::read(folder.join("game.pak")).unwrap()
    );
    server.close().await;
    dialer.close().await;

    // The store outlives the port: the next transfer is served from it.
    let (near, far) = (port(), port());
    let (dialer, addrs) = DirectDialer::bind_on(&friend, &near).await.unwrap();
    let server = FileServer::bind(&desk, &far, offering.protocol(&gate), friend.id(), &addrs)
        .await
        .unwrap();
    gate.allow(dialer.id(), &offered.root).unwrap();
    let again = CopyStore::open(&temp.path().join("again")).await.unwrap();
    let connection = dialer.connect(desk.id(), &server.addrs()).await.unwrap();
    again
        .fetch(connection, &offered.root, "copy/again", |_| {})
        .await
        .unwrap();
    server.close().await;
    dialer.close().await;
}
