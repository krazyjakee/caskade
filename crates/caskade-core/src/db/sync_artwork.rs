//! The user's own artwork between devices (#273): each `user`-source artwork
//! row as an `artwork` entity, keyed `game|artwork id`, whose value names
//! its kind, size and content hash. The file follows as an `artwork` blob
//! (`sync::artwork`); a row arrives only once its file is in the cache here,
//! so a game never shows a picture that isn't there.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::sync_log::{FieldState, field, mark_applied, set_field};
use super::{Applied, FieldKey, LibraryStore};
use crate::Result;
use crate::artwork::CachedArtwork;
use crate::model::{Artwork, ArtworkKind, validation as v};
use crate::sync::change::{Change, EXISTS, Entity, join_key, key_parts};

/// What an `artwork` change says: its kind, content hash and size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedArtwork {
    pub kind: ArtworkKind,
    pub hash: String,
}

impl SyncedArtwork {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            kind: serde_json::from_value(value.get("kind")?.clone()).ok()?,
            hash: value.get("hash")?.as_str()?.to_owned(),
        })
    }
}

/// The user's artwork here, as the log sees it.
pub(super) async fn snapshot(
    conn: &mut sqlx::SqliteConnection,
    values: &mut HashMap<FieldKey, Value>,
) -> Result<()> {
    for (id, game, kind, hash, width, height) in
        sqlx::query_as::<_, (String, String, String, String, Option<i64>, Option<i64>)>(
            "SELECT id,game_id,kind,content_hash,record_json->>'$.width',record_json->>'$.height' \
             FROM artwork WHERE record_json->>'$.source'='user'",
        )
        .fetch_all(conn)
        .await?
    {
        values.insert(
            (Entity::Artwork, join_key(&[&game, &id]), EXISTS.to_owned()),
            json!({ "kind": kind, "hash": hash, "width": width, "height": height }),
        );
    }
    Ok(())
}

impl LibraryStore {
    /// The artwork files changes waiting here name: what to ask other
    /// devices for.
    pub async fn sync_wanted_artwork(&self) -> Result<Vec<String>> {
        let values: Vec<String> = sqlx::query_scalar(
            "SELECT value_json FROM sync_changes \
             WHERE entity=? AND applied=0 AND value_json IS NOT NULL",
        )
        .bind(Entity::Artwork.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut hashes: Vec<String> = values
            .iter()
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .filter_map(|value| SyncedArtwork::from_value(&value))
            .map(|artwork| artwork.hash)
            .collect();
        hashes.sort();
        hashes.dedup();
        Ok(hashes)
    }

    /// Whether the user's own artwork here uses `hash`: only those files
    /// are served to other devices.
    pub async fn sync_has_user_artwork(&self, hash: &str) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM artwork WHERE content_hash=? \
             AND record_json->>'$.source'='user')",
        )
        .bind(hash)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Applies an `artwork` change to `target` (`game|id` here): the row,
    /// from `cached` (its file, already in the cache), or its removal. One
    /// transaction with the field's new winner.
    pub async fn sync_apply_artwork(
        &self,
        change: &Change,
        target: &str,
        value: Option<Value>,
        cached: Option<CachedArtwork>,
    ) -> Result<Applied> {
        let [game, id] = key_parts(target)[..] else {
            self.sync_mark_applied(change, Some(target)).await?;
            return Ok(Applied::Superseded);
        };
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = field(&mut tx, Entity::Artwork, target, EXISTS).await?;
        if current.is_some_and(|current| current.stamp >= change.stamp()) {
            mark_applied(&mut tx, change, Some(target)).await?;
            tx.commit().await?;
            return Ok(Applied::Superseded);
        }
        match value.as_ref().and_then(SyncedArtwork::from_value) {
            Some(synced) => {
                let Some(cached) = cached else {
                    return Ok(Applied::Wait);
                };
                let found: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id=?)")
                        .bind(game)
                        .fetch_one(&mut *tx)
                        .await?;
                if !found {
                    return Ok(Applied::Wait);
                }
                let mut artwork = Artwork::new(
                    game,
                    synced.kind,
                    &cached.content_hash,
                    "user",
                    &cached.cached_path,
                    cached.width,
                    cached.height,
                );
                artwork.id = id.to_owned();
                artwork.thumbnails = cached
                    .thumbnails
                    .iter()
                    .map(|thumbnail| crate::model::ArtworkThumbnail {
                        size: thumbnail.size,
                        path: thumbnail.path.clone(),
                        width: thumbnail.width,
                        height: thumbnail.height,
                    })
                    .collect();
                artwork.validate()?;
                sqlx::query(
                    "INSERT INTO artwork(id,game_id,kind,content_hash,record_json) VALUES(?,?,?,?,?) \
                     ON CONFLICT(id) DO UPDATE SET game_id=excluded.game_id,kind=excluded.kind,\
                     content_hash=excluded.content_hash,record_json=excluded.record_json",
                )
                .bind(&artwork.id)
                .bind(game)
                .bind(serde_json::to_value(synced.kind)?.as_str().unwrap_or_default().to_owned())
                .bind(&artwork.content_hash)
                .bind(v::json(&artwork)?)
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query("DELETE FROM artwork WHERE id=? AND record_json->>'$.source'='user'")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        set_field(
            &mut tx,
            (&Entity::Artwork, target, EXISTS),
            &FieldState {
                value,
                stamp: change.stamp(),
            },
        )
        .await?;
        mark_applied(&mut tx, change, Some(target)).await?;
        tx.commit().await?;
        Ok(Applied::Written)
    }
}
