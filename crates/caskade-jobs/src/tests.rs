use super::*;
use std::time::Duration;
use tokio::sync::oneshot;

async fn tasks(queue: &Queue, count: usize) {
    let mut updates = queue.subscribe();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if updates.borrow_and_update().tasks.len() == count {
                break;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .expect("queue did not reach the expected state");
}

#[tokio::test]
async fn fifo_queue_skips_cancelled_waiters_and_reports_idle() {
    let queue = Queue::default();
    let first = queue.acquire("First").await;
    let second = tokio::spawn({
        let queue = queue.clone();
        async move { queue.acquire("Cancelled").await }
    });
    tasks(&queue, 2).await;
    let third = tokio::spawn({
        let queue = queue.clone();
        async move { queue.acquire("Third").await }
    });
    tasks(&queue, 3).await;
    let fourth = tokio::spawn({
        let queue = queue.clone();
        async move { queue.acquire("Fourth").await }
    });
    tasks(&queue, 4).await;
    second.abort();
    let _ = second.await;
    drop(first);
    let third = third.await.unwrap();
    assert!(!fourth.is_finished());
    let snapshot = queue.snapshot();
    assert_eq!(snapshot.tasks.iter().filter(|t| !t.queued).count(), 1);
    assert_eq!(
        snapshot.tasks.iter().find(|t| !t.queued).unwrap().label,
        "Third"
    );
    drop(third);
    drop(fourth.await.unwrap());
    assert!(queue.snapshot().tasks.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborted_caller_keeps_slot_until_its_blocking_worker_stops() {
    let queue = Queue::default();
    let (started, running) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let caller = tokio::spawn({
        let queue = queue.clone();
        async move {
            queue
                .blocking("Slow worker", move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                })
                .await
        }
    });
    running.await.unwrap();
    caller.abort();
    let _ = caller.await;
    let next = tokio::spawn({
        let queue = queue.clone();
        async move { queue.acquire("Next").await }
    });
    tasks(&queue, 2).await;
    assert!(!next.is_finished());
    assert!(!queue.snapshot().tasks[0].queued);
    release.send(()).unwrap();
    drop(next.await.unwrap());
    assert!(queue.snapshot().tasks.is_empty());
}

#[tokio::test]
async fn nested_async_and_sync_stages_reuse_the_slot_and_panics_release_it() {
    let queue = Queue::default();
    let nested = queue.clone();
    let value = tokio::time::timeout(
        Duration::from_secs(2),
        queue.run("Parent", async {
            queue
                .blocking("Child", move || nested.sync("Nested", || 42))
                .await
                .unwrap()
        }),
    )
    .await
    .unwrap();
    assert_eq!(value, 42);
    assert!(queue.snapshot().tasks.is_empty());
    assert!(
        queue
            .blocking("Panic", || panic!("test worker failure"))
            .await
            .unwrap_err()
            .is_panic()
    );
    assert!(queue.snapshot().tasks.is_empty());
    assert_eq!(queue.blocking("Recovered", || 7).await.unwrap(), 7);
}

#[tokio::test]
async fn synchronous_watchers_share_the_async_fifo() {
    let queue = Queue::default();
    let first = queue.acquire("Async").await;
    let thread = std::thread::spawn({
        let queue = queue.clone();
        move || queue.sync("Watcher", || 42)
    });
    tasks(&queue, 2).await;
    assert!(queue.snapshot().tasks[1].queued);
    drop(first);
    assert_eq!(thread.join().unwrap(), 42);
    assert!(queue.snapshot().tasks.is_empty());
}

#[tokio::test]
async fn child_source_threads_retain_their_parents_resource_slot() {
    let queue = global();
    let (release, wait) = std::sync::mpsc::channel();
    let child = queue
        .run("Source scan", async {
            std::thread::spawn(inherit(move || wait.recv().unwrap()))
        })
        .await;
    let next = tokio::spawn(async { global().acquire("Next scan").await });
    tasks(queue, 2).await;
    assert!(!next.is_finished());
    release.send(()).unwrap();
    child.join().unwrap();
    drop(next.await.unwrap());
    assert!(queue.snapshot().tasks.is_empty());
}

#[test]
fn relabelling_keeps_the_task_and_publishes_only_changes() {
    let queue = Queue::default();
    let activity = queue.activity("Matching");
    let before = queue.snapshot();
    activity.set_label("Matching · 1/2");
    let after = queue.snapshot();
    assert_eq!(after.tasks.len(), 1);
    assert_eq!(after.tasks[0].id, before.tasks[0].id);
    assert_eq!(after.tasks[0].label, "Matching · 1/2");
    activity.set_label("Matching · 1/2");
    assert_eq!(queue.snapshot().revision, after.revision);
}
