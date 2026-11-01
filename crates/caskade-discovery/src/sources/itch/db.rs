//! Read-only table snapshots. Optional columns/tables tolerate old butler builds.
use crate::{
    SourceError,
    parsers::sqlite::{Database, DatabaseLimits, SqlValue},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub(super) struct Record(pub BTreeMap<String, SqlValue>);
impl Record {
    pub fn text(&self, key: &str) -> Option<String> {
        match self.0.get(key)? {
            SqlValue::Text(v) if !v.is_empty() => Some(v.clone()),
            SqlValue::Integer(n) => Some(n.to_string()),
            _ => None,
        }
    }
    pub fn number(&self, key: &str) -> Option<u64> {
        self.text(key)?.parse().ok()
    }
    pub fn flag(&self, key: &str) -> bool {
        self.number(key) == Some(1)
    }
    pub fn platform(&self, key: &str) -> bool {
        self.flag(&format!("p_{key}"))
            || self
                .text(key)
                .is_some_and(|v| !matches!(v.as_str(), "0" | "false"))
    }
}
pub(super) struct Tables {
    pub caves: Vec<Record>,
    pub games: BTreeMap<String, Record>,
    pub uploads: BTreeMap<String, Record>,
    pub locations: BTreeMap<String, Record>,
}
pub(super) fn read(path: &Path) -> Result<Tables, SourceError> {
    let mut db = Database::open(
        path,
        DatabaseLimits {
            max_rows: 10_000,
            ..DatabaseLimits::default()
        },
    )
    .map_err(|e| SourceError::parse(path, e))?;
    let names = db
        .query("SELECT name FROM sqlite_master WHERE type='table'", &[])
        .map_err(|e| SourceError::parse(path, e))?;
    let names: BTreeSet<_> = names
        .rows
        .iter()
        .filter_map(|r| match r.first() {
            Some(SqlValue::Text(s)) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    if !names.contains("caves") {
        return Err(SourceError::parse(path, "missing caves table"));
    }
    let mut rows = |name: &str| -> Result<Vec<Record>, SourceError> {
        if !names.contains(name) {
            return Ok(vec![]);
        }
        let result = db
            .query(&format!("SELECT * FROM {name} ORDER BY id"), &[])
            .map_err(|e| SourceError::parse(path, e))?;
        Ok(result
            .rows
            .into_iter()
            .map(|r| {
                Record(
                    result
                        .columns
                        .iter()
                        .map(|c| c.to_ascii_lowercase())
                        .zip(r)
                        .collect(),
                )
            })
            .collect())
    };
    fn keyed(rows: Vec<Record>) -> BTreeMap<String, Record> {
        rows.into_iter()
            .filter_map(|r| Some((r.text("id")?, r)))
            .collect()
    }
    Ok(Tables {
        caves: rows("caves")?,
        games: keyed(rows("games")?),
        uploads: keyed(rows("uploads")?),
        locations: keyed(rows("install_locations")?),
    })
}
