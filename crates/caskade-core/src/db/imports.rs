//! Transactional import boundary. Adapters own discovery policy and mapping;
//! the repository owns identity, merging and first-import persistence.
use super::{
    LibraryStore, decode,
    games::{merge_in, save_entry, save_game},
};
use crate::{Error, Result, model::*};
use sqlx::{Sqlite, Transaction};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedGroup {
    pub game_id: String,
    pub created: bool,
    pub entry_ids: Vec<String>,
    pub seeded_history: bool,
}

/// What happened to a game when a source's entries left the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRemoval {
    /// It lost entries and keeps others.
    Updated(String),
    /// Nothing else was left of it.
    Removed(String),
}
impl SourceRemoval {
    pub fn game_id(&self) -> &str {
        match self {
            Self::Updated(id) | Self::Removed(id) => id,
        }
    }
}

impl LibraryStore {
    /// Import one already-intercepted group, primary entry first. Existing
    /// source identities win over proposed IDs and retain user metadata.
    /// History is an aggregate seed, considered only on this game's first
    /// import, even when disabled or absent. Never replaces actual sessions.
    pub async fn import_group(
        &self,
        proposed: &Game,
        entries: &[GameEntry],
        history: Option<&PlaySession>,
    ) -> Result<ImportedGroup> {
        self.import_group_retiring(proposed, entries, &[], history)
            .await
    }

