#![cfg(feature = "sqlite")]
use caskade_discovery::parsers::sqlite::{Database, DatabaseLimits, SqlValue};
use rusqlite::Connection;
use std::time::Duration;
fn open(path: &std::path::Path) -> Database {
    Database::open(path, DatabaseLimits::default()).unwrap()
}
#[test]
fn committed_sqlite_golden_and_source_is_untouched() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/formats/pga.db");
    let before = std::fs::read(&path).unwrap();
    let mut db = open(&path);
    let result = db
        .query(
            "SELECT name, runner, directory FROM games WHERE installed=? ORDER BY id",
            &[1i64.into()],
        )
        .unwrap();
    insta::assert_json_snapshot!("sqlite", result);
    assert!(db.query("DELETE FROM games", &[]).is_err());
    assert!(
        db.query("ATTACH DATABASE ':memory:' AS other", &[])
            .is_err()
    );
    assert!(db.query("PRAGMA query_only=OFF", &[]).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!path.with_extension("db-wal").exists());
}
/// Characters SQLite's URI form would misread. Windows file names can't hold
/// `?`; `%` exercises the same escaping there.
const URI_NAME: &str = if cfg!(windows) {
    "test %# café.db"
} else {
    "test ?# café.db"
};
#[test]
fn writer_open_wal_commits_are_visible_without_source_side_effects() {
    let temp = crate::fixtures::tempdir();
    let path = temp.path().join(URI_NAME);
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE games(name); INSERT INTO games VALUES('committed'); BEGIN IMMEDIATE; INSERT INTO games VALUES('uncommitted');").unwrap();
    let names = [
        path.clone(),
        temp.path().join(format!("{URI_NAME}-wal")),
        temp.path().join(format!("{URI_NAME}-shm")),
    ];
    let before = names
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect::<Vec<_>>();
    let mut db = open(&path);
    assert!(db.is_snapshot());
    let result = db.query("SELECT name FROM games", &[]).unwrap();
    assert_eq!(result.rows, [vec![SqlValue::Text("committed".into())]]);
    for (p, bytes) in names.iter().zip(before) {
        assert_eq!(std::fs::read(p).unwrap(), bytes);
    }
    writer.execute_batch("ROLLBACK").unwrap();
}
#[test]
fn exclusive_writer_is_readable_through_private_rollback_recovery() {
    let temp = crate::fixtures::tempdir();
    let path = temp.path().join("exclusive.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE games(name); INSERT INTO games VALUES('before'); BEGIN EXCLUSIVE; UPDATE games SET name='after';").unwrap();
    let mut db = open(&path);
    assert_eq!(
        db.query("SELECT name FROM games", &[]).unwrap().rows,
        [vec![SqlValue::Text("before".into())]]
    );
    writer.execute_batch("ROLLBACK").unwrap();
}
#[test]
fn row_byte_and_vm_limits_stop_queries() {
    let temp = crate::fixtures::tempdir();
    let path = temp.path().join("limits.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE games(name); INSERT INTO games VALUES('first'),('second');")
        .unwrap();
    let mut db = Database::open(
        &path,
        DatabaseLimits {
            max_rows: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(db.query("SELECT * FROM games", &[]).is_err());
    let mut db = Database::open(
        &path,
        DatabaseLimits {
            max_result_bytes: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(db.query("SELECT * FROM games", &[]).is_err());
    let mut db = Database::open(
        &path,
        DatabaseLimits {
            max_vm_steps: 1000,
            timeout: Duration::from_millis(100),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        db.query(
            "WITH RECURSIVE r(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM r) SELECT sum(x) FROM r",
            &[]
        )
        .is_err()
    );
    assert!(db.query("SELECT load_extension('nope')", &[]).is_err());
}

// APFS and HFS+ refuse file names that aren't UTF-8 (EILSEQ), so no such
// file can exist on macOS.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn sqlite_non_utf8_filename_is_read_without_lossy_conversion() {
    use std::os::unix::ffi::OsStringExt;
    let temp = crate::fixtures::tempdir();
    let path = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"query-\xff.db".to_vec()));
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE game(id); INSERT INTO game VALUES(620);")
        .unwrap();
    let mut db = open(&path);
    assert_eq!(
        db.query("SELECT id FROM game", &[]).unwrap().rows,
        [vec![SqlValue::Integer(620)]]
    );
}
