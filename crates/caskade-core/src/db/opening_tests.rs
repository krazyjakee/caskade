//! Opening a database file has its own bound (#36): a slow first connection
//! is waited out as long as `Opening` allows, and queries keep sqlx's wait.
use std::time::Duration;

use super::{LibraryStore, Opening};

const SLOW: Duration = Duration::from_millis(300);

#[tokio::test]
async fn an_open_waits_out_a_slow_first_connection() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::open_after(
        &dir.path().join("library.sqlite"),
        Opening {
            wait: Duration::from_secs(30),
        },
        tokio::time::sleep(SLOW),
    )
    .await
    .unwrap();
    // Migrated, and its queries connect as they need to.
    store.list_games(&Default::default()).await.unwrap();
    // Each query still waits sqlx's default for a connection.
    assert_eq!(
        store.pool.options().get_acquire_timeout(),
        Duration::from_secs(30)
    );
}

#[tokio::test]
async fn an_open_slower_than_its_bound_fails() {
    let dir = tempfile::tempdir().unwrap();
    let error = LibraryStore::open_after(
        &dir.path().join("library.sqlite"),
        Opening {
            wait: Duration::from_millis(50),
        },
        tokio::time::sleep(SLOW),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, crate::Error::Db(sqlx::Error::PoolTimedOut)),
        "{error:?}"
    );
}

#[test]
fn the_library_waits_longer_than_a_query() {
    assert!(Opening::PATIENT.wait >= Duration::from_secs(180));
    assert_eq!(Opening::QUICK.wait, Duration::from_secs(30));
}
