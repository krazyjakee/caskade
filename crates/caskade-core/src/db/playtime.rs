//! Play time stores and launchers report (#169), reconciled with the
//! sessions Caskade tracks. Sessions are never made up from a total: a
//! remote total is a `remote_playtime` row of its release, and the game's
//! play time is its sessions plus the rows' `counted_ms`.
//!
//! Each game's rows are recounted ([`recount`]) when a total arrives and
//! when games merge, per family: the store identity a total is about (its
//! release's store id, `store_accounts::key`):
//!
//! - In a family, totals of different accounts add up, and a launcher's own
//!   total on this computer (account `''`) overlaps them: the family counts
//!   the larger of the two.
//! - Each total counts its largest value seen (`max_total_ms`): a store
//!   that resets or lowers a total never lowers the game's play time.
//!   Unknown totals count nothing and aren't zero.
//! - Sessions Caskade tracked on the family's releases that ended before the
//!   family's newest total was read are inside that total already: the
//!   family adds only what it has beyond them, never less than nothing.
//!   Sessions after it count on their own until a newer total covers them.
//! - Totals of different families (a Steam copy and a GOG copy of one game)
//!   are different play and add up. Totals of releases that name no store
//!   are one family: the game's own play, reported by more than one
//!   launcher.
//!
//! Last played is the latest of the sessions' starts and the totals' last
//! plays, each as precise as its source.

use super::{LibraryStore, decode};
use crate::{Result, model::GameEntry};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};

/// Whether store totals are imported (`core.storeAccounts.playtimeImport`).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", rename = "PlaytimeImport")]
pub enum PlaytimeImport {
    /// Store totals are ignored, and those already imported removed.
    Never,
    /// A release's first total is kept, never updated.
    NewGames,
    /// Every refresh's total is kept.
    #[default]
    Always,
}

/// One total to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTotal {
    pub entry_id: String,
    pub total_ms: Option<u64>,
    pub last_played_ms: Option<i64>,
}

/// A game's play time and where it comes from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "PlaytimeBreakdown")]
pub struct PlaytimeBreakdown {
    pub total_ms: u64,
    /// Sessions Caskade tracked.
    pub tracked_ms: u64,
    /// What reported totals add beyond them.
    pub reported_ms: u64,
    pub last_played_ms: Option<i64>,
    pub sources: Vec<ReportedPlaytime>,
}

/// One reported total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "ReportedPlaytime")]
pub struct ReportedPlaytime {
    /// `steam`, `gog`, `epic`, or the launcher that reported it.
    pub provider: String,
    /// The store account's name; `None` for a launcher on this computer.
    pub account_name: Option<String>,
    /// `None`: the source doesn't say.
    pub total_ms: Option<u64>,
    /// What this total adds to the game.
    pub counted_ms: u64,
    pub last_played_ms: Option<i64>,
    pub observed_at_ms: i64,
}

/// Provider, account, account name, total, counted, last play, read at.
type SourceRow = (
    String,
    String,
    Option<String>,
    Option<i64>,
    i64,
    Option<i64>,
    i64,
);
/// Entry, provider, account, family, largest total, read at.
type CountRow = (String, String, String, Option<String>, Option<i64>, i64);

/// The family of totals whose releases name no store.
const LOCAL: &str = "local";

