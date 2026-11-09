//! The notification feed (#198): items, members, read state and the game
//! additions journal. The service and the rules live in
//! `crate::notifications`.
//!
//! Writes that read first take `BEGIN IMMEDIATE`, so an import committing
//! meanwhile makes them wait rather than fail. Read state changes are one
//! statement each.

use crate::notifications::{
    BATCH_MS, MemberKind, MemberPage, MemberRow, MemberState, Notice, Notification,
    NotificationCategory, NotificationMember, NotificationPage, NotificationTarget, Severity,
    batch_title, source_label, source_name, source_text, sources_body,
};
use caskade_core::{
    Result,
    bus::SourceCount,
    db::LibraryStore,
    i18n::{Message, Text},
    model::new_id,
};
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};

/// A recorded addition: game id, when, and the game's title, hidden flag,
/// first entry id and that entry's source id.
type AdditionRow = (
    String,
    i64,
    Option<String>,
    Option<bool>,
    Option<String>,
    Option<String>,
);
/// A batch member as queried: kind, label, source, when, game id, title,
/// merged-into game id and its title.
type MemberQueryRow = (
    String,
    String,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// What one pass over the additions journal did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ingested {
    /// Additions put in a batch.
    pub added: usize,
    /// Additions skipped: merged away, removed or hidden before they were seen.
    pub skipped: usize,
    /// Additions left for later: their game has no entry yet.
    pub waiting: usize,
}

type ItemRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
    String,
    String,
    i64,
    i64,
    Option<i64>,
    String,
);

const ITEM: &str = "SELECT id,category,severity,title,body,created_at_ms,updated_at_ms,\
     first_at_ms,last_at_ms,count,sources_json,target_json,version,read_version,resolved_at_ms,\
     json_array(title_message,body_message) FROM notifications";

/// A notice's title and body as stored: the English, and the messages
/// translated when the feed is read (#214).
struct Stored {
    title: String,
    body: String,
    title_message: String,
    body_message: Option<String>,
}

impl Stored {
    fn of(title: &Message, body: &Text) -> Result<Self> {
        Ok(Self {
            title: title.source(),
            body: body.source(),
            title_message: serde_json::to_string(title)?,
            body_message: match body {
                Text::Message(message) => Some(serde_json::to_string(message)?),
                Text::Plain(_) => None,
            },
        })
    }
}

/// A stored message in the language in use; the English stored beside it
/// when there is none (items from before #214) or it can't be read.
fn shown(english: String, message: Option<&str>) -> String {
    message
        .and_then(|json| serde_json::from_str::<Message>(json).ok())
        .map_or(english, |message| message.render())
}

fn item(row: ItemRow) -> Result<Notification> {
    let (
        id,
        category,
        severity,
        title,
        body,
        created_at_ms,
        updated_at_ms,
        first_at_ms,
        last_at_ms,
        count,
        sources,
        target,
        version,
        read_version,
        resolved,
        messages,
    ) = row;
    // The messages as stored, as one column: a tuple row has at most 16.
    let (title_message, body_message): (Option<String>, Option<String>) =
        serde_json::from_str(&messages).unwrap_or_default();
    let mut sources: Vec<SourceCount> = serde_json::from_str(&sources).unwrap_or_default();
    for source in &mut sources {
        source.source = source_text(&source.source).render();
    }
    Ok(Notification {
        category: NotificationCategory::parse(&category).ok_or_else(|| {
            caskade_core::Error::Invalid(format!("notification {id} has category {category}"))
        })?,
        id,
        severity: Severity::parse(&severity),
        title: shown(title, title_message.as_deref()),
        body: shown(body, body_message.as_deref()),
        created_at_ms,
        updated_at_ms,
        first_at_ms,
        last_at_ms,
        count: u32::try_from(count).unwrap_or(0),
        sources,
        target: serde_json::from_str(&target)?,
        version,
        unread: read_version < version,
        resolved: resolved.is_some(),
    })
}

/// The feed's next revision.
async fn next_version(conn: &mut SqliteConnection) -> Result<i64> {
    Ok(
        sqlx::query_scalar("UPDATE notification_revision SET value=value+1 RETURNING value")
            .fetch_one(conn)
            .await?,
    )
}

