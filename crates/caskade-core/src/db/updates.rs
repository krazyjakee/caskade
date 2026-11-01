//! Managed installs' update state (`game_updates`). The service lives in
//! `crate::updates`.

use super::{LibraryStore, decode};
use crate::{
    Result,
    model::GameEntry,
    updates::{Followed, NOT_FOLLOWED, UpdateRow},
};

type Row = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
);

const SELECT: &str = "SELECT u.entry_id,e.game_id,u.provider,u.project,u.installed,u.asset,\
     u.prereleases,u.latest,u.latest_url,u.checked_at_ms,u.skipped,u.error \
     FROM game_updates u JOIN game_entries e ON e.id=u.entry_id";

fn read(row: Row) -> UpdateRow {
    let (
        entry_id,
        game_id,
        provider,
        project,
        installed,
        asset,
        prereleases,
        latest,
        latest_url,
        checked_at_ms,
        skipped,
        error,
    ) = row;
    UpdateRow {
        entry_id,
        game_id,
        provider,
        project,
        installed,
        asset,
        prereleases,
        latest,
        latest_url,
        checked_at_ms,
        skipped,
        error,
    }
}

impl LibraryStore {
    pub async fn game_update(&self, entry_id: &str) -> Result<Option<UpdateRow>> {
        Ok(
            sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("{SELECT} WHERE u.entry_id=?")))
                .bind(entry_id)
                .fetch_optional(&self.pool)
                .await?
                .map(read),
        )
    }

    /// The followed installs of `game_id`.
    pub async fn game_updates(&self, game_id: &str) -> Result<Vec<UpdateRow>> {
        Ok(sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!(
            "{SELECT} WHERE e.game_id=? AND u.provider<>'{NOT_FOLLOWED}' ORDER BY u.entry_id"
        )))
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(read)
        .collect())
    }

    /// Every followed install.
    pub async fn followed_updates(&self) -> Result<Vec<UpdateRow>> {
        Ok(sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!(
            "{SELECT} WHERE u.provider<>'{NOT_FOLLOWED}' ORDER BY u.checked_at_ms, u.entry_id"
        )))
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(read)
        .collect())
    }

    /// Follows `entry_id` through `provider`. Following the same project
    /// again keeps the last check and the user's choices; a release the
    /// URLs don't name keeps the one recorded.
    pub async fn follow_updates(
        &self,
        entry_id: &str,
        provider: &str,
        followed: &Followed,
    ) -> Result<()> {
        let prereleases = followed
            .version
            .as_deref()
            .is_some_and(crate::updates::looks_prerelease);
        sqlx::query(
            "INSERT INTO game_updates(entry_id,provider,project,installed,asset,prereleases) \
             VALUES(?,?,?,?,?,?) ON CONFLICT(entry_id) DO UPDATE SET \
             installed=CASE WHEN provider=excluded.provider AND project=excluded.project \
                 THEN coalesce(excluded.installed,installed) ELSE excluded.installed END,\
             asset=coalesce(excluded.asset,CASE WHEN provider=excluded.provider \
                 AND project=excluded.project THEN asset END),\
             latest=CASE WHEN provider=excluded.provider AND project=excluded.project \
                 THEN latest END,\
             latest_url=CASE WHEN provider=excluded.provider AND project=excluded.project \
                 THEN latest_url END,\
             checked_at_ms=CASE WHEN provider=excluded.provider AND project=excluded.project \
                 THEN checked_at_ms ELSE 0 END,\
             skipped=CASE WHEN provider=excluded.provider AND project=excluded.project \
                 THEN skipped END,\
             prereleases=CASE WHEN provider=excluded.provider AND project=excluded.project \
                 THEN prereleases ELSE excluded.prereleases END,\
             error=NULL,provider=excluded.provider,project=excluded.project",
        )
        .bind(entry_id)
        .bind(provider)
        .bind(&followed.project)
        .bind(&followed.version)
        .bind(&followed.asset)
        .bind(prereleases)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records that no provider follows `entry_id`'s install, so tracking
    /// doesn't read its manifest again.
    pub async fn not_followed(&self, entry_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO game_updates(entry_id,provider,project) VALUES(?,?,'') \
             ON CONFLICT(entry_id) DO UPDATE SET provider=excluded.provider,project='',\
             installed=NULL,asset=NULL,latest=NULL,latest_url=NULL,checked_at_ms=0,\
             skipped=NULL,error=NULL",
        )
        .bind(entry_id)
        .bind(NOT_FOLLOWED)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Managed installs tracking hasn't looked at.
    pub async fn untracked_managed_entries(&self) -> Result<Vec<GameEntry>> {
        sqlx::query_scalar::<_, String>(
            "SELECT e.record_json FROM game_entries e \
             LEFT JOIN game_updates u ON u.entry_id=e.id \
             WHERE e.source_id=? AND e.location_kind='managed' AND u.entry_id IS NULL \
             ORDER BY e.id",
        )
        .bind(crate::install::STORAGE_SOURCE.as_str())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }

    /// Saves a check's answer: the newest release, and the installed one a
    /// first check worked out.
    pub async fn save_update_check(&self, row: &UpdateRow) -> Result<()> {
        sqlx::query(
            "UPDATE game_updates SET installed=?,latest=?,latest_url=?,checked_at_ms=?,error=? \
             WHERE entry_id=?",
        )
        .bind(&row.installed)
        .bind(&row.latest)
        .bind(&row.latest_url)
        .bind(row.checked_at_ms)
        .bind(&row.error)
        .bind(&row.entry_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Saves the user's choices: prereleases, and a skipped release.
    pub async fn save_update_choices(&self, row: &UpdateRow) -> Result<()> {
        sqlx::query("UPDATE game_updates SET prereleases=?,skipped=? WHERE entry_id=?")
            .bind(row.prereleases)
            .bind(&row.skipped)
            .bind(&row.entry_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Records that `entry_id` now has release `version`, installed from
    /// `asset`.
    pub async fn updated_to(
        &self,
        entry_id: &str,
        version: &str,
        asset: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE game_updates SET installed=?,asset=coalesce(?,asset),error=NULL \
             WHERE entry_id=?",
        )
        .bind(version)
        .bind(asset)
        .bind(entry_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
