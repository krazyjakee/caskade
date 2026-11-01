//! Logical export into fresh pages: deleted records/WAL/free pages cannot leak.
use super::{
    DumpError, Result,
    scrub::{Scrubber, account, secret},
};
use crate::parsers::sqlite::{Database, DatabaseLimits, SqlValue};
use rusqlite::{Connection, types::Value};
use std::path::Path;
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
pub(super) fn export(source: &Path, destination: &Path, scrub: &Scrubber) -> Result<()> {
    let mut source = Database::open(
        source,
        DatabaseLimits {
            max_file_bytes: super::MAX_INPUT,
            max_rows: 10_000,
            max_result_bytes: super::MAX_TOTAL as usize,
            ..DatabaseLimits::default()
        },
    )
    .map_err(|e| DumpError::Invalid(e.to_string()))?;
    let tables = source
        .query(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
            &[],
        )
        .map_err(|e| DumpError::Invalid(e.to_string()))?;
    let mut output =
        Connection::open(destination).map_err(|e| DumpError::Invalid(e.to_string()))?;
    let transaction = output
        .transaction()
        .map_err(|e| DumpError::Invalid(e.to_string()))?;
    for row in tables.rows {
        let Some(SqlValue::Text(name)) = row.first() else {
            continue;
        };
        if ![
            "games",
            "caves",
            "uploads",
            "DbSet",
            "InstalledProducts",
            "Products",
        ]
        .iter()
        .any(|allowed| name.eq_ignore_ascii_case(allowed))
        {
            continue;
        }
        let data = source
            .query(&format!("SELECT * FROM {}", quote(name)), &[])
            .map_err(|e| DumpError::Invalid(e.to_string()))?;
        if data.columns.is_empty() {
            continue;
        }
        let columns = data
            .columns
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(",");
        transaction
            .execute_batch(&format!("CREATE TABLE {} ({columns})", quote(name)))
            .map_err(|e| DumpError::Invalid(e.to_string()))?;
        let sql = format!(
            "INSERT INTO {} VALUES ({})",
            quote(name),
            vec!["?"; data.columns.len()].join(",")
        );
        let mut statement = transaction
            .prepare(&sql)
            .map_err(|e| DumpError::Invalid(e.to_string()))?;
        for row in data.rows {
            let values: Vec<Value> = row
                .into_iter()
                .zip(&data.columns)
                .map(|(value, key)| {
                    if secret(key) || account(key) {
                        return Value::Text(scrub.field(key, "fixture"));
                    }
                    match value {
                        SqlValue::Null | SqlValue::Blob(_) => Value::Null,
                        SqlValue::Integer(n) => Value::Integer(n),
                        SqlValue::Real(n) => Value::Real(n),
                        SqlValue::Text(text) => Value::Text(scrub.text(&text)),
                    }
                })
                .collect();
            statement
                .execute(rusqlite::params_from_iter(&values))
                .map_err(|e| DumpError::Invalid(e.to_string()))?;
        }
    }
    transaction
        .commit()
        .map_err(|e| DumpError::Invalid(e.to_string()))?;
    output
        .execute_batch("VACUUM")
        .map_err(|e| DumpError::Invalid(e.to_string()))?;
    Ok(())
}