/// The notification feed in the library database: what `Notifications`
/// reads and writes.
#[allow(
    async_fn_in_trait,
    reason = "implemented only for LibraryStore, whose futures callers see"
)]
pub trait NotificationStore {
    /// Folds up to `limit` unseen game additions into their hourly batches,
    /// oldest first, in one transaction with marking them seen: a crash or a
    /// second pass never counts one twice. A game merged away, removed or
    /// hidden before this pass isn't counted; one without an entry waits
    /// `grace_ms` for its entries, for its source.
    async fn ingest_game_additions(&self, now: i64, grace_ms: i64, limit: u32) -> Result<Ingested>;
    /// Adds `notice`, or updates the item with its key. The same title and
    /// body again changes nothing (repeated identical failures). Otherwise
    /// it takes a new version, so it is unread again, and is no longer
    /// resolved. Whether anything changed.
    async fn upsert_notice(&self, notice: &Notice, now: i64) -> Result<bool>;
    /// The problem behind the item with `key` cleared: it is resolved and
    /// read. Whether it changed.
    async fn resolve_notice(&self, key: &str, now: i64) -> Result<bool>;
    /// Makes the members of the grouped item with `notice.key` exactly
    /// `members`. A member it didn't have makes it unread again; members
    /// that left (an update installed or skipped) don't, and none left
    /// resolves it. Repeating the same set changes nothing.
    async fn sync_notice_members(
        &self,
        notice: &Notice,
        members: &[MemberRow],
        now: i64,
    ) -> Result<bool>;
    /// A page of the feed, newest change first; only unread items with
    /// `unread_only`.
    async fn notifications(
        &self,
        unread_only: bool,
        offset: u32,
        limit: u32,
    ) -> Result<NotificationPage>;
    async fn notification(&self, id: &str) -> Result<Option<Notification>>;
    async fn unread_notifications(&self) -> Result<u32>;
    /// Marks an item read up to `version`, the one that was shown: a change
    /// since keeps it unread. Never moves backwards. Whether it changed.
    async fn mark_notification_read(&self, id: &str, version: i64) -> Result<bool>;
    async fn mark_notification_unread(&self, id: &str) -> Result<bool>;
    /// Marks read every item whose last change is at or before `revision`,
    /// the revision the feed was read at. How many changed.
    async fn mark_all_notifications_read(&self, revision: i64) -> Result<u64>;
    /// Deletes read items last changed before `before`, and seen additions
    /// of games long gone. Unread items stay however old. How many items went.
    async fn prune_notifications(&self, before: i64) -> Result<u64>;
    /// A page of what an item is about, oldest first, each followed to the
    /// game it is now: a game merged since points at the game it joined.
    async fn notification_members(&self, id: &str, offset: u32, limit: u32) -> Result<MemberPage>;
    /// Test support: moves an addition in time, as if it happened at `at`.
    #[cfg(test)]
    async fn set_addition_time(&self, game_id: &str, at: i64) -> Result<()>;
}