fn ms(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl LibraryStore {
    /// Records `totals` from `provider`'s `account` read at `observed_at_ms`
    /// under `mode`, and recounts their games. Returns the games whose play
    /// time or last play changed.
    pub async fn record_remote_playtime(
        &self,
        provider: &str,
        account: &str,
        totals: &[RemoteTotal],
        observed_at_ms: i64,
        mode: PlaytimeImport,
    ) -> Result<Vec<String>> {
        // Reads before it writes: IMMEDIATE waits for another writer
        // where a deferred transaction would fail as busy.
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut games = BTreeSet::new();
        for total in totals {
            let Some(game_id): Option<String> =
                sqlx::query_scalar("SELECT game_id FROM game_entries WHERE id=?")
                    .bind(&total.entry_id)
                    .fetch_optional(&mut *tx)
                    .await?
            else {
                continue;
            };
            if write(&mut tx, provider, account, total, observed_at_ms, mode).await? {
                games.insert(game_id);
            }
        }
        let mut changed = Vec::new();
        for game_id in games {
            let before = self.totals_in(&mut tx, &game_id).await?;
            recount(&mut tx, &game_id).await?;
            if self.totals_in(&mut tx, &game_id).await? != before {
                changed.push(game_id);
            }
        }
        tx.commit().await?;
        Ok(changed)
    }

    /// Removes every total `account` reported (play time import turned
    /// off), recounting their games; returns them.
    pub async fn forget_remote_playtime(&self, account: &str) -> Result<Vec<String>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let games: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT e.game_id FROM remote_playtime r JOIN game_entries e ON e.id=r.entry_id \
             WHERE r.account=?",
        )
        .bind(account)
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM remote_playtime WHERE account=?")
            .bind(account)
            .execute(&mut *tx)
            .await?;
        for game_id in &games {
            recount(&mut tx, game_id).await?;
        }
        tx.commit().await?;
        Ok(games)
    }

    async fn totals_in(
        &self,
        connection: &mut SqliteConnection,
        game_id: &str,
    ) -> Result<(i64, Option<i64>)> {
        Ok(sqlx::query_as(
            "SELECT coalesce(sum(r.counted_ms),0),max(r.last_played_ms) FROM remote_playtime r \
             JOIN game_entries e ON e.id=r.entry_id WHERE e.game_id=?",
        )
        .bind(game_id)
        .fetch_one(&mut *connection)
        .await?)
    }

    /// Where `game_id`'s play time comes from.
    pub async fn playtime_breakdown(&self, game_id: &str) -> Result<PlaytimeBreakdown> {
        let (tracked, session_last): (i64, Option<i64>) = sqlx::query_as(
            "SELECT coalesce(sum(duration_ms),0),max(started_at_ms) FROM play_sessions WHERE game_id=?",
        )
        .bind(game_id)
        .fetch_one(&self.pool)
        .await?;
        let rows: Vec<SourceRow> = sqlx::query_as(
            "SELECT r.provider,r.account,s.display_name,r.total_ms,r.counted_ms,r.last_played_ms,\
                 r.observed_at_ms FROM remote_playtime r JOIN game_entries e ON e.id=r.entry_id \
                 LEFT JOIN store_accounts s ON s.id=r.account WHERE e.game_id=? \
                 ORDER BY r.counted_ms DESC,r.provider,r.account",
        )
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?;
        let sources: Vec<ReportedPlaytime> = rows
            .into_iter()
            .map(
                |(provider, account, name, total, counted, last, observed)| ReportedPlaytime {
                    provider,
                    account_name: name.or_else(|| (!account.is_empty()).then_some(account)),
                    total_ms: total.map(ms),
                    counted_ms: ms(counted),
                    last_played_ms: last,
                    observed_at_ms: observed,
                },
            )
            .collect();
        let reported = sources
            .iter()
            .fold(0u64, |sum, source| sum.saturating_add(source.counted_ms));
        let tracked = ms(tracked);
        Ok(PlaytimeBreakdown {
            total_ms: tracked.saturating_add(reported),
            tracked_ms: tracked,
            reported_ms: reported,
            last_played_ms: sources
                .iter()
                .filter_map(|source| source.last_played_ms)
                .chain(session_last)
                .max(),
            sources,
        })
    }
}

/// Writes one total under `mode`; whether the row changed.
async fn write(
    connection: &mut SqliteConnection,
    provider: &str,
    account: &str,
    total: &RemoteTotal,
    observed_at_ms: i64,
    mode: PlaytimeImport,
) -> Result<bool> {
    if mode == PlaytimeImport::Never {
        return Ok(sqlx::query(
            "DELETE FROM remote_playtime WHERE entry_id=? AND provider=? AND account=?",
        )
        .bind(&total.entry_id)
        .bind(provider)
        .bind(account)
        .execute(&mut *connection)
        .await?
        .rows_affected()
            > 0);
    }
    if total.total_ms.is_none() && total.last_played_ms.is_none() {
        return Ok(false);
    }
    let old: Option<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT observed_at_ms,max_total_ms FROM remote_playtime \
         WHERE entry_id=? AND provider=? AND account=?",
    )
    .bind(&total.entry_id)
    .bind(provider)
    .bind(account)
    .fetch_optional(&mut *connection)
    .await?;
    match old {
        // New games only: the first total stays.
        Some(_) if mode == PlaytimeImport::NewGames => return Ok(false),
        // An older read than the one kept.
        Some((observed, _)) if observed > observed_at_ms => return Ok(false),
        _ => {}
    }
    let total_ms = total.total_ms.map(signed);
    let max_total = match (old.and_then(|(_, max)| max), total_ms) {
        (Some(old), Some(new)) => Some(old.max(new)),
        (old, new) => old.or(new),
    };
    sqlx::query(
        "INSERT INTO remote_playtime(entry_id,provider,account,total_ms,max_total_ms,last_played_ms,\
         observed_at_ms) VALUES (?,?,?,?,?,?,?) ON CONFLICT(entry_id,provider,account) DO UPDATE SET \
         total_ms=coalesce(excluded.total_ms,total_ms),max_total_ms=excluded.max_total_ms,\
         last_played_ms=coalesce(excluded.last_played_ms,last_played_ms),\
         observed_at_ms=excluded.observed_at_ms",
    )
    .bind(&total.entry_id)
    .bind(provider)
    .bind(account)
    .bind(total_ms)
    .bind(max_total)
    .bind(total.last_played_ms)
    .bind(observed_at_ms)
    .execute(&mut *connection)
    .await?;
    Ok(true)
}

