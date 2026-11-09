//! A library a newer Caskade migrated opens when its newer migrations are
//! all compatible, and says it's newer otherwise.
use super::{FIRST_CLASSIFIED, INCOMPATIBLE, classification, needed, ups};
use crate::db::LibraryStore;
use crate::error::{ErrorReport, RecoveryHint};
use sqlx::{AssertSqlSafe, Connection, SqliteConnection};
use std::path::Path;

const COMPATIBLE: &str = "-- older Caskade: compatible";

/// A migration this Caskade doesn't have.
const FUTURE: i64 = 9_000_000;

/// `library` as a newer Caskade would leave it: `FUTURE` applied, and
/// `marker` as the newest incompatible migration.
async fn migrated_by_newer(library: &Path, marker: i64) {
    LibraryStore::open(library).await.unwrap().close().await;
    let mut connection = SqliteConnection::connect(&format!("sqlite://{}", library.display()))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time)
         VALUES (?, 'from a newer Caskade', 1, x'00', 0)",
    )
    .bind(FUTURE)
    .execute(&mut connection)
    .await
    .unwrap();
    sqlx::query(AssertSqlSafe(format!("PRAGMA user_version = {marker}")))
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
}

async fn marker(store: &LibraryStore) -> i64 {
    sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&store.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn compatible_newer_migrations_open_and_keep_the_marker() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library.sqlite");
    // An incompatible migration this Caskade has.
    migrated_by_newer(&library, 1).await;
    let store = LibraryStore::open(&library).await.unwrap();
    store.list_games(&Default::default()).await.unwrap();
    assert_eq!(marker(&store).await, 1, "the newer Caskade's marker stays");
}

#[tokio::test]
async fn without_a_marker_newer_migrations_are_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library.sqlite");
    migrated_by_newer(&library, 0).await;
    LibraryStore::open(&library).await.unwrap();
}

#[tokio::test]
async fn an_unknown_incompatible_migration_says_the_library_is_newer() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library.sqlite");
    migrated_by_newer(&library, FUTURE).await;
    let error = LibraryStore::open(&library).await.unwrap_err();
    assert!(
        matches!(
            error,
            crate::Error::Migration(sqlx::migrate::MigrateError::VersionMissing(FUTURE))
        ),
        "{error:?}"
    );
    let report = ErrorReport::from(&error);
    assert!(
        report.hints.contains(&RecoveryHint::UpdateCaskade),
        "{report:?}"
    );
}

#[tokio::test]
async fn an_up_to_date_library_records_what_older_caskades_need() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library.sqlite");
    let store = LibraryStore::open(&library).await.unwrap();
    assert_eq!(marker(&store).await, needed());
}

#[test]
fn new_migrations_say_whether_older_caskades_can_open_the_library() {
    let unclassified: Vec<String> = ups()
        .filter(|m| m.version >= FIRST_CLASSIFIED)
        .filter(|m| ![COMPATIBLE, INCOMPATIBLE].contains(&classification(m)))
        .map(|m| format!("{} ({})", m.version, m.description))
        .collect();
    assert!(
        unclassified.is_empty(),
        "start each migration with `{COMPATIBLE}` or `{INCOMPATIBLE}` \
         (see src/db/compat.rs): {unclassified:?}"
    );
}
