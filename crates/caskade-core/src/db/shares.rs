//! Shares (#246, #248): things this device shares once with another person
//! through a link, and who received them. A share's grant is a credential:
//! the tables are this device's only, never synced or backed up.

use super::LibraryStore;
use crate::Result;

/// A share as stored.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ShareRow {
    pub id: String,
    /// `game`, `save`, `web`, `rom` or `collection`.
    pub kind: String,
    pub game_id: Option<String>,
    pub entry_id: Option<String>,
    pub title: String,
    pub size: i64,
    pub root: Option<String>,
    pub extras_root: Option<String>,
    /// 64 hex characters.
    pub grant: String,
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub stopped_at_ms: Option<i64>,
    pub details_json: String,
}

impl ShareRow {
    /// Whether the link works at `now`: not stopped, not expired.
    pub fn running(&self, now: i64) -> bool {
        self.stopped_at_ms.is_none() && self.expires_at_ms.is_none_or(|at| now < at)
    }
}

/// Someone who opened a share's link.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct RecipientRow {
    pub share_id: String,
    pub endpoint_id: String,
    pub name: String,
    pub first_seen_ms: i64,
    pub done_bytes: i64,
    pub finished_at_ms: Option<i64>,
}

macro_rules! columns {
    () => {
        "id,kind,game_id,entry_id,title,size,root,extras_root,grant,\
         created_at_ms,expires_at_ms,stopped_at_ms,details_json"
    };
}

impl LibraryStore {
    pub async fn insert_share(&self, share: &ShareRow) -> Result<()> {
        sqlx::query(concat!(
            "INSERT INTO shares(",
            columns!(),
            ") VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)"
        ))
        .bind(&share.id)
        .bind(&share.kind)
        .bind(&share.game_id)
        .bind(&share.entry_id)
        .bind(&share.title)
        .bind(share.size)
        .bind(&share.root)
        .bind(&share.extras_root)
        .bind(&share.grant)
        .bind(share.created_at_ms)
        .bind(share.expires_at_ms)
        .bind(share.stopped_at_ms)
        .bind(&share.details_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every share, the newest first.
    pub async fn shares(&self) -> Result<Vec<ShareRow>> {
        Ok(sqlx::query_as(concat!(
            "SELECT ",
            columns!(),
            " FROM shares ORDER BY created_at_ms DESC,rowid DESC"
        ))
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn share(&self, id: &str) -> Result<Option<ShareRow>> {
        Ok(
            sqlx::query_as(concat!("SELECT ", columns!(), " FROM shares WHERE id=?"))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Records the collection share `id`'s files were read as, and its size.
    pub async fn set_share_root(&self, id: &str, root: &str, size: i64) -> Result<()> {
        sqlx::query("UPDATE shares SET root=?,size=? WHERE id=?")
            .bind(root)
            .bind(size)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Stops a share at `at`; `false` when it was stopped already.
    pub async fn stop_share(&self, id: &str, at: i64) -> Result<bool> {
        Ok(
            sqlx::query("UPDATE shares SET stopped_at_ms=? WHERE id=? AND stopped_at_ms IS NULL")
                .bind(at)
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }

    /// Forgets a stopped share and its recipients.
    pub async fn delete_share(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM share_recipients WHERE share_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM shares WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Records that `endpoint_id`, called `name`, opened share `share_id`.
    pub async fn add_share_recipient(
        &self,
        share_id: &str,
        endpoint_id: &str,
        name: &str,
        at: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO share_recipients(share_id,endpoint_id,name,first_seen_ms) VALUES(?,?,?,?) \
             ON CONFLICT(share_id,endpoint_id) DO UPDATE SET name=excluded.name",
        )
        .bind(share_id)
        .bind(endpoint_id)
        .bind(name)
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// How far a recipient got; `finished_at` once everything arrived.
    pub async fn set_share_progress(
        &self,
        share_id: &str,
        endpoint_id: &str,
        done_bytes: i64,
        finished_at: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE share_recipients SET done_bytes=max(done_bytes,?),\
             finished_at_ms=coalesce(finished_at_ms,?) WHERE share_id=? AND endpoint_id=?",
        )
        .bind(done_bytes)
        .bind(finished_at)
        .bind(share_id)
        .bind(endpoint_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn share_recipients(&self, share_id: &str) -> Result<Vec<RecipientRow>> {
        Ok(sqlx::query_as(
            "SELECT share_id,endpoint_id,name,first_seen_ms,done_bytes,finished_at_ms \
             FROM share_recipients WHERE share_id=? ORDER BY first_seen_ms,rowid",
        )
        .bind(share_id)
        .fetch_all(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, expires: Option<i64>) -> ShareRow {
        ShareRow {
            id: id.into(),
            kind: "game".into(),
            game_id: Some("g1".into()),
            entry_id: Some("e1".into()),
            title: "Celeste".into(),
            size: 1024,
            root: Some("ab".repeat(32)),
            extras_root: None,
            grant: "cd".repeat(32),
            created_at_ms: 1_000,
            expires_at_ms: expires,
            stopped_at_ms: None,
            details_json: "{}".into(),
        }
    }

    #[tokio::test]
    async fn shares_and_their_recipients_are_kept_and_stopped() {
        let store = LibraryStore::in_memory().await.unwrap();
        store.insert_share(&row("s1", Some(5_000))).await.unwrap();
        store.insert_share(&row("s2", None)).await.unwrap();
        let s1 = store.share("s1").await.unwrap().unwrap();
        assert!(s1.running(4_999));
        assert!(!s1.running(5_000), "expired");
        assert!(store.share("s2").await.unwrap().unwrap().running(i64::MAX));

        store.add_share_recipient("s1", "peer1", "Sam", 2_000).await.unwrap();
        store.add_share_recipient("s1", "peer1", "Sam's PC", 2_500).await.unwrap();
        store.set_share_progress("s1", "peer1", 512, None).await.unwrap();
        store.set_share_progress("s1", "peer1", 100, Some(3_000)).await.unwrap();
        let recipients = store.share_recipients("s1").await.unwrap();
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0].name, "Sam's PC");
        assert_eq!(recipients[0].done_bytes, 512, "progress never goes back");
        assert_eq!(recipients[0].finished_at_ms, Some(3_000));

        assert!(store.stop_share("s1", 4_000).await.unwrap());
        assert!(!store.stop_share("s1", 4_100).await.unwrap());
        assert!(!store.share("s1").await.unwrap().unwrap().running(4_001));
        store.delete_share("s1").await.unwrap();
        assert!(store.share("s1").await.unwrap().is_none());
        assert!(store.share_recipients("s1").await.unwrap().is_empty());
        assert_eq!(store.shares().await.unwrap().len(), 1);
    }
}
