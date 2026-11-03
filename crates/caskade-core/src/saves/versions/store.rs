//! Save versions in the library database (migration 0033), and their part
//! in the change log: each version made here is one `saveVersion` record.

use serde_json::{Value, json};
use sqlx::SqliteConnection;

use super::{SaveVersion, VersionReason};
use crate::db::{FieldKey, LibraryStore};
use crate::sync::change::{EXISTS, Entity};
use crate::{Error, Result};

macro_rules! columns {
    () => {
        "id,game_id,account_id,device_id,session_id,created_at_ms,content_hash,\
         size_bytes,file_count,parents_json,reason,local"
    };
}

type Row = (
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    String,
    i64,
    i64,
    String,
    String,
    bool,
);

fn version(row: Row) -> SaveVersion {
    let (id, game, account, device, session, created, content, size, files, parents, reason, local) =
        row;
    SaveVersion {
        id,
        game_id: game,
        account_id: account,
        device_id: device,
        session_id: session,
        created_at_ms: created,
        content_hash: content,
        size_bytes: u64::try_from(size).unwrap_or_default(),
        file_count: u32::try_from(files).unwrap_or_default(),
        parents: serde_json::from_str(&parents).unwrap_or_default(),
        reason: VersionReason::parse(&reason),
        local,
    }
}

/// A version as the change log carries it: the same value whether it's
/// read here or applied from another device, so capture never sees a
/// difference that isn't one.
pub(super) fn record(version: &SaveVersion) -> Value {
    let mut parents = version.parents.clone();
    parents.sort();
    parents.dedup();
    json!({
        "game": version.game_id,
        "account": version.account_id,
        "device": version.device_id,
        "session": version.session_id,
        "created": version.created_at_ms,
        "content": version.content_hash,
        "size": version.size_bytes,
        "files": version.file_count,
        "parents": parents,
        "reason": version.reason.as_str(),
    })
}

/// A record from another device as a version here; `None` when malformed.
fn from_record(id: &str, value: &Value) -> Option<SaveVersion> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let content = text("content")?;
    if super::blobs::BlobStore::new("").path(&content).is_none() || id.is_empty() {
        return None;
    }
    let parents: Vec<String> = value
        .get("parents")
        .and_then(Value::as_array)?
        .iter()
        .take(64)
        .filter_map(|parent| parent.as_str().map(str::to_owned))
        .collect();
    Some(SaveVersion {
        id: id.to_owned(),
        game_id: text("game")?,
        account_id: text("account").unwrap_or_default(),
        device_id: text("device").unwrap_or_default(),
        session_id: text("session"),
        created_at_ms: value.get("created").and_then(Value::as_i64)?,
        content_hash: content,
        size_bytes: value
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        file_count: value
            .get("files")
            .and_then(Value::as_u64)
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or_default(),
        parents,
        reason: VersionReason::parse(&text("reason").unwrap_or_default()),
        local: false,
    })
}

/// The change log's snapshot: every version made here.
pub(crate) async fn sync_snapshot(
    conn: &mut SqliteConnection,
    values: &mut std::collections::HashMap<FieldKey, Value>,
) -> Result<()> {
    let rows: Vec<Row> = sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM save_versions WHERE local=1"
    ))
    .fetch_all(&mut *conn)
    .await?;
    for row in rows {
        let version = version(row);
        values.insert(
            (Entity::SaveVersion, version.id.clone(), EXISTS.to_owned()),
            record(&version),
        );
    }
    Ok(())
}

/// Applies another device's version: `false` while its game hasn't arrived.
/// Versions are never deleted by sync: each device prunes its own.
pub(crate) async fn sync_apply(
    conn: &mut SqliteConnection,
    id: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let Some(value) = value else {
        return Ok(true);
    };
    let Some(version) = from_record(id, value) else {
        tracing::warn!(%id, "a save version from another device was unreadable");
        return Ok(true);
    };
    let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM games WHERE id=?")
        .bind(&version.game_id)
        .fetch_one(&mut *conn)
        .await?;
    if exists == 0 {
        return Ok(false);
    }
    insert(conn, &version).await?;
    Ok(true)
}