/// A launcher's own total discovery found on a game's first import.
pub(super) async fn seed(
    connection: &mut SqliteConnection,
    game_id: &str,
    source: &str,
    total: &RemoteTotal,
    observed_at_ms: i64,
) -> Result<()> {
    write(
        connection,
        source,
        "",
        total,
        observed_at_ms,
        PlaytimeImport::NewGames,
    )
    .await?;
    recount(connection, game_id).await
}

/// Recounts `game_id`'s reported totals (see the module's rules).
pub(super) async fn recount(connection: &mut SqliteConnection, game_id: &str) -> Result<()> {
    // Each release's store identities.
    let entries: Vec<String> =
        sqlx::query_scalar("SELECT record_json FROM game_entries WHERE game_id=?")
            .bind(game_id)
            .fetch_all(&mut *connection)
            .await?;
    let mut keys: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for record in entries {
        let entry: GameEntry = decode(record)?;
        let mut ids = BTreeSet::new();
        for id in &entry.ids {
            if let Some(key) = super::store_accounts::key(id)? {
                ids.insert(key);
            }
        }
        keys.insert(entry.id, ids);
    }
    struct Row {
        entry_id: String,
        provider: String,
        account: String,
        family: String,
        max_total: Option<u64>,
        observed: i64,
    }
    let rows: Vec<CountRow> = sqlx::query_as(
        "SELECT r.entry_id,r.provider,r.account,r.family,r.max_total_ms,r.observed_at_ms \
         FROM remote_playtime r JOIN game_entries e ON e.id=r.entry_id WHERE e.game_id=?",
    )
    .bind(game_id)
    .fetch_all(&mut *connection)
    .await?;
    let rows: Vec<Row> = rows
        .into_iter()
        .map(
            |(entry_id, provider, account, family, max_total, observed)| {
                // A total belongs to its release's first store identity.
                // Totals of releases without one (launchers that name no
                // store) are the game's own play: they overlap one another.
                let family = family
                    .filter(|family| keys.get(&entry_id).is_some_and(|k| k.contains(family)))
                    .or_else(|| keys.get(&entry_id)?.iter().next().cloned())
                    .unwrap_or_else(|| LOCAL.to_owned());
                Row {
                    entry_id,
                    provider,
                    account,
                    family,
                    max_total: max_total.map(ms),
                    observed,
                }
            },
        )
        .collect();
    let sessions: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT entry_id,ended_at_ms,duration_ms FROM play_sessions \
         WHERE game_id=? AND ended_at_ms IS NOT NULL",
    )
    .bind(game_id)
    .fetch_all(&mut *connection)
    .await?;
    let mut families: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for row in &rows {
        families.entry(row.family.as_str()).or_default().push(row);
    }
    let mut counted: BTreeMap<(&str, &str, &str), u64> = BTreeMap::new();
    for (family, members) in families {
        let launcher = members
            .iter()
            .filter(|row| row.account.is_empty())
            .filter_map(|row| row.max_total.map(|total| (total, *row)))
            .max_by_key(|(total, _)| *total);
        let accounts: u64 = members
            .iter()
            .filter(|row| !row.account.is_empty())
            .filter_map(|row| row.max_total)
            .fold(0u64, u64::saturating_add);
        let observed = members.iter().map(|row| row.observed).max().unwrap_or(0);
        let covered = sessions
            .iter()
            .filter(|(entry, ended, _)| {
                *ended <= observed
                    && match keys.get(entry) {
                        Some(k) if !k.is_empty() => k.contains(family),
                        _ => family == LOCAL,
                    }
            })
            .fold(0u64, |sum, (_, _, duration)| {
                sum.saturating_add(ms(*duration))
            });
        // The account totals, or the launcher's when larger: never both.
        let (total, lead) = match launcher {
            Some((launcher_total, row)) if launcher_total > accounts => (launcher_total, Some(row)),
            _ => (
                accounts,
                members
                    .iter()
                    .filter(|row| !row.account.is_empty())
                    .max_by_key(|row| row.max_total.unwrap_or(0))
                    .copied(),
            ),
        };
        if let Some(lead) = lead {
            counted.insert(
                (&lead.entry_id, &lead.provider, &lead.account),
                total.saturating_sub(covered),
            );
        }
    }
    for row in &rows {
        let value = counted
            .get(&(
                row.entry_id.as_str(),
                row.provider.as_str(),
                row.account.as_str(),
            ))
            .copied()
            .unwrap_or(0);
        sqlx::query(
            "UPDATE remote_playtime SET counted_ms=?,family=? WHERE entry_id=? AND provider=? AND account=?",
        )
        .bind(signed(value))
        .bind(&row.family)
        .bind(&row.entry_id)
        .bind(&row.provider)
        .bind(&row.account)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "playtime_tests.rs"]
mod tests;