impl NotificationStore for LibraryStore {
    async fn ingest_game_additions(&self, now: i64, grace_ms: i64, limit: u32) -> Result<Ingested> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let rows: Vec<AdditionRow> =
            sqlx::query_as(
                "SELECT a.game_id,a.added_at_ms,g.title,g.hidden,\
                 (SELECT e.id FROM game_entries e WHERE e.game_id=a.game_id ORDER BY e.id LIMIT 1),\
                 (SELECT e.source_id FROM game_entries e WHERE e.game_id=a.game_id ORDER BY e.id LIMIT 1) \
                 FROM game_additions a LEFT JOIN games g ON g.id=a.game_id \
                 WHERE a.processed=0 ORDER BY a.added_at_ms,a.game_id LIMIT ?",
            )
            .bind(i64::from(limit))
            .fetch_all(&mut *tx)
            .await?;
        let mut done = Ingested::default();
        let mut touched = BTreeSet::new();
        for (game_id, added_at, title, hidden, entry, source) in rows {
            let Some(title) = title.filter(|_| hidden != Some(true)) else {
                done.skipped += 1;
                sqlx::query("UPDATE game_additions SET processed=1 WHERE game_id=?")
                    .bind(&game_id)
                    .execute(&mut *tx)
                    .await?;
                continue;
            };
            if entry.is_none() && now.saturating_sub(added_at) < grace_ms {
                done.waiting += 1;
                continue;
            }
            let batch: Option<String> = sqlx::query_scalar(
                "SELECT id FROM notifications WHERE category='games.added' \
                 AND first_at_ms<=?1 AND ?1<first_at_ms+?2 ORDER BY first_at_ms DESC LIMIT 1",
            )
            .bind(added_at)
            .bind(BATCH_MS)
            .fetch_optional(&mut *tx)
            .await?;
            let batch = match batch {
                Some(id) => id,
                None => {
                    let id = new_id();
                    sqlx::query(
                        "INSERT INTO notifications(id,category,severity,group_key,title,body,\
                         created_at_ms,updated_at_ms,first_at_ms,last_at_ms,target_json) \
                         VALUES(?,'games.added','info',?,'','',?,?,?,?,?)",
                    )
                    .bind(&id)
                    .bind(format!("games.added:{added_at}:{id}"))
                    .bind(now)
                    .bind(now)
                    .bind(added_at)
                    .bind(added_at)
                    .bind(serde_json::to_string(&NotificationTarget::Members)?)
                    .execute(&mut *tx)
                    .await?;
                    id
                }
            };
            let inserted = sqlx::query(
                "INSERT OR IGNORE INTO notification_members(notification_id,member,kind,game_id,\
                 entry_id,source,label,at_ms) VALUES(?,?,'game',?,?,?,?,?)",
            )
            .bind(&batch)
            .bind(&game_id)
            .bind(&game_id)
            .bind(&entry)
            .bind(source.as_deref().unwrap_or("other"))
            .bind(&title)
            .bind(added_at)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if inserted > 0 {
                done.added += 1;
                touched.insert(batch);
            }
            sqlx::query("UPDATE game_additions SET processed=1 WHERE game_id=?")
                .bind(&game_id)
                .execute(&mut *tx)
                .await?;
        }
        for batch in touched {
            let members: Vec<(String, String)> = sqlx::query_as(
                "SELECT source,label FROM notification_members WHERE notification_id=?",
            )
            .bind(&batch)
            .fetch_all(&mut *tx)
            .await?;
            let mut counts = BTreeMap::<String, u32>::new();
            for (source, _) in &members {
                *counts.entry(source_name(source)).or_default() += 1;
            }
            let mut sources: Vec<SourceCount> = counts
                .into_iter()
                .map(|(source, count)| SourceCount { source, count })
                .collect();
            sources.sort_by(|a, b| b.count.cmp(&a.count).then(a.source.cmp(&b.source)));
            let count = u32::try_from(members.len()).unwrap_or(u32::MAX);
            let only = members.first().map(|(_, label)| label.as_str());
            let text = Stored::of(&batch_title(count, only), &sources_body(&sources))?;
            let version = next_version(&mut tx).await?;
            sqlx::query(
                "UPDATE notifications SET title=?1,body=?2,count=?3,sources_json=?4,\
                 updated_at_ms=?5,last_at_ms=(SELECT max(at_ms) FROM notification_members \
                 WHERE notification_id=?6),version=?7,resolved_at_ms=NULL,title_message=?8,\
                 body_message=?9 WHERE id=?6",
            )
            .bind(&text.title)
            .bind(&text.body)
            .bind(i64::from(count))
            .bind(serde_json::to_string(&sources)?)
            .bind(now)
            .bind(&batch)
            .bind(version)
            .bind(&text.title_message)
            .bind(&text.body_message)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(done)
    }

