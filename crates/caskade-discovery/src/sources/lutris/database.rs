use crate::{
    SourceError,
    parsers::sqlite::{Database, DatabaseLimits, SqlValue},
};
use std::{collections::BTreeMap, path::Path};
pub(super) struct Record(pub BTreeMap<String, SqlValue>);
impl Record {
    pub fn text(&self, key: &str) -> Option<String> {
        match self.0.get(key)? {
            SqlValue::Text(value) if !value.trim().is_empty() => Some(value.clone()),
            SqlValue::Integer(value) => Some(value.to_string()),
            _ => None,
        }
    }
    pub fn number(&self, key: &str) -> Option<f64> {
        match self.0.get(key)? {
            SqlValue::Integer(n) => Some(*n as f64),
            SqlValue::Real(n) => Some(*n),
            SqlValue::Text(n) => n.parse().ok(),
            _ => None,
        }
        .filter(|n| n.is_finite() && *n >= 0.)
    }
}
pub(super) fn records(path: &Path) -> Result<Vec<Record>, SourceError> {
    let mut db = Database::open(
        path,
        DatabaseLimits {
            max_rows: 10_000,
            ..DatabaseLimits::default()
        },
    )
    .map_err(|e| SourceError::parse(path, e))?;
    // SELECT * tolerates older databases that lack newer optional columns.
    let result = db
        .query("SELECT * FROM games WHERE installed=1 ORDER BY id", &[])
        .map_err(|e| SourceError::parse(path, e))?;
    Ok(result
        .rows
        .into_iter()
        .map(|row| {
            Record(
                result
                    .columns
                    .iter()
                    .map(|c| c.to_ascii_lowercase())
                    .zip(row)
                    .collect(),
            )
        })
        .collect())
}
