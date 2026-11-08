//! Applied migrations never change: sqlx compares each file's checksum with the
//! one a library recorded when it ran it, and refuses to open the library on a
//! difference, even for an edited comment. `migration_checksums.txt` freezes them.

use super::MIGRATOR;
use std::collections::BTreeMap;

const FROZEN: &str = include_str!("migration_checksums.txt");

fn frozen() -> BTreeMap<i64, &'static str> {
    FROZEN
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (version, sum) = line.split_once(' ').expect("`<version> <sha384>`");
            (version.parse().expect("a version number"), sum)
        })
        .collect()
}

#[test]
fn applied_migrations_are_never_edited() {
    let frozen = frozen();
    let mut problems = Vec::new();
    let mut seen = Vec::new();
    for migration in MIGRATOR
        .iter()
        .filter(|m| !m.migration_type.is_down_migration())
    {
        let sum: String = migration
            .checksum
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        seen.push(migration.version);
        match frozen.get(&migration.version) {
            Some(expected) if *expected == sum => {}
            Some(_) => problems.push(format!(
                "migration {} ({}) was edited: libraries that applied it won't open. \
                 Restore the file and make the change in a new migration.",
                migration.version, migration.description
            )),
            None => problems.push(format!(
                "migration {} is new: add `{} {sum}` to src/db/migration_checksums.txt",
                migration.version, migration.version
            )),
        }
    }
    for version in frozen.keys().filter(|v| !seen.contains(v)) {
        problems.push(format!(
            "migration {version} is gone: libraries that applied it won't open"
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
