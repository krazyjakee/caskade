#![no_main]
use caskade_discovery::parsers::sqlite::{Database, DatabaseLimits};
use libfuzzer_sys::fuzz_target;
use std::time::Duration;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fuzz.db");
    std::fs::write(&path, data).unwrap();
    let limits = DatabaseLimits {
        max_file_bytes: 65536,
        max_rows: 32,
        max_value_bytes: 4096,
        max_result_bytes: 65536,
        max_vm_steps: 10000,
        timeout: Duration::from_millis(10),
    };
    if let Ok(mut db) = Database::open(&path, limits) {
        let _ = db.query("SELECT name, type FROM sqlite_schema LIMIT 32", &[]);
    }
});
