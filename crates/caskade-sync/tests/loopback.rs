//! Two endpoints in one process on loopback: the opening, admission and the
//! refusal (#224).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use caskade_sync::{
    ALPN, Admission, Config, EndpointId, Error, Grant, Identity, Loaded, MemoryKeyStore, Purpose,
    REFUSAL, Session, Sessions, SyncEndpoint,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Admits `peer` for sync when it presents `grant`.
struct Paired {
    peer: Mutex<Option<EndpointId>>,
    grant: Grant,
    asked: Mutex<Vec<(EndpointId, Purpose, bool)>>,
}

impl Paired {
    fn new(grant: Grant) -> Arc<Self> {
        Arc::new(Self {
            peer: Mutex::new(None),
            grant,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn pair(&self, peer: EndpointId) {
        *self.peer.lock().unwrap() = Some(peer);
    }
}

impl Admission for Paired {
    fn admits(&self, peer: EndpointId, purpose: Purpose, grant: Option<&Grant>) -> bool {
        self.asked
            .lock()
            .unwrap()
            .push((peer, purpose, grant.is_some()));
        *self.peer.lock().unwrap() == Some(peer)
            && purpose == Purpose::Sync
            && grant == Some(&self.grant)
    }
}

/// Admits nothing.
struct Nobody;

impl Admission for Nobody {
    fn admits(&self, _: EndpointId, _: Purpose, _: Option<&Grant>) -> bool {
        false
    }
}

/// Answers one ping on each accepted session, then reports it.
struct Echo(mpsc::UnboundedSender<(EndpointId, Purpose)>);

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Ping {
    n: u32,
}

#[async_trait]
impl Sessions for Echo {
    async fn opened(&self, mut session: Session) {
        let _ = self.0.send((session.peer(), session.purpose()));
        if let Ok(ping) = session.reader().read::<Ping>(64).await {
            let _ = session.writer().write(&Ping { n: ping.n + 1 }).await;
            let _ = session.connection().closed().await;
        }
    }
}

async fn bind(
    admission: Arc<dyn Admission>,
) -> (SyncEndpoint, mpsc::UnboundedReceiver<(EndpointId, Purpose)>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let endpoint = SyncEndpoint::bind(Config::loopback(), admission, Arc::new(Echo(tx)))
        .await
        .unwrap();
    (endpoint, rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn paired_devices_connect_and_exchange_the_opening() {
    let grant = Grant::generate();
    let policy = Paired::new(grant);
    let (server, mut opened) = bind(policy.clone()).await;
    let (client, _) = bind(Arc::new(Nobody)).await;
    policy.pair(client.id());

    let mut session = client
        .connect(server.addr(), Purpose::Sync, Some(grant))
        .await
        .unwrap();
    assert_eq!(session.peer(), server.id());
    assert_eq!(session.purpose(), Purpose::Sync);

    let (peer, purpose) = opened.recv().await.unwrap();
    assert_eq!(
        peer,
        client.id(),
        "the server saw the client's TLS identity"
    );
    assert_eq!(purpose, Purpose::Sync);

    session.writer().write(&Ping { n: 1 }).await.unwrap();
    let pong: Ping = session.reader().read(64).await.unwrap();
    assert_eq!(pong, Ping { n: 2 });

    session.close();
    client.shutdown().await.unwrap();
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_or_missing_grant_is_refused() {
    let grant = Grant::generate();
    let policy = Paired::new(grant);
    let (server, mut opened) = bind(policy.clone()).await;
    let (client, _) = bind(Arc::new(Nobody)).await;
    policy.pair(client.id());

    for presented in [None, Some(Grant::generate())] {
        let refused = client
            .connect(server.addr(), Purpose::Sync, presented)
            .await;
        assert!(matches!(refused, Err(Error::Refused)), "{refused:?}");
    }
    assert!(opened.try_recv().is_err(), "no session was opened");
    let asked = policy.asked.lock().unwrap().clone();
    assert_eq!(
        asked,
        vec![
            (client.id(), Purpose::Sync, false),
            (client.id(), Purpose::Sync, true)
        ]
    );
}

/// Dials with a hand-written opening line and returns every byte the server
/// wrote back, and the close reason.
async fn raw_refusal(
    client: &SyncEndpoint,
    server: &SyncEndpoint,
    line: &[u8],
) -> (Vec<u8>, String) {
    let connection = client
        .endpoint()
        .connect(server.addr(), ALPN)
        .await
        .unwrap();
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    send.write_all(line).await.unwrap();
    let bytes = tokio::time::timeout(Duration::from_secs(10), recv.read_to_end(4096))
        .await
        .expect("the server answered")
        .unwrap_or_default();
    let reason = format!("{:?}", connection.closed().await);
    (bytes, reason)
}

#[tokio::test(flavor = "multi_thread")]
async fn every_refusal_is_the_same_bytes() {
    let grant = Grant::generate();
    let policy = Paired::new(grant);
    let (server, _opened) = bind(policy.clone()).await;
    let (paired_client, _) = bind(Arc::new(Nobody)).await;
    let (stranger, _) = bind(Arc::new(Nobody)).await;
    policy.pair(paired_client.id());

    let right = format!(
        "{{\"purpose\":\"sync\",\"grant\":\"{}\"}}\n",
        grant.to_hex()
    );
    let wrong = format!(
        "{{\"purpose\":\"sync\",\"grant\":\"{}\"}}\n",
        Grant::generate().to_hex()
    );
    let other_purpose = format!(
        "{{\"purpose\":\"copy\",\"grant\":\"{}\"}}\n",
        grant.to_hex()
    );
    let cases: Vec<(&str, &SyncEndpoint, Vec<u8>)> = vec![
        (
            "missing grant",
            &paired_client,
            b"{\"purpose\":\"sync\"}\n".to_vec(),
        ),
        ("wrong grant", &paired_client, wrong.clone().into_bytes()),
        (
            "purpose not granted",
            &paired_client,
            other_purpose.into_bytes(),
        ),
        ("unknown device, right grant", &stranger, right.into_bytes()),
        (
            "unknown device, no grant",
            &stranger,
            b"{\"purpose\":\"pair\"}\n".to_vec(),
        ),
        (
            "unknown purpose",
            &paired_client,
            b"{\"purpose\":\"probe\"}\n".to_vec(),
        ),
        (
            "malformed grant",
            &paired_client,
            b"{\"purpose\":\"sync\",\"grant\":\"zz\"}\n".to_vec(),
        ),
        ("not JSON", &paired_client, b"hello\n".to_vec()),
        ("too long", &paired_client, vec![b'x'; 4096]),
    ];

    let mut seen = Vec::new();
    for (case, client, line) in cases {
        let (bytes, reason) = raw_refusal(client, &server, &line).await;
        assert_eq!(
            bytes,
            REFUSAL,
            "{case}: {}",
            String::from_utf8_lossy(&bytes)
        );
        seen.push((case, reason));
    }
    let first = &seen[0].1;
    for (case, reason) in &seen {
        assert_eq!(reason, first, "{case} closed differently");
    }

    // A server that holds nothing at all refuses with the same bytes.
    let (empty, _) = bind(Arc::new(Nobody)).await;
    let (bytes, reason) = raw_refusal(&paired_client, &empty, wrong.as_bytes()).await;
    assert_eq!(bytes, REFUSAL);
    assert_eq!(&reason, first);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_dialler_is_refused_after_the_timeout() {
    let config = Config {
        open_timeout: Duration::from_millis(300),
        ..Config::loopback()
    };
    let (tx, _rx) = mpsc::unbounded_channel();
    let server = SyncEndpoint::bind(config, Arc::new(Nobody), Arc::new(Echo(tx)))
        .await
        .unwrap();
    let (client, _) = bind(Arc::new(Nobody)).await;

    let connection = client
        .endpoint()
        .connect(server.addr(), ALPN)
        .await
        .unwrap();
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    // Half an opening, and then nothing.
    send.write_all(b"{\"purpose\":").await.unwrap();
    let bytes = tokio::time::timeout(Duration::from_secs(10), recv.read_to_end(4096))
        .await
        .expect("the server gave up on the dialler")
        .unwrap_or_default();
    assert_eq!(bytes, REFUSAL);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_endpoint_presents_the_stored_identity_across_restarts() {
    let store = Arc::new(MemoryKeyStore::default());
    let Loaded::Ready { identity, .. } = Identity::load(store.clone(), None).await else {
        panic!("first use makes an identity");
    };
    let id = identity.id();
    let config = Config {
        identity: Some(identity),
        ..Config::loopback()
    };
    let (tx, _rx) = mpsc::unbounded_channel();
    let first = SyncEndpoint::bind(config, Arc::new(Nobody), Arc::new(Echo(tx.clone())))
        .await
        .unwrap();
    assert_eq!(first.id(), id);
    first.shutdown().await.unwrap();

    // "Restart": read the identity back from the store.
    let Loaded::Ready { identity, created } = Identity::load(store, Some(id)).await else {
        panic!("the stored identity reads back");
    };
    assert!(!created);
    let config = Config {
        identity: Some(identity),
        ..Config::loopback()
    };
    let second = SyncEndpoint::bind(config, Arc::new(Nobody), Arc::new(Echo(tx)))
        .await
        .unwrap();
    assert_eq!(second.id(), id);
}

#[tokio::test(flavor = "multi_thread")]
async fn blobs_share_the_endpoint() {
    let store = iroh_blobs::store::mem::MemStore::new();
    let blobs = iroh_blobs::BlobsProtocol::new(&store, None);
    let (tx, _rx) = mpsc::unbounded_channel();
    let server = SyncEndpoint::bind(
        Config {
            blobs: Some(blobs),
            ..Config::loopback()
        },
        Arc::new(Nobody),
        Arc::new(Echo(tx)),
    )
    .await
    .unwrap();
    let (client, _) = bind(Arc::new(Nobody)).await;

    let tag = store.add_slice(b"caskade").await.unwrap();
    let downloaded = iroh_blobs::store::mem::MemStore::new();
    let connection = client
        .endpoint()
        .connect(server.addr(), iroh_blobs::ALPN)
        .await
        .expect("the server speaks iroh-blobs on the same endpoint");
    downloaded
        .remote()
        .fetch(connection, tag.hash)
        .await
        .unwrap();
    assert_eq!(
        downloaded.get_bytes(tag.hash).await.unwrap().as_ref(),
        b"caskade"
    );
}
