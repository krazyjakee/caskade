//! Bounded SQLite reads. Sources are never opened with write/create permissions.
//! A live WAL (or rollback journal) is copied with the database into a private
//! directory before SQLite touches it. Rebuilt SHM/recovery writes stay there.
use super::{ParseError, Result};
use rusqlite::{
    Connection, OpenFlags,
    hooks::{AuthAction, AuthContext, Authorization},
    limits::Limit,
    types::{Value, ValueRef},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Debug)]
pub struct DatabaseLimits {
    pub max_file_bytes: u64,
    pub max_rows: usize,
    pub max_value_bytes: usize,
    pub max_result_bytes: usize,
    pub max_vm_steps: u64,
    pub timeout: Duration,
}
impl Default for DatabaseLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 256 * 1024 * 1024,
            max_rows: 100_000,
            max_value_bytes: 1024 * 1024,
            max_result_bytes: 32 * 1024 * 1024,
            max_vm_steps: 5_000_000,
            timeout: Duration::from_secs(2),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
}
pub struct Database {
    connection: Connection,
    // Keep the directory alive until the connection and its private WAL/SHM close.
    _snapshot: Option<tempfile::TempDir>,
    limits: DatabaseLimits,
}
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
fn stamp(path: &Path) -> Result<Option<(u64, Option<SystemTime>)>> {
    match fs::metadata(path) {
        Ok(m) => Ok(Some((m.len(), m.modified().ok()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn copy_bounded(source: &Path, destination: &Path, maximum: u64) -> Result<()> {
    let file = fs::File::open(source)?;
    if file.metadata()?.len() > maximum {
        return Err(ParseError::Limit {
            budget: "database file bytes",
            maximum: maximum.min(usize::MAX as u64) as usize,
        });
    }
    let mut reader = file.take(maximum.saturating_add(1));
    let mut output = fs::File::create(destination)?;
    let n = std::io::copy(&mut reader, &mut output)?;
    if n > maximum {
        return Err(ParseError::Limit {
            budget: "database file bytes",
            maximum: maximum.min(usize::MAX as u64) as usize,
        });
    }
    output.flush()?;
    Ok(())
}
fn uri(path: &Path) -> Result<String> {
    #[cfg(unix)]
    let path = path.as_os_str().as_encoded_bytes();
    #[cfg(not(unix))]
    let path = path
        .to_str()
        .ok_or_else(|| ParseError::Invalid("non-Unicode SQLite path".into()))?
        .as_bytes();
    // Encode URI metacharacters; keep drive separators usable on every host.
    let mut out = String::from("file:");
    for &byte in path {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out.push_str("?mode=ro&immutable=1");
    Ok(out)
}
impl Database {
    pub fn open(path: &Path, limits: DatabaseLimits) -> Result<Self> {
        let path = dunce::canonicalize(&path)?;
        let size = fs::metadata(&path)?.len();
        if size > limits.max_file_bytes {
            return Err(ParseError::Limit {
                budget: "database file bytes",
                maximum: limits.max_file_bytes.min(usize::MAX as u64) as usize,
            });
        }
        let sidecars = [sidecar(&path, "-wal"), sidecar(&path, "-journal")];
        if sidecars
            .iter()
            .map(|p| stamp(p))
            .collect::<Result<Vec<_>>>()?
            .iter()
            .any(Option::is_some)
        {
            return Self::snapshot(&path, limits);
        }
        let connection = Connection::open_with_flags(
            uri(&path)?,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        );
        match connection {
            Ok(connection) => Self::finish(connection, None, limits),
            Err(_) => Self::snapshot(&path, limits),
        }
    }
    fn snapshot(path: &Path, limits: DatabaseLimits) -> Result<Self> {
        for _ in 0..3 {
            let dir = tempfile::tempdir()?;
            let target = dir.path().join("snapshot.db");
            let sources = [
                path.to_owned(),
                sidecar(path, "-wal"),
                sidecar(path, "-journal"),
            ];
            let before = sources
                .iter()
                .map(|p| stamp(p))
                .collect::<Result<Vec<_>>>()?;
            for (index, source) in sources.iter().enumerate() {
                if before[index].is_some() {
                    let dest = match index {
                        0 => target.clone(),
                        1 => sidecar(&target, "-wal"),
                        _ => sidecar(&target, "-journal"),
                    };
                    copy_bounded(source, &dest, limits.max_file_bytes)?;
                }
            }
            let after = sources
                .iter()
                .map(|p| stamp(p))
                .collect::<Result<Vec<_>>>()?;
            if before != after {
                continue;
            }
            // Recovery is permitted only on this private copy, without CREATE.
            let recovery = Connection::open_with_flags(
                &target,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            recovery.busy_timeout(limits.timeout)?;
            recovery.query_row("SELECT count(*) FROM sqlite_schema", [], |r| {
                r.get::<_, i64>(0)
            })?;
            drop(recovery);
            let connection = Connection::open_with_flags(
                &target,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            return Self::finish(connection, Some(dir), limits);
        }
        Err(ParseError::Invalid(
            "database changed during all snapshot attempts".into(),
        ))
    }
    fn finish(
        connection: Connection,
        snapshot: Option<tempfile::TempDir>,
        limits: DatabaseLimits,
    ) -> Result<Self> {
        connection.busy_timeout(limits.timeout)?;
        connection.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
        connection.set_limit(
            Limit::SQLITE_LIMIT_LENGTH,
            limits.max_value_bytes.min(i32::MAX as usize) as i32,
        )?;
        connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 64 * 1024)?;
        connection.set_limit(Limit::SQLITE_LIMIT_COLUMN, 256)?;
        connection.set_limit(Limit::SQLITE_LIMIT_EXPR_DEPTH, 64)?;
        connection.authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Select | AuthAction::Read { .. } | AuthAction::Recursive => {
                Authorization::Allow
            }
            AuthAction::Function { function_name }
                if !matches!(
                    function_name.to_ascii_lowercase().as_str(),
                    "load_extension" | "writefile" | "readfile"
                ) =>
            {
                Authorization::Allow
            }
            _ => Authorization::Deny,
        }))?;
        Ok(Self {
            connection,
            _snapshot: snapshot,
            limits,
        })
    }
    pub fn is_snapshot(&self) -> bool {
        self._snapshot.is_some()
    }
    /// SQL is a caller-authored SELECT, with positional bound parameters. An
    /// authorizer rejects writes, ATTACH, PRAGMA and extension/file functions.
    pub fn query(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        let start = Instant::now();
        let timeout = self.limits.timeout;
        let mut remaining = self.limits.max_vm_steps;
        self.connection.progress_handler(
            100,
            Some(move || {
                if remaining < 100 {
                    return true;
                }
                remaining -= 100;
                start.elapsed() >= timeout
            }),
        )?;
        let mut statement = self.connection.prepare(sql)?;
        if !statement.readonly() {
            return Err(ParseError::Invalid("writable SQLite statement".into()));
        }
        let columns = statement
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let mut cursor = statement.query(rusqlite::params_from_iter(params))?;
        let mut rows = Vec::new();
        let mut bytes: usize = 0;
        while let Some(row) = cursor.next()? {
            if start.elapsed() >= timeout {
                return Err(ParseError::Limit {
                    budget: "query milliseconds",
                    maximum: timeout.as_millis().min(usize::MAX as u128) as usize,
                });
            }
            if rows.len() >= self.limits.max_rows {
                return Err(ParseError::Limit {
                    budget: "query rows",
                    maximum: self.limits.max_rows,
                });
            }
            let mut values = Vec::new();
            for index in 0..columns.len() {
                let raw = row.get_ref(index)?;
                let size = match raw {
                    ValueRef::Text(s) | ValueRef::Blob(s) => s.len(),
                    _ => 8,
                };
                bytes = bytes.saturating_add(size);
                if size > self.limits.max_value_bytes || bytes > self.limits.max_result_bytes {
                    return Err(ParseError::Limit {
                        budget: "query result bytes",
                        maximum: self.limits.max_result_bytes,
                    });
                }
                values.push(match raw {
                    ValueRef::Null => SqlValue::Null,
                    ValueRef::Integer(v) => SqlValue::Integer(v),
                    ValueRef::Real(v) => SqlValue::Real(v),
                    ValueRef::Text(v) => SqlValue::Text(
                        std::str::from_utf8(v)
                            .map_err(|_| ParseError::Encoding)?
                            .to_owned(),
                    ),
                    ValueRef::Blob(v) => SqlValue::Blob(v.to_vec()),
                });
            }
            rows.push(values);
        }
        Ok(QueryResult { columns, rows })
    }
}
