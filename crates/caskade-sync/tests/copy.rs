//! Game folders between two endpoints on loopback (#233): offered,
//! fetched over the direct dialer, exported byte for byte; refused to
//! anyone not allowed; resumed after an interruption without starting over.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use caskade_sync::copy::{CopyGate, CopyStore, DirectDialer, place};
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

/// A game folder: a program, nested data and an empty file.
fn game(dir: &Path, big: usize) {
    std::fs::create_dir_all(dir.join("data/levels")).unwrap();
    std::fs::write(dir.join("Game.exe"), b"MZ fixture program").unwrap();
    std::fs::write(dir.join("data/levels/one.dat"), vec![7u8; 40_000]).unwrap();
    let pattern: Vec<u8> = (0..big).map(|i| (i * 31 % 251) as u8).collect();
    std::fs::write(dir.join("data/big.pak"), pattern).unwrap();
    std::fs::write(dir.join("empty.txt"), b"").unwrap();
}

/// A sending endpoint serving `store` through `gate`.
async fn sender(store: &CopyStore, gate: &CopyGate) -> SyncEndpoint {
    let config = Config {
        blobs: Some(store.protocol(gate)),
        ..Config::loopback()
    };
    SyncEndpoint::bind(config, Arc::new(Nobody), Arc::new(Ignore))
        .await
        .unwrap()
}

fn direct(endpoint: &SyncEndpoint) -> Vec<SocketAddr> {
    endpoint.addr().ip_addrs().copied().collect()
}