    async fn upsert_notice(&self, notice: &Notice, now: i64) -> Result<bool> {
        let text = Stored::of(&notice.title, &notice.body)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: Option<(String, String, Option<i64>)> =
            sqlx::query_as("SELECT title,body,resolved_at_ms FROM notifications WHERE group_key=?")
                .bind(&notice.key)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some((title, body, None)) = &current
            && *title == text.title
            && *body == text.body
        {
            // The same English: nothing new to read. The messages are
            // stored anyway, for an item from before they were.
            sqlx::query(
                "UPDATE notifications SET title_message=?,body_message=? WHERE group_key=?",
            )
            .bind(&text.title_message)
            .bind(&text.body_message)
            .bind(&notice.key)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(false);
        }
        let version = next_version(&mut tx).await?;
        let target = serde_json::to_string(&notice.target)?;
        sqlx::query(
            "INSERT INTO notifications(id,category,severity,group_key,title,body,created_at_ms,\
             updated_at_ms,first_at_ms,last_at_ms,count,target_json,version,title_message,\
             body_message) VALUES(?,?,?,?,?,?,?,?,?,?,1,?,?,?,?) ON CONFLICT(group_key) DO UPDATE SET \
             severity=excluded.severity,title=excluded.title,body=excluded.body,\
             updated_at_ms=excluded.updated_at_ms,last_at_ms=excluded.last_at_ms,\
             target_json=excluded.target_json,version=excluded.version,resolved_at_ms=NULL,\
             title_message=excluded.title_message,body_message=excluded.body_message",
        )
        .bind(new_id())
        .bind(notice.category.as_str())
        .bind(notice.severity.as_str())
        .bind(&notice.key)
        .bind(&text.title)
        .bind(&text.body)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(target)
        .bind(version)
        .bind(&text.title_message)
        .bind(&text.body_message)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn resolve_notice(&self, key: &str, now: i64) -> Result<bool> {
        Ok(sqlx::query(
            "UPDATE notifications SET resolved_at_ms=?,read_version=version \
             WHERE group_key=? AND resolved_at_ms IS NULL",
        )
        .bind(now)
        .bind(key)
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0)
    }

    async fn sync_notice_members(
        &self,
        notice: &Notice,
        members: &[MemberRow],
        now: i64,
    ) -> Result<bool> {
        let text = Stored::of(&notice.title, &notice.body)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: Option<(String, Option<i64>)> =
            sqlx::query_as("SELECT id,resolved_at_ms FROM notifications WHERE group_key=?")
                .bind(&notice.key)
                .fetch_optional(&mut *tx)
                .await?;
        let had: BTreeSet<String> = match &current {
            Some((id, _)) => sqlx::query_scalar(
                "SELECT member FROM notification_members WHERE notification_id=?",
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect(),
            None => BTreeSet::new(),
        };
        let wanted: BTreeSet<&str> = members.iter().map(|m| m.key.as_str()).collect();
        let arrived = members.iter().any(|m| !had.contains(&m.key));
        let left = had.iter().any(|key| !wanted.contains(key.as_str()));
        if !arrived && !left {
            return Ok(false);
        }
        let id = match current {
            Some((id, _)) => id,
            None => {
                let id = new_id();
                sqlx::query(
                    "INSERT INTO notifications(id,category,severity,group_key,title,body,\
                     created_at_ms,updated_at_ms,first_at_ms,last_at_ms,target_json) \
                     VALUES(?,?,?,?,'','',?,?,?,?,?)",
                )
                .bind(&id)
                .bind(notice.category.as_str())
                .bind(notice.severity.as_str())
                .bind(&notice.key)
                .bind(now)
                .bind(now)
                .bind(now)
                .bind(now)
                .bind(serde_json::to_string(&notice.target)?)
                .execute(&mut *tx)
                .await?;
                id
            }
        };
        for key in had.iter().filter(|key| !wanted.contains(key.as_str())) {
            sqlx::query("DELETE FROM notification_members WHERE notification_id=? AND member=?")
                .bind(&id)
                .bind(key)
                .execute(&mut *tx)
                .await?;
        }
        for member in members.iter().filter(|m| !had.contains(&m.key)) {
            sqlx::query(
                "INSERT OR IGNORE INTO notification_members(notification_id,member,kind,game_id,\
                 entry_id,source,label,at_ms) VALUES(?,?,?,?,?,NULL,?,?)",
            )
            .bind(&id)
            .bind(&member.key)
            .bind(member.kind.as_str())
            .bind(&member.game_id)
            .bind(&member.entry_id)
            .bind(&member.label)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        let count = u32::try_from(members.len()).unwrap_or(u32::MAX);
        if arrived {
            let version = next_version(&mut tx).await?;
            sqlx::query(
                "UPDATE notifications SET title=?,body=?,count=?,updated_at_ms=?,last_at_ms=?,\
                 version=?,resolved_at_ms=NULL,title_message=?,body_message=? WHERE id=?",
            )
            .bind(&text.title)
            .bind(&text.body)
            .bind(i64::from(count))
            .bind(now)
            .bind(now)
            .bind(version)
            .bind(&text.title_message)
            .bind(&text.body_message)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        } else if members.is_empty() {
            sqlx::query(
                "UPDATE notifications SET count=0,resolved_at_ms=coalesce(resolved_at_ms,?),\
                 read_version=version WHERE id=?",
            )
            .bind(now)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        } else {
            // Fewer than before: the text follows without making it unread.
            sqlx::query(
                "UPDATE notifications SET title=?,body=?,count=?,title_message=?,body_message=? \
                 WHERE id=?",
            )
            .bind(&text.title)
            .bind(&text.body)
            .bind(i64::from(count))
            .bind(&text.title_message)
            .bind(&text.body_message)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    async fn notifications(
        &self,
        unread_only: bool,
        offset: u32,
        limit: u32,
    ) -> Result<NotificationPage> {
        let limit = limit.clamp(1, 200);
        // One read transaction: the page, the counts and the revision agree.
        let mut tx = self.pool.begin().await?;
        let revision: i64 = sqlx::query_scalar("SELECT value FROM notification_revision")
            .fetch_one(&mut *tx)
            .await?;
        let rows: Vec<ItemRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "{ITEM} WHERE (?1=0 OR read_version<version) \
             ORDER BY updated_at_ms DESC,id DESC LIMIT ?2 OFFSET ?3"
        )))
        .bind(unread_only)
        .bind(i64::from(limit))
        .bind(i64::from(offset))
        .fetch_all(&mut *tx)
        .await?;
        let (total, unread): (i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE ?1=0 OR read_version<version),\
             count(*) FILTER (WHERE read_version<version) FROM notifications",
        )
        .bind(unread_only)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(NotificationPage {
            // One unreadable row (a category a newer Caskade wrote, damaged
            // JSON) is left out, not the whole feed (#160).
            items: rows
                .into_iter()
                .filter_map(|row| {
                    item(row)
                        .inspect_err(|error| tracing::warn!(%error, "notification left out"))
                        .ok()
                })
                .collect(),
            total: u32::try_from(total).unwrap_or(u32::MAX),
            unread: u32::try_from(unread).unwrap_or(u32::MAX),
            revision,
        })
    }

    async fn notification(&self, id: &str) -> Result<Option<Notification>> {
        sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!("{ITEM} WHERE id=?")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(item)
            .transpose()
    }

    async fn unread_notifications(&self) -> Result<u32> {
        let unread: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notifications WHERE read_version<version")
                .fetch_one(&self.pool)
                .await?;
        Ok(u32::try_from(unread).unwrap_or(u32::MAX))
    }

    async fn mark_notification_read(&self, id: &str, version: i64) -> Result<bool> {
        Ok(sqlx::query(
            "UPDATE notifications SET read_version=min(?1,version) \
             WHERE id=?2 AND read_version<min(?1,version)",
        )
        .bind(version)
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0)
    }

    async fn mark_notification_unread(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query(
            "UPDATE notifications SET read_version=0 WHERE id=? AND read_version>=version",
        )
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0)
    }

