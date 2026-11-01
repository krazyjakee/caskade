use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use caskade_installer::{
    exec::{CancellationToken, ExecError, with_root_exclusive},
    runtime::alias::AliasStore,
};

#[tokio::test]
async fn aborted_maintenance_retains_the_install_root_lease_until_the_worker_exits() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let aliases = AliasStore::new(home.path().join("aliases"));
    let alias = aliases.link("same-root", &root).unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (cancelled, observed) = tokio::sync::oneshot::channel();
    let (release, wait) = mpsc::channel();
    let first = tokio::spawn(async move {
        with_root_exclusive(&root, CancellationToken::new(), move |_, token| {
            started.send(()).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !token.is_cancelled() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "caller drop did not cancel worker"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            cancelled.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            Err::<(), _>(ExecError::Cancelled)
        })
        .await
    });
    ready.await.unwrap();
    let (entered, mut entry) = tokio::sync::oneshot::channel();
    let second = tokio::spawn(async move {
        with_root_exclusive(&alias, CancellationToken::new(), move |_, _| {
            entered.send(()).unwrap();
            Ok::<_, ExecError>(())
        })
        .await
    });
    first.abort();
    observed.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut entry)
            .await
            .is_err(),
        "second operation entered while aborted caller's writer was still alive"
    );
    release.send(()).unwrap();
    entry.await.unwrap();
    second.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancelled_queue_wait_does_not_start_a_maintenance_writer() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = mpsc::channel();
    let first = tokio::spawn(async move {
        with_root_exclusive(&path, CancellationToken::new(), move |_, _| {
            started.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok::<_, ExecError>(())
        })
        .await
    });
    ready.await.unwrap();
    let token = CancellationToken::new();
    let called = Arc::new(AtomicBool::new(false));
    let flag = called.clone();
    let path = root.path().to_path_buf();
    let cancel = token.clone();
    let waiting = tokio::spawn(async move {
        with_root_exclusive(&path, cancel, move |_, _| {
            flag.store(true, Ordering::SeqCst);
            Ok::<_, ExecError>(())
        })
        .await
    });
    token.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(ExecError::Cancelled)));
    assert!(!called.load(Ordering::SeqCst));
    release.send(()).unwrap();
    first.await.unwrap().unwrap();
}