fn same_tree(a: &Path, b: &Path) {
    for name in [
        "Game.exe",
        "data/levels/one.dat",
        "data/big.pak",
        "empty.txt",
    ] {
        assert_eq!(
            std::fs::read(a.join(name)).unwrap(),
            std::fs::read(b.join(name)).unwrap(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn a_folder_arrives_whole_and_only_for_who_was_allowed() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Celeste");
    game(&folder, 3 * 1024 * 1024);
    let offering = CopyStore::open(&temp.path().join("send")).await.unwrap();
    let gate = CopyGate::default();
    let endpoint = sender(&offering, &gate).await;
    let offered = offering
        .offer(&folder, "copy/test", |_, _| {})
        .await
        .unwrap();
    assert_eq!(offered.files, 4);
    assert_eq!(offered.size, 18 + 40_000 + 3 * 1024 * 1024);

    let receiving = CopyStore::open(&temp.path().join("receive")).await.unwrap();
    let dialer = DirectDialer::bind(None, true).await.unwrap();
    // Not allowed yet: refused.
    let connection = dialer.connect(endpoint.id(), &direct(&endpoint)).await;
    if let Ok(connection) = connection {
        let refused = receiving
            .fetch(connection, &offered.root, "copy/in", |_| {})
            .await;
        assert!(refused.is_err(), "a device nobody allowed got the files");
    }

    gate.allow(dialer.id(), &offered.root).unwrap();
    let connection = dialer
        .connect(endpoint.id(), &direct(&endpoint))
        .await
        .unwrap();
    let started = Instant::now();
    receiving
        .fetch(connection, &offered.root, "copy/in", |_| {})
        .await
        .unwrap();
    let elapsed = started.elapsed();
    let target = temp.path().join("installed");
    receiving.export(&offered.root, &target).await.unwrap();
    same_tree(&folder, &target);
    assert!(receiving.local_bytes(&offered.root).await.unwrap() >= offered.size);
    eprintln!(
        "loopback copy: {} bytes in {:?} ({:.0} MB/s)",
        offered.size,
        elapsed,
        offered.size as f64 / elapsed.as_secs_f64() / 1e6
    );
    // The sender counted what it sent.
    assert!(gate.sent(dialer.id(), &offered.root) >= 3 * 1024 * 1024);
}

#[tokio::test]
async fn an_interrupted_copy_resumes_without_starting_over() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Hades");
    let big = 48 * 1024 * 1024;
    game(&folder, big);
    let offering = CopyStore::open(&temp.path().join("send")).await.unwrap();
    let gate = CopyGate::default();
    let endpoint = sender(&offering, &gate).await;
    let offered = offering
        .offer(&folder, "copy/test", |_, _| {})
        .await
        .unwrap();
    let receiving = CopyStore::open(&temp.path().join("receive")).await.unwrap();
    let dialer = DirectDialer::bind(None, true).await.unwrap();
    gate.allow(dialer.id(), &offered.root).unwrap();

    // The network goes away part-way: the connection is closed under it.
    let connection = dialer
        .connect(endpoint.id(), &direct(&endpoint))
        .await
        .unwrap();
    let closer = connection.clone();
    let (seen, mut first) = tokio::sync::watch::channel(0u64);
    let fetch = {
        let receiving = receiving.clone();
        let root = offered.root.clone();
        tokio::spawn(async move {
            receiving
                .fetch(connection, &root, "copy/in", move |bytes| {
                    let _ = seen.send(bytes);
                })
                .await
        })
    };
    tokio::time::timeout(
        Duration::from_secs(30),
        first.wait_for(|bytes| *bytes > 4 * 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    closer.close(0u32.into(), b"unplugged");
    assert!(fetch.await.unwrap().is_err());
    let kept = receiving.local_bytes(&offered.root).await.unwrap();
    assert!(kept > 0, "nothing of the first try was kept");
    assert!(
        kept < offered.size,
        "the first try finished before it was cut"
    );

    // Back: only what's missing comes.
    let connection = dialer
        .connect(endpoint.id(), &direct(&endpoint))
        .await
        .unwrap();
    let mut again = 0;
    receiving
        .fetch(connection, &offered.root, "copy/in", |bytes| again = bytes)
        .await
        .unwrap();
    assert!(
        again <= offered.size - kept + 1024 * 1024,
        "the second try fetched {again} of {} with {kept} kept",
        offered.size
    );
    let target = temp.path().join("installed");
    receiving.export(&offered.root, &target).await.unwrap();
    same_tree(&folder, &target);
}

#[tokio::test]
async fn revoking_stops_a_transfer_part_way() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Hades");
    let big = 48 * 1024 * 1024;
    game(&folder, big);
    let offering = CopyStore::open(&temp.path().join("send")).await.unwrap();
    let gate = CopyGate::default();
    let endpoint = sender(&offering, &gate).await;
    let offered = offering
        .offer(&folder, "copy/test", |_, _| {})
        .await
        .unwrap();
    let receiving = CopyStore::open(&temp.path().join("receive")).await.unwrap();
    let dialer = DirectDialer::bind(None, true).await.unwrap();
    gate.allow(dialer.id(), &offered.root).unwrap();
    let connection = dialer
        .connect(endpoint.id(), &direct(&endpoint))
        .await
        .unwrap();
    let (seen, mut first) = tokio::sync::watch::channel(0u64);
    let fetch = {
        let receiving = receiving.clone();
        let root = offered.root.clone();
        tokio::spawn(async move {
            receiving
                .fetch(connection, &root, "copy/in", move |bytes| {
                    let _ = seen.send(bytes);
                })
                .await
        })
    };
    tokio::time::timeout(
        Duration::from_secs(30),
        first.wait_for(|bytes| *bytes > 2 * 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    // Stopped: the transfer in flight ends, not just new ones.
    gate.revoke(dialer.id(), &offered.root);
    let stopped = tokio::time::timeout(Duration::from_secs(10), fetch)
        .await
        .expect("the transfer went on after the revoke")
        .unwrap();
    assert!(stopped.is_err());
    assert!(receiving.local_bytes(&offered.root).await.unwrap() < offered.size);
    // And it can't start again.
    let connection = dialer.connect(endpoint.id(), &direct(&endpoint)).await;
    if let Ok(connection) = connection {
        assert!(
            receiving
                .fetch(connection, &offered.root, "copy/in", |_| {})
                .await
                .is_err()
        );
    }
}

#[test]
fn names_never_leave_the_target() {
    let target = Path::new("/games/incoming");
    assert_eq!(
        place(target, "data/levels/one.dat").unwrap(),
        target.join("data/levels/one.dat")
    );
    for bad in [
        "",
        "../escape",
        "data/../../escape",
        "/etc/passwd",
        "C:/x",
        "a\\b",
        "a//b",
        "./a",
    ] {
        assert!(place(target, bad).is_none(), "{bad}");
    }
}

/// A large copy on loopback, measured: `cargo test --release -p caskade-sync
/// --test it copy::loopback_throughput -- --ignored --nocapture`. The figure
/// goes in docs/sync.md.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "moves 256 MiB; run by hand, in release"]
async fn loopback_throughput() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Big");
    game(&folder, 256 * 1024 * 1024);
    let offering = CopyStore::open(&temp.path().join("send")).await.unwrap();
    let gate = CopyGate::default();
    let endpoint = sender(&offering, &gate).await;
    let prepared = Instant::now();
    let offered = offering
        .offer(&folder, "copy/test", |_, _| {})
        .await
        .unwrap();
    let preparing = prepared.elapsed();
    let receiving = CopyStore::open(&temp.path().join("receive")).await.unwrap();
    let dialer = DirectDialer::bind(None, true).await.unwrap();
    gate.allow(dialer.id(), &offered.root).unwrap();
    let connection = dialer
        .connect(endpoint.id(), &direct(&endpoint))
        .await
        .unwrap();
    let started = Instant::now();
    receiving
        .fetch(connection, &offered.root, "copy/in", |_| {})
        .await
        .unwrap();
    let elapsed = started.elapsed();
    eprintln!(
        "offer (BLAKE3 outboards): {preparing:?}; transfer: {} MiB in {elapsed:?} = {:.0} MB/s",
        offered.size / (1024 * 1024),
        offered.size as f64 / elapsed.as_secs_f64() / 1e6
    );
}