    async fn mark_all_notifications_read(&self, revision: i64) -> Result<u64> {
        Ok(sqlx::query(
            "UPDATE notifications SET read_version=version \
             WHERE read_version<version AND version<=?",
        )
        .bind(revision)
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

    async fn prune_notifications(&self, before: i64) -> Result<u64> {
        let deleted = sqlx::query(
            "DELETE FROM notifications WHERE read_version>=version AND updated_at_ms<?",
        )
        .bind(before)
        .execute(&self.pool)
        .await?
        .rows_affected();
        sqlx::query(
            "DELETE FROM game_additions WHERE processed=1 AND added_at_ms<? \
             AND game_id NOT IN (SELECT id FROM games)",
        )
        .bind(before)
        .execute(&self.pool)
        .await?;
        Ok(deleted)
    }

    async fn notification_members(&self, id: &str, offset: u32, limit: u32) -> Result<MemberPage> {
        let limit = limit.clamp(1, 500);
        let rows: Vec<MemberQueryRow> = sqlx::query_as(
            "SELECT m.kind,m.label,m.source,m.at_ms,m.game_id,g.title,e.game_id,eg.title \
             FROM notification_members m \
             LEFT JOIN games g ON g.id=m.game_id \
             LEFT JOIN game_entries e ON e.id=m.entry_id \
             LEFT JOIN games eg ON eg.id=e.game_id \
             WHERE m.notification_id=? ORDER BY m.at_ms,m.member LIMIT ? OFFSET ?",
        )
        .bind(id)
        .bind(i64::from(limit))
        .bind(i64::from(offset))
        .fetch_all(&self.pool)
        .await?;
        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notification_members WHERE notification_id=?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
        let items = rows
            .into_iter()
            .map(
                |(kind, label, source, at_ms, game_id, title, joined, joined_title)| {
                    let kind = MemberKind::parse(&kind);
                    let (state, game_id, title) = match (game_id, title, joined, joined_title) {
                        (None, ..) => (MemberState::Other, None, None),
                        (Some(id), Some(title), ..) => {
                            (MemberState::Present, Some(id), Some(title))
                        }
                        (Some(_), None, Some(joined), Some(title)) => {
                            (MemberState::Merged, Some(joined), Some(title))
                        }
                        (Some(_), None, ..) => (MemberState::Removed, None, None),
                    };
                    NotificationMember {
                        kind,
                        label,
                        source: source.map(|source| source_label(&source)),
                        at_ms,
                        state,
                        game_id,
                        title,
                    }
                },
            )
            .collect();
        Ok(MemberPage {
            items,
            total: u32::try_from(total).unwrap_or(u32::MAX),
        })
    }

    #[cfg(test)]
    async fn set_addition_time(&self, game_id: &str, at: i64) -> Result<()> {
        sqlx::query("UPDATE game_additions SET added_at_ms=? WHERE game_id=?")
            .bind(at)
            .bind(game_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "{game} couldn't install"
msgstr "{game} kunde inte installeras"
"#;

    #[tokio::test]
    async fn notifications_from_before_messages_keep_their_english() {
        let store = LibraryStore::in_memory().await.unwrap();
        // A library from before migration 0018: English only.
        caskade_core::db::MIGRATOR
            .undo(&store.pool, 17)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO notifications(id,category,severity,group_key,title,body,created_at_ms,\
             updated_at_ms,first_at_ms,last_at_ms,count,target_json,version) \
             VALUES('old','job.failed','error','job.failed:j1','Celeste couldn''t install',\
             'disk full',1,1,1,1,1,'{\"type\":\"jobs\"}',1)",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        caskade_core::db::MIGRATOR.run(&store.pool).await.unwrap();

        let _sv = caskade_core::i18n::scope_catalog("sv", SV);
        let old = store.notification("old").await.unwrap().unwrap();
        assert_eq!(
            (old.title.as_str(), old.body.as_str()),
            ("Celeste couldn't install", "disk full")
        );
        assert!(old.unread);

        // The same failure again keeps the item as it was, read state
        // included, and from now on it is translated.
        let notice = Notice {
            category: NotificationCategory::JobFailed,
            severity: Severity::Error,
            key: "job.failed:j1".into(),
            title: Message::new(None, "{game} couldn't install", None, None).arg("game", "Celeste"),
            body: Text::Plain("disk full".into()),
            target: NotificationTarget::Jobs,
        };
        assert!(!store.upsert_notice(&notice, 2).await.unwrap());
        let now = store.notification("old").await.unwrap().unwrap();
        assert_eq!(now.title, "Celeste kunde inte installeras");
        assert_eq!((now.body.as_str(), now.version), ("disk full", old.version));
        store.close().await;
    }
}
