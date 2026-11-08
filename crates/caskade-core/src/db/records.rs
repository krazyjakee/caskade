//! Storage, artwork, collections and settings repositories.
use super::{LibraryStore, decode};
use crate::{
    Error, Result,
    model::{validation as v, *},
};
use std::collections::BTreeMap;

impl LibraryStore {
    pub async fn upsert_root(&self, root: &StorageRoot) -> Result<StorageRoot> {
        root.validate()?;
        let mut tx = self.begin_write().await?;
        if root.is_default {
            sqlx::query("UPDATE storage_roots SET is_default=0,record_json=json_set(record_json,'$.isDefault',json('false')) WHERE is_default=1 AND id<>?").bind(&root.id).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO storage_roots(id,path,is_default,record_json) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET path=excluded.path,is_default=excluded.is_default,record_json=excluded.record_json").bind(&root.id).bind(v::path_text(&root.path)?).bind(root.is_default).bind(v::json(root)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(root.clone())
    }
    pub async fn get_root(&self, id: &str) -> Result<Option<StorageRoot>> {
        sqlx::query_scalar::<_, String>("SELECT record_json FROM storage_roots WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(decode)
            .transpose()
    }
    pub async fn roots(&self) -> Result<Vec<StorageRoot>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM storage_roots ORDER BY is_default DESC,id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }
    pub async fn delete_root(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM storage_roots WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }
    pub async fn upsert_artwork(&self, artwork: &Artwork) -> Result<Artwork> {
        artwork.validate()?;
        let kind = artwork_kind(artwork.kind);
        sqlx::query("INSERT INTO artwork(id,game_id,kind,content_hash,record_json) VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET game_id=excluded.game_id,kind=excluded.kind,content_hash=excluded.content_hash,record_json=excluded.record_json").bind(&artwork.id).bind(&artwork.game_id).bind(kind).bind(&artwork.content_hash).bind(v::json(artwork)?).execute(&self.pool).await?;
        Ok(artwork.clone())
    }
    pub async fn artwork_for_game(&self, game_id: &str) -> Result<Vec<Artwork>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM artwork WHERE game_id=? ORDER BY kind,id",
        )
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }
    /// Whether a game other than `game_id` has artwork of `kind` with
    /// these bytes.
    pub async fn artwork_shared(
        &self,
        content_hash: &str,
        game_id: &str,
        kind: ArtworkKind,
    ) -> Result<bool> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM artwork WHERE content_hash=? AND kind=? AND game_id<>?)",
        )
        .bind(content_hash)
        .bind(artwork_kind(kind))
        .bind(game_id)
        .fetch_one(&self.pool)
        .await?)
    }
    pub async fn delete_artwork(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM artwork WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }
    /// Writes one of `account`'s collections whole; an id another account
    /// uses is not found.
    pub async fn upsert_collection(
        &self,
        account: &str,
        collection: &Collection,
    ) -> Result<Collection> {
        v::id(&collection.id)?;
        collection_text(&collection.name, &collection.description)?;
        let query = smart_query(&collection.kind)?;
        if query.is_some() && !collection.game_ids.is_empty() {
            return Err(Error::Invalid(crate::tr!(
                "smart collections cannot contain manual members"
            )));
        }
        if collection.game_ids.len() > MEMBER_LIMIT {
            return Err(Error::Invalid(crate::tr!("collection member limit")));
        }
        let mut tx = self.begin_write().await?;
        let written = sqlx::query("INSERT INTO collections(id,account_id,name,description,pinned,kind,query_json,updated_at_ms) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,pinned=excluded.pinned,kind=excluded.kind,query_json=excluded.query_json,updated_at_ms=excluded.updated_at_ms WHERE collections.account_id=excluded.account_id").bind(&collection.id).bind(account).bind(&collection.name).bind(&collection.description).bind(collection.pinned).bind(if query.is_some(){"smart"}else{"manual"}).bind(query).bind(collection.updated_at_ms).execute(&mut *tx).await?;
        if written.rows_affected() == 0 {
            return Err(super::missing("collection", &collection.id));
        }
        sqlx::query("DELETE FROM collection_members WHERE collection_id=?")
            .bind(&collection.id)
            .execute(&mut *tx)
            .await?;
        for (position, id) in collection.game_ids.iter().enumerate() {
            v::id(id)?;
            sqlx::query("INSERT OR IGNORE INTO collection_members(collection_id,game_id,position) VALUES(?,?,?)").bind(&collection.id).bind(id).bind(position as i64).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        self.get_collection(account, &collection.id)
            .await?
            .ok_or_else(|| super::missing("collection", &collection.id))
    }
    /// Makes a collection, or changes one's name, description and rules
    /// (its pin and games stay). A collection that turns smart loses the
    /// games it was given by hand.
    pub async fn save_collection(
        &self,
        account: &str,
        save: &SaveCollection,
    ) -> Result<Collection> {
        collection_text(&save.name, &save.description)?;
        let query = smart_query(&save.kind)?;
        let kind = if query.is_some() { "smart" } else { "manual" };
        let now = crate::now_ms();
        let mut tx = self.begin_write().await?;
        let id = match &save.id {
            Some(id) => {
                let updated = sqlx::query("UPDATE collections SET name=?,description=?,kind=?,query_json=?,updated_at_ms=? WHERE id=? AND account_id=?").bind(&save.name).bind(&save.description).bind(kind).bind(&query).bind(now).bind(id).bind(account).execute(&mut *tx).await?;
                if updated.rows_affected() == 0 {
                    return Err(super::missing("collection", id));
                }
                if query.is_some() {
                    sqlx::query("DELETE FROM collection_members WHERE collection_id=?")
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
                id.clone()
            }
            None => {
                let id = new_id();
                sqlx::query("INSERT INTO collections(id,account_id,name,description,pinned,kind,query_json,updated_at_ms) VALUES(?,?,?,?,0,?,?,?)").bind(&id).bind(account).bind(&save.name).bind(&save.description).bind(kind).bind(&query).bind(now).execute(&mut *tx).await?;
                id
            }
        };
        tx.commit().await?;
        self.get_collection(account, &id)
            .await?
            .ok_or_else(|| super::missing("collection", &id))
    }
    /// Pins a collection to the front of the list, or unpins it.
    pub async fn set_collection_pinned(&self, account: &str, id: &str, pinned: bool) -> Result<()> {
        let updated = sqlx::query("UPDATE collections SET pinned=? WHERE id=? AND account_id=?")
            .bind(pinned)
            .bind(id)
            .bind(account)
            .execute(&self.pool)
            .await?;
        if updated.rows_affected() == 0 {
            return Err(super::missing("collection", id));
        }
        Ok(())
    }
    /// Adds games to the end of a manual collection; ones already in it
    /// keep their place.
    pub async fn add_to_collection(
        &self,
        account: &str,
        id: &str,
        game_ids: &[String],
    ) -> Result<()> {
        if game_ids.len() > MEMBER_LIMIT {
            return Err(Error::Invalid(crate::tr!("collection member limit")));
        }
        let mut tx = self.begin_write().await?;
        manual_collection(&mut tx, account, id).await?;
        let mut next: i64 = sqlx::query_scalar(
            "SELECT coalesce(max(position)+1,0) FROM collection_members WHERE collection_id=?",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let mut added = false;
        for game in game_ids {
            v::id(game)?;
            let inserted = sqlx::query("INSERT OR IGNORE INTO collection_members(collection_id,game_id,position) VALUES(?,?,?)").bind(id).bind(game).bind(next).execute(&mut *tx).await?;
            if inserted.rows_affected() > 0 {
                next += 1;
                added = true;
            }
        }
        if added {
            touch_collection(&mut tx, id).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    /// Takes games out of a manual collection.
    pub async fn remove_from_collection(
        &self,
        account: &str,
        id: &str,
        game_ids: &[String],
    ) -> Result<()> {
        let mut tx = self.begin_write().await?;
        manual_collection(&mut tx, account, id).await?;
        let mut removed = false;
        for game in game_ids {
            let deleted =
                sqlx::query("DELETE FROM collection_members WHERE collection_id=? AND game_id=?")
                    .bind(id)
                    .bind(game)
                    .execute(&mut *tx)
                    .await?;
            removed |= deleted.rows_affected() > 0;
        }
        if removed {
            touch_collection(&mut tx, id).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    /// The manual collections of `account`'s a game is in.
    pub async fn game_collection_ids(&self, account: &str, game_id: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT m.collection_id FROM collection_members m JOIN collections c ON c.id=m.collection_id WHERE m.game_id=? AND c.account_id=? ORDER BY m.collection_id",
        )
        .bind(game_id)
        .bind(account)
        .fetch_all(&self.pool)
        .await?)
    }
    /// One of `account`'s collections; another account's is `None`.
    pub async fn get_collection(&self, account: &str, id: &str) -> Result<Option<Collection>> {
        let Some(row) = sqlx::query_as::<_, CollectionRow>(
            "SELECT id,name,description,pinned,kind,query_json,updated_at_ms FROM collections WHERE id=? AND account_id=?",
        )
        .bind(id)
        .bind(account)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let mut collection = collection_from_row(row)?;
        collection.game_ids=sqlx::query_scalar::<_,String>("SELECT game_id FROM collection_members WHERE collection_id=? ORDER BY position,game_id").bind(id).fetch_all(&self.pool).await?;
        Ok(Some(collection))
    }
    /// Every collection of `account`'s, without its games: pinned ones
    /// first, then by name.
    pub(super) async fn collection_list(&self, account: &str) -> Result<Vec<Collection>> {
        sqlx::query_as::<_, CollectionRow>(
            "SELECT id,name,description,pinned,kind,query_json,updated_at_ms FROM collections WHERE account_id=? ORDER BY pinned DESC,name COLLATE NOCASE,id",
        )
        .bind(account)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(collection_from_row)
        .collect()
    }
    pub async fn collections(&self, account: &str) -> Result<Vec<Collection>> {
        let mut out = Vec::new();
        for collection in self.collection_list(account).await? {
            if let Some(collection) = self.get_collection(account, &collection.id).await? {
                out.push(collection);
            }
        }
        Ok(out)
    }
    pub async fn delete_collection(&self, account: &str, id: &str) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM collections WHERE id=? AND account_id=?")
                .bind(id)
                .bind(account)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }
    pub async fn get_setting(&self, key: &str) -> Result<Option<serde_json::Value>> {
        sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key=?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?
            .map(decode)
            .transpose()
    }
    pub async fn set_setting(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        let value = setting(key, value)?;
        sqlx::query("INSERT INTO settings(key,value_json) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json").bind(key).bind(value).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn delete_setting(&self, key: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM settings WHERE key=?")
            .bind(key)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }
    /// Atomic optimistic write. Pass the exact value returned by get_setting;
    /// None means the row must not exist (distinct from a stored JSON null).
    pub async fn compare_and_set_setting(
        &self,
        key: &str,
        expected: Option<&serde_json::Value>,
        value: &serde_json::Value,
    ) -> Result<bool> {
        let value = setting(key, value)?;
        let changed = if let Some(expected) = expected {
            sqlx::query("UPDATE settings SET value_json=? WHERE key=? AND value_json=?")
                .bind(value)
                .bind(key)
                .bind(v::json(expected)?)
                .execute(&self.pool)
                .await?
                .rows_affected()
        } else {
            sqlx::query("INSERT OR IGNORE INTO settings(key,value_json) VALUES(?,?)")
                .bind(key)
                .bind(value)
                .execute(&self.pool)
                .await?
                .rows_affected()
        };
        Ok(changed == 1)
    }
    pub async fn settings_with_prefix(
        &self,
        prefix: &str,
    ) -> Result<BTreeMap<String, serde_json::Value>> {
        v::text(prefix, "setting prefix", true, 1024)?;
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT key,value_json FROM settings WHERE key LIKE ? ESCAPE '\\' ORDER BY key",
        )
        .bind(format!("{}%", escape_like(prefix)))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(key, value)| Ok((key, decode(value)?)))
            .collect()
    }
}
fn setting(key: &str, value: &serde_json::Value) -> Result<String> {
    v::text(key, "setting key", false, 1024)?;
    let value = v::json(value)?;
    if value.len() > 1024 * 1024 {
        return Err(Error::Invalid(crate::tr!("setting value limit")));
    }
    Ok(value)
}
pub(super) fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

const MEMBER_LIMIT: usize = 100_000;
type CollectionRow = (String, String, String, bool, String, Option<String>, i64);

fn collection_from_row(
    (id, name, description, pinned, kind, query, updated_at_ms): CollectionRow,
) -> Result<Collection> {
    let kind = if kind == "manual" {
        CollectionKind::Manual
    } else {
        CollectionKind::Smart {
            query: decode(
                query.ok_or_else(|| Error::Invalid(crate::tr!("smart collection has no query")))?,
            )?,
        }
    };
    Ok(Collection {
        id,
        name,
        description,
        pinned,
        kind,
        game_ids: vec![],
        updated_at_ms,
    })
}

fn collection_text(name: &str, description: &str) -> Result<()> {
    v::text(name, "collection name", false, 4096)?;
    v::text(description, "collection description", true, 4096)
}

/// A smart collection's query as stored; `None` for a manual one.
fn smart_query(kind: &CollectionKind) -> Result<Option<String>> {
    match kind {
        CollectionKind::Manual => Ok(None),
        CollectionKind::Smart { query } => {
            query.validate()?;
            Ok(Some(v::json(query)?))
        }
    }
}

async fn manual_collection(tx: &mut sqlx::SqliteConnection, account: &str, id: &str) -> Result<()> {
    let kind: Option<String> =
        sqlx::query_scalar("SELECT kind FROM collections WHERE id=? AND account_id=?")
            .bind(id)
            .bind(account)
            .fetch_optional(&mut *tx)
            .await?;
    match kind.as_deref() {
        None => Err(super::missing("collection", id)),
        Some("smart") => Err(Error::Invalid(crate::tr!(
            "a smart collection chooses its own games"
        ))),
        Some(_) => Ok(()),
    }
}

async fn touch_collection(tx: &mut sqlx::SqliteConnection, id: &str) -> Result<()> {
    sqlx::query("UPDATE collections SET updated_at_ms=? WHERE id=?")
        .bind(crate::now_ms())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// An artwork kind as the `artwork` table's `kind` column has it.
fn artwork_kind(kind: ArtworkKind) -> &'static str {
    match kind {
        ArtworkKind::Cover => "cover",
        ArtworkKind::Hero => "hero",
        ArtworkKind::Logo => "logo",
        ArtworkKind::Icon => "icon",
        ArtworkKind::Screenshot => "screenshot",
    }
}
