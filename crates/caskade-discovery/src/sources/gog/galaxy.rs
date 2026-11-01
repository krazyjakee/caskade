//! Installed base products, titles and playtime from `galaxy-2.0.db`.
//! The database is opened read-only (or snapshotted with its WAL) by the
//! shared SQLite reader; a missing table becomes a warning.

use super::Record;
use crate::{
    env::HostEnv,
    parsers::sqlite::{Database, DatabaseLimits, SqlValue},
    sources::{heroic::time::unix_seconds, resolve},
    types::{Evidence, ScanCtx},
};
use std::{collections::BTreeMap, path::Path};

pub(super) fn records(env: &HostEnv, db: &Path, ctx: &ScanCtx) -> BTreeMap<u64, Record> {
    let mut records = BTreeMap::new();
    let mut database = match Database::open(db, DatabaseLimits::default()) {
        Ok(database) => database,
        Err(error) => {
            ctx.warn(format!("{}: {error}", db.display()));
            return records;
        }
    };
    let mut rows = |sql: &str| match database.query(sql, &[]) {
        Ok(result) => result.rows,
        Err(error) => {
            ctx.warn(format!("{}: {error}", db.display()));
            Vec::new()
        }
    };
    let id = |value: &SqlValue| match value {
        SqlValue::Integer(id) => u64::try_from(*id).ok(),
        SqlValue::Text(text) => text.strip_prefix("gog_").unwrap_or(text).parse().ok(),
        _ => None,
    };
    let text = |value: &SqlValue| match value {
        SqlValue::Text(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        _ => None,
    };
    for row in rows("SELECT productId, installationPath FROM InstalledBaseProducts") {
        let [product, path] = row.as_slice() else {
            continue;
        };
        let Some(product) = id(product) else { continue };
        records.insert(
            product,
            Record {
                install_dir: text(path).and_then(|path| resolve(env, &path)),
                evidence: vec![Evidence::File {
                    path: db.to_path_buf(),
                }],
                ..Record::default()
            },
        );
    }
    if records.is_empty() {
        return records;
    }
    for row in rows("SELECT productId, title FROM LimitedDetails") {
        if let [product, title] = row.as_slice()
            && let (Some(product), Some(title)) = (id(product), text(title))
            && let Some(record) = records.get_mut(&product)
        {
            record.title.get_or_insert(title);
        }
    }
    for row in rows("SELECT releaseKey, minutesInGame FROM GameTimes") {
        if let [key, SqlValue::Integer(minutes)] = row.as_slice()
            && let Some(record) = id(key).and_then(|product| records.get_mut(&product))
            && *minutes > 0
        {
            record.playtime_secs = Some(*minutes as u64 * 60);
        }
    }
    for row in rows("SELECT gameReleaseKey, lastPlayedDate FROM LastPlayedDates") {
        if let [key, date] = row.as_slice()
            && let Some(record) = id(key).and_then(|product| records.get_mut(&product))
        {
            record.last_played_unix = text(date).and_then(|date| {
                let iso = date.replace(' ', "T");
                unix_seconds(&iso).or_else(|| unix_seconds(&format!("{iso}Z")))
            });
        }
    }
    records
}
