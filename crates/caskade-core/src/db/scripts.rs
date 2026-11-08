//! Lifecycle scripts (#179) in the library: global, per emulator profile
//! and per game, each scope in its own order.

use super::{LibraryStore, decode};
use crate::{
    Error, Result,
    model::validation as v,
    scripts::{LifecycleScript, ScriptScope, ScriptSet},
};

/// Scripts one scope may have.
const MAX_SCRIPTS: i64 = 256;

fn columns(scope: &ScriptScope) -> (&'static str, Option<&str>, Option<&str>) {
    match scope {
        ScriptScope::Global => ("global", None, None),
        ScriptScope::Profile { profile_id } => ("profile", Some(profile_id), None),
        ScriptScope::Game { game_id } => ("game", None, Some(game_id)),
    }
}

impl LibraryStore {
    /// One scope's scripts in their order, enabled or not.
    pub async fn lifecycle_scripts(&self, scope: &ScriptScope) -> Result<Vec<LifecycleScript>> {
        let (kind, profile, game) = columns(scope);
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM lifecycle_scripts WHERE scope=? AND profile_id IS ? \
             AND game_id IS ? ORDER BY position,id",
        )
        .bind(kind)
        .bind(profile)
        .bind(game)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }

    pub async fn lifecycle_script(&self, id: &str) -> Result<Option<LifecycleScript>> {
        sqlx::query_scalar::<_, String>("SELECT record_json FROM lifecycle_scripts WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(decode)
            .transpose()
    }

    /// The scripts a launch of `game_id` (through `profile_id`, for an
    /// emulated game) runs with.
    pub async fn scripts_for_launch(
        &self,
        game_id: &str,
        profile_id: Option<&str>,
    ) -> Result<ScriptSet> {
        Ok(ScriptSet {
            global: self.lifecycle_scripts(&ScriptScope::Global).await?,
            profile: match profile_id {
                Some(profile_id) => {
                    self.lifecycle_scripts(&ScriptScope::Profile {
                        profile_id: profile_id.into(),
                    })
                    .await?
                }
                None => vec![],
            },
            game: self
                .lifecycle_scripts(&ScriptScope::Game {
                    game_id: game_id.into(),
                })
                .await?,
        })
    }

    /// Adds the script last in its scope, or replaces it in its place. A
    /// script can't move to another scope.
    pub async fn save_lifecycle_script(&self, script: &LifecycleScript) -> Result<LifecycleScript> {
        let script = script.normalized()?;
        let (kind, profile, game) = columns(&script.scope);
        let mut tx = self.begin_write().await?;
        let existing =
            sqlx::query_scalar::<_, String>("SELECT record_json FROM lifecycle_scripts WHERE id=?")
                .bind(&script.id)
                .fetch_optional(&mut *tx)
                .await?
                .map(decode::<LifecycleScript>)
                .transpose()?;
        match existing {
            Some(old) if old.scope != script.scope => {
                return Err(Error::Invalid(crate::tr!(
                    "a script can't move to another scope; copy it instead"
                )));
            }
            Some(_) => {
                sqlx::query("UPDATE lifecycle_scripts SET record_json=? WHERE id=?")
                    .bind(v::json(&script)?)
                    .bind(&script.id)
                    .execute(&mut *tx)
                    .await?;
            }
            None => {
                let (count, next): (i64, i64) = sqlx::query_as(
                    "SELECT count(*),coalesce(max(position)+1,0) FROM lifecycle_scripts \
                     WHERE scope=? AND profile_id IS ? AND game_id IS ?",
                )
                .bind(kind)
                .bind(profile)
                .bind(game)
                .fetch_one(&mut *tx)
                .await?;
                if count >= MAX_SCRIPTS {
                    return Err(Error::Invalid(crate::tr!(
                        "a scope can have at most {count} scripts",
                        count = MAX_SCRIPTS
                    )));
                }
                let written = sqlx::query(
                    "INSERT INTO lifecycle_scripts(id,scope,profile_id,game_id,position,record_json) \
                     VALUES(?,?,?,?,?,?)",
                )
                .bind(&script.id)
                .bind(kind)
                .bind(profile)
                .bind(game)
                .bind(next)
                .bind(v::json(&script)?)
                .execute(&mut *tx)
                .await;
                if let Err(sqlx::Error::Database(error)) = &written
                    && error.is_foreign_key_violation()
                {
                    return Err(Error::Invalid(match &script.scope {
                        ScriptScope::Profile { .. } => {
                            crate::tr!("This emulator profile no longer exists.")
                        }
                        _ => crate::tr!("the script's game was not found"),
                    }));
                }
                written?;
            }
        }
        tx.commit().await?;
        Ok(script)
    }

    pub async fn delete_lifecycle_script(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM lifecycle_scripts WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }

    /// Puts a scope's scripts in the order of `ids`, which must name each
    /// of them once.
    pub async fn order_lifecycle_scripts(&self, scope: &ScriptScope, ids: &[String]) -> Result<()> {
        let (kind, profile, game) = columns(scope);
        let mut tx = self.begin_write().await?;
        let mut current: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM lifecycle_scripts WHERE scope=? AND profile_id IS ? AND game_id IS ?",
        )
        .bind(kind)
        .bind(profile)
        .bind(game)
        .fetch_all(&mut *tx)
        .await?;
        let mut wanted = ids.to_vec();
        current.sort();
        wanted.sort();
        if current != wanted {
            return Err(Error::Invalid(crate::tr!(
                "the new order must list each of the scope's scripts once"
            )));
        }
        for (position, id) in ids.iter().enumerate() {
            sqlx::query("UPDATE lifecycle_scripts SET position=? WHERE id=?")
                .bind(position as i64)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
