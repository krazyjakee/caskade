//! A release's launch actions (#177): the actions the user added beside its
//! own launch, in order, and its default action or ask-at-launch choice.

use super::{LibraryStore, decode, missing};
use crate::{
    Error, Result,
    model::{EntryActionSettings, GameAction, InstallLocation, validation as v},
};

/// Actions one release may have; more is a mistake, not a use.
const MAX_ACTIONS: i64 = 256;

impl LibraryStore {
    /// The actions added to `entry_id`, in their order. Its built-in action
    /// (its own `launch`) is not among them.
    pub async fn game_actions(&self, entry_id: &str) -> Result<Vec<GameAction>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM game_actions WHERE entry_id=? ORDER BY position,id",
        )
        .bind(entry_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }

    pub async fn game_action(&self, id: &str) -> Result<Option<GameAction>> {
        sqlx::query_scalar::<_, String>("SELECT record_json FROM game_actions WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(decode)
            .transpose()
    }

    /// Adds the action, last, or replaces it in its place. An action can't
    /// move to another release.
    pub async fn save_game_action(&self, action: &GameAction) -> Result<GameAction> {
        let mut tx = self.begin_write().await?;
        let entry = super::games::load_entry(&mut tx, &action.entry_id).await?;
        let action = action.normalized(matches!(
            entry.location,
            Some(InstallLocation::Managed { .. })
        ))?;
        let owner = sqlx::query_scalar::<_, String>("SELECT entry_id FROM game_actions WHERE id=?")
            .bind(&action.id)
            .fetch_optional(&mut *tx)
            .await?;
        match owner {
            Some(owner) if owner != action.entry_id => {
                return Err(Error::Invalid(crate::tr!(
                    "an action can't move to another release"
                )));
            }
            Some(_) => {
                sqlx::query("UPDATE game_actions SET record_json=? WHERE id=?")
                    .bind(v::json(&action)?)
                    .bind(&action.id)
                    .execute(&mut *tx)
                    .await?;
            }
            None => {
                let (count, next): (i64, i64) = sqlx::query_as(
                    "SELECT count(*),coalesce(max(position)+1,0) FROM game_actions WHERE entry_id=?",
                )
                .bind(&action.entry_id)
                .fetch_one(&mut *tx)
                .await?;
                if count >= MAX_ACTIONS {
                    return Err(Error::Invalid(crate::tr!(
                        "a release can have at most {count} actions",
                        count = MAX_ACTIONS
                    )));
                }
                sqlx::query(
                    "INSERT INTO game_actions(id,entry_id,position,record_json) VALUES(?,?,?,?)",
                )
                .bind(&action.id)
                .bind(&action.entry_id)
                .bind(next)
                .bind(v::json(&action)?)
                .execute(&mut *tx)
                .await?;
            }
        }
        if action.kind == crate::model::ActionKind::Utility {
            // A tool can't be what Play starts.
            sqlx::query(
                "UPDATE entry_action_settings SET record_json=json_remove(record_json,'$.defaultAction') \
                 WHERE entry_id=? AND record_json->>'$.defaultAction'=?",
            )
            .bind(&action.entry_id)
            .bind(&action.id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(action)
    }

    /// Removes the action; a release whose default it was plays its
    /// built-in action again.
    pub async fn delete_game_action(&self, id: &str) -> Result<bool> {
        let mut tx = self.begin_write().await?;
        let entry = sqlx::query_scalar::<_, String>("SELECT entry_id FROM game_actions WHERE id=?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(entry) = entry else {
            return Ok(false);
        };
        sqlx::query("DELETE FROM game_actions WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE entry_action_settings SET record_json=json_remove(record_json,'$.defaultAction') \
             WHERE entry_id=? AND record_json->>'$.defaultAction'=?",
        )
        .bind(&entry)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Puts `entry_id`'s actions in the order of `ids`, which must name each
    /// of them once.
    pub async fn order_game_actions(&self, entry_id: &str, ids: &[String]) -> Result<()> {
        let mut tx = self.begin_write().await?;
        let mut current: Vec<String> =
            sqlx::query_scalar("SELECT id FROM game_actions WHERE entry_id=?")
                .bind(entry_id)
                .fetch_all(&mut *tx)
                .await?;
        let mut wanted = ids.to_vec();
        current.sort();
        wanted.sort();
        if current != wanted {
            return Err(Error::Invalid(crate::tr!(
                "the new order must list each of the release's actions once"
            )));
        }
        for (position, id) in ids.iter().enumerate() {
            sqlx::query("UPDATE game_actions SET position=? WHERE id=?")
                .bind(position as i64)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// `entry_id`'s default action and ask-at-launch choice; the defaults
    /// when the user never changed them.
    pub async fn entry_action_settings(&self, entry_id: &str) -> Result<EntryActionSettings> {
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM entry_action_settings WHERE entry_id=?",
        )
        .bind(entry_id)
        .fetch_optional(&self.pool)
        .await?
        .map(decode)
        .transpose()?
        .unwrap_or_default())
    }

    /// Stores `entry_id`'s action choice. A default must be one of its play
    /// actions, or its built-in action (`None`).
    pub async fn save_entry_action_settings(
        &self,
        entry_id: &str,
        settings: &EntryActionSettings,
    ) -> Result<EntryActionSettings> {
        let entry = self
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| missing("entry", entry_id))?;
        let managed = matches!(entry.location, Some(InstallLocation::Managed { .. }));
        let mut settings = settings.clone();
        if let Some(name) = &mut settings.built_in.name {
            *name = name.trim().to_owned();
            if name.is_empty() {
                settings.built_in.name = None;
            } else {
                v::text(name, "action name", false, 256)?;
            }
        }
        if let Some(tracking) = &settings.built_in.tracking {
            settings.built_in.tracking = Some(tracking.normalized(managed)?);
        }
        if let Some(id) = &settings.default_action
            && *id != crate::model::built_in_action_id(entry_id)
        {
            match self.game_action(id).await? {
                Some(action)
                    if action.entry_id == entry_id
                        && action.kind == crate::model::ActionKind::Play => {}
                _ => {
                    return Err(Error::Invalid(crate::tr!(
                        "the default must be one of the release's play actions"
                    )));
                }
            }
        }
        if settings
            .default_action
            .as_deref()
            .is_some_and(|id| id == crate::model::built_in_action_id(entry_id))
        {
            settings.default_action = None;
        }
        sqlx::query(
            "INSERT INTO entry_action_settings(entry_id,record_json) VALUES(?,?) \
             ON CONFLICT(entry_id) DO UPDATE SET record_json=excluded.record_json",
        )
        .bind(entry_id)
        .bind(v::json(&settings)?)
        .execute(&self.pool)
        .await?;
        Ok(settings)
    }
}