    /// `import_group`, plus source identities that belong to the group but
    /// are not launch entries of their own (a desktop shortcut that opens a
    /// Steam game names the Steam installation). Rows saved for them earlier
    /// join this game; their sessions move to the primary entry and the rows
    /// go. A row with an open session stays until a later import.
    pub async fn import_group_retiring(
        &self,
        proposed: &Game,
        entries: &[GameEntry],
        retired: &[(SourceId, String)],
        history: Option<&PlaySession>,
    ) -> Result<ImportedGroup> {
        proposed.validate()?;
        if entries.is_empty() {
            return Err(Error::Invalid(crate::tr!("cannot import an empty group")));
        }
        let entries = entries
            .iter()
            .map(GameEntry::normalized)
            .collect::<Result<Vec<_>>>()?;
        let mut keys = BTreeSet::new();
        for entry in &entries {
            if !keys.insert((entry.source_id.clone(), entry.external_id.clone())) {
                return Err(Error::Invalid(crate::tr!(
                    "duplicate source identity in import group"
                )));
            }
        }
        if retired.iter().any(|key| keys.contains(key)) {
            return Err(Error::Invalid(crate::tr!(
                "retired source identity is also an import entry"
            )));
        }
        if let Some(history) = history {
            history.validate()?;
            if history.ended_at_ms.is_none() {
                return Err(Error::Invalid(crate::tr!(
                    "imported history must be finished"
                )));
            }
            if !entries.iter().any(|e| e.id == history.entry_id) {
                return Err(Error::Invalid(crate::tr!(
                    "history seed has no matching import entry"
                )));
            }
        }
        let mut tx = self.begin_write().await?;
        let mut previous = Vec::new();
        let mut game_ids = Vec::new();
        for entry in &entries {
            let old = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM game_entries WHERE source_id=? AND external_id=?",
            )
            .bind(entry.source_id.as_str())
            .bind(&entry.external_id)
            .fetch_optional(&mut *tx)
            .await?
            .map(decode::<GameEntry>)
            .transpose()?;
            if let Some(old) = &old
                && !game_ids.contains(&old.game_id)
            {
                game_ids.push(old.game_id.clone());
            }
            previous.push(old);
        }
        let mut retiring = Vec::new();
        for (source, external) in retired {
            let old = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM game_entries WHERE source_id=? AND external_id=?",
            )
            .bind(source.as_str())
            .bind(external)
            .fetch_optional(&mut *tx)
            .await?
            .map(decode::<GameEntry>)
            .transpose()?;
            if let Some(old) = old {
                if !game_ids.contains(&old.game_id) {
                    game_ids.push(old.game_id.clone());
                }
                retiring.push(old.id);
            }
        }
        let created = game_ids.is_empty();
        let game_id = game_ids
            .first()
            .cloned()
            .unwrap_or_else(|| proposed.id.clone());
        if created {
            // A proposed ID must never overwrite an unrelated existing game.
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id=?)")
                .bind(&game_id)
                .fetch_one(&mut *tx)
                .await?;
            if exists {
                return Err(Error::Invalid(crate::tr!(
                    "proposed game ID already exists"
                )));
            }
            save_game(&mut tx, proposed).await?;
        } else {
            for donor in game_ids.iter().skip(1) {
                merge_in(&mut tx, &game_id, donor).await?;
            }
            if game_ids.len() > 1 {
                // Launcher totals are aggregate seeds rather than individual
                // plays. When newly discovered identity evidence joins games,
                // keep the largest seed once; real tracked sessions survive.
                sqlx::query("DELETE FROM play_sessions WHERE game_id=? AND json_extract(tracking_json,'$.discoverySeed.aggregate')=1 AND id NOT IN (SELECT id FROM play_sessions WHERE game_id=? AND json_extract(tracking_json,'$.discoverySeed.aggregate')=1 ORDER BY duration_ms DESC,started_at_ms DESC,id LIMIT 1)")
                    .bind(&game_id).bind(&game_id).execute(&mut *tx).await?;
            }
        }
        let mut entry_ids = Vec::new();
        let mut seed_entry = None;
        for (mut entry, old) in entries.into_iter().zip(previous) {
            let proposed_id = entry.id.clone();
            entry.game_id = game_id.clone();
            if let Some(old) = old {
                entry.id = old.id;
                // An OS/launcher refresh must not demote a managed install to
                // an external path, losing its root/alias relocation contract.
                if matches!(old.location, Some(InstallLocation::Managed { .. }))
                    && !matches!(entry.location, Some(InstallLocation::Managed { .. }))
                {
                    entry.location = old.location;
                    entry.alias_id = old.alias_id;
                    entry.launch = old.launch;
                    entry.runtime = old.runtime;
                    entry.tracking = old.tracking;
                }
            }
            if history.is_some_and(|h| h.entry_id == proposed_id) {
                seed_entry = Some(entry.id.clone());
            }
            save_entry(&mut tx, &entry).await?;
            entry_ids.push(entry.id);
        }
        for id in retiring {
            let open: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM play_sessions WHERE entry_id=? AND ended_at_ms IS NULL)",
            )
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
            if open {
                continue;
            }
            // Both rows belong to `game_id` now, so the composite FK holds.
            hand_over(&mut tx, &id, &entry_ids[0]).await?;
            sqlx::query("DELETE FROM game_entries WHERE id=?")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
        }
        let marker = format!("discovery.imported.{game_id}");
        let first = sqlx::query("INSERT OR IGNORE INTO settings(key,value_json) VALUES(?,'true')")
            .bind(marker)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            == 1;
        let mut seeded_history = false;
        if first && let (Some(history), Some(entry_id)) = (history, seed_entry) {
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM play_sessions WHERE game_id=?")
                    .bind(&game_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if count == 0 {
                // A launcher's total is reported play time, not a session
                // (#169): it goes with the release, and counts once.
                let source = history
                    .tracking
                    .as_ref()
                    .and_then(|t| t.pointer("/discoverySeed/source")?.as_str())
                    .unwrap_or("launcher")
                    .to_owned();
                let total = super::playtime::RemoteTotal {
                    entry_id,
                    total_ms: Some(history.duration_ms),
                    last_played_ms: Some(history.started_at_ms).filter(|at| *at > 0),
                };
                super::playtime::seed(&mut tx, &game_id, &source, &total, history.heartbeat_at_ms)
                    .await?;
                seeded_history = true;
            }
        }
        tx.commit().await?;
        Ok(ImportedGroup {
            game_id,
            created,
            entry_ids,
            seeded_history,
        })
    }

    /// Takes `source`'s entries out of the library, for a source the user
    /// turned off. Each entry's plays, launch actions and achievement links
    /// move to another entry of its game first; a game with nothing else
    /// left goes with it. Caskade's own installs and running games stay.
    pub async fn remove_source_entries(&self, source: &str) -> Result<Vec<SourceRemoval>> {
        let mut tx = self.begin_write().await?;
        let entries = sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM game_entries WHERE source_id=? ORDER BY external_id,id",
        )
        .bind(source)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(decode::<GameEntry>)
        .collect::<Result<Vec<_>>>()?;
        let mut changes: Vec<SourceRemoval> = Vec::new();
        for entry in entries {
            if matches!(entry.location, Some(InstallLocation::Managed { .. })) {
                continue;
            }
            let running: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM play_sessions WHERE game_id=? AND ended_at_ms IS NULL)",
            )
            .bind(&entry.game_id)
            .fetch_one(&mut *tx)
            .await?;
            if running {
                continue;
            }
            let heir: Option<String> = sqlx::query_scalar(
                "SELECT id FROM game_entries WHERE game_id=? AND source_id<>? ORDER BY state_kind='installed' DESC,id LIMIT 1",
            )
            .bind(&entry.game_id)
            .bind(source)
            .fetch_optional(&mut *tx)
            .await?;
            let change = match heir {
                Some(heir) => {
                    hand_over(&mut tx, &entry.id, &heir).await?;
                    sqlx::query("DELETE FROM game_entries WHERE id=?")
                        .bind(&entry.id)
                        .execute(&mut *tx)
                        .await?;
                    if let Some((mut game, overrides)) =
                        super::game_edit::load_layers(&mut tx, &entry.game_id).await?
                        && game.default_entry.as_deref() == Some(entry.id.as_str())
                    {
                        game.default_entry = None;
                        super::games::write_game(&mut tx, &game, Some(&overrides)).await?;
                    }
                    SourceRemoval::Updated(entry.game_id)
                }
                None => {
                    sqlx::query("DELETE FROM games WHERE id=?")
                        .bind(&entry.game_id)
                        .execute(&mut *tx)
                        .await?;
                    SourceRemoval::Removed(entry.game_id)
                }
            };
            // A game left with no entries replaces its earlier update.
            changes.retain(|c| c.game_id() != change.game_id());
            changes.push(change);
        }
        tx.commit().await?;
        Ok(changes)
    }

    pub async fn entries_for_source(&self, source: &str) -> Result<Vec<GameEntry>> {
        sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM game_entries WHERE source_id=? ORDER BY external_id,id",
        )
        .bind(source)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(decode)
        .collect()
    }
}

/// Moves what belongs to entry `from` (plays, the user's launch actions
/// (#177) and achievement links) to `to`, another entry of the same game.
async fn hand_over(tx: &mut Transaction<'_, Sqlite>, from: &str, to: &str) -> Result<()> {
    sqlx::query("UPDATE play_sessions SET entry_id=? WHERE entry_id=?")
        .bind(to)
        .bind(from)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE game_actions SET entry_id=?1,record_json=json_set(record_json,'$.entryId',?1) WHERE entry_id=?2")
        .bind(to)
        .bind(from)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE OR IGNORE entry_action_settings SET entry_id=? WHERE entry_id=?")
        .bind(to)
        .bind(from)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE OR IGNORE achievement_links SET entry_id=? WHERE entry_id=?")
        .bind(to)
        .bind(from)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