async fn insert(conn: &mut SqliteConnection, version: &SaveVersion) -> Result<()> {
    let mut parents = version.parents.clone();
    parents.sort();
    parents.dedup();
    sqlx::query(concat!(
        "INSERT OR IGNORE INTO save_versions(",
        columns!(),
        ") VALUES(?,?,?,?,?,?,?,?,?,?,?,?)"
    ))
    .bind(&version.id)
    .bind(&version.game_id)
    .bind(&version.account_id)
    .bind(&version.device_id)
    .bind(&version.session_id)
    .bind(version.created_at_ms)
    .bind(&version.content_hash)
    .bind(i64::try_from(version.size_bytes).unwrap_or(i64::MAX))
    .bind(i64::from(version.file_count))
    .bind(serde_json::to_string(&parents)?)
    .bind(version.reason.as_str())
    .bind(version.local)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

impl LibraryStore {
    pub async fn insert_save_version(&self, version: &SaveVersion) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        insert(&mut conn, version).await
    }

    pub async fn save_version(&self, id: &str) -> Result<Option<SaveVersion>> {
        let row: Option<Row> = sqlx::query_as(concat!(
            "SELECT ",
            columns!(),
            " FROM save_versions WHERE id=?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(version))
    }

    /// A game's versions, newest first; one account's when given.
    pub async fn save_versions(
        &self,
        game_id: &str,
        account_id: Option<&str>,
    ) -> Result<Vec<SaveVersion>> {
        let rows: Vec<Row> = sqlx::query_as(concat!("SELECT ", columns!(), " FROM save_versions WHERE game_id=?1 AND (?2 IS NULL OR account_id=?2) ORDER BY created_at_ms DESC, id DESC"))
        .bind(game_id)
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(version).collect())
    }

    /// Every version's content hash: the file lists to keep.
    pub async fn save_content_hashes(&self) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT DISTINCT content_hash FROM save_versions")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn delete_save_versions(&self, ids: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for id in ids {
            sqlx::query("DELETE FROM save_versions WHERE id=?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The version this device's files match for a game and account, and
    /// the files' hash then.
    pub async fn save_head(
        &self,
        game_id: &str,
        account_id: &str,
    ) -> Result<Option<(String, String)>> {
        Ok(sqlx::query_as(
            "SELECT version_id,disk_hash FROM save_heads WHERE game_id=? AND account_id=?",
        )
        .bind(game_id)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn set_save_head(
        &self,
        game_id: &str,
        account_id: &str,
        version_id: &str,
        disk_hash: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO save_heads(game_id,account_id,version_id,disk_hash) VALUES(?,?,?,?) \
             ON CONFLICT(game_id,account_id) DO UPDATE SET version_id=excluded.version_id,\
             disk_hash=excluded.disk_hash",
        )
        .bind(game_id)
        .bind(account_id)
        .bind(version_id)
        .bind(disk_hash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every version id a head points at: never pruned.
    pub async fn save_head_versions(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar("SELECT version_id FROM save_heads")
            .fetch_all(&self.pool)
            .await?)
    }
}

impl VersionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Found => "found",
            Self::BeforeRestore => "beforeRestore",
            Self::Restored => "restored",
            Self::Chosen => "chosen",
            Self::Shared => "shared",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "found" => Self::Found,
            "beforeRestore" => Self::BeforeRestore,
            "restored" => Self::Restored,
            "chosen" => Self::Chosen,
            "shared" => Self::Shared,
            _ => Self::Session,
        }
    }
}

/// `Error::Invalid` for a version that isn't here.
pub(super) fn missing(id: &str) -> Error {
    Error::Invalid(crate::tr!("Save version {id} was not found.", id = id))
}
