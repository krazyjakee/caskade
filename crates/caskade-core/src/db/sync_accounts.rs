//! Each library starts with a default admin, "Player" (migration 0014). Until
//! someone changes it, it stands for "whoever set this device up", and two
//! devices' stand-ins are the same household (#273):
//!
//! - an untouched default admin never travels as an account, only as a
//!   marker (`{"placeholder": true}`), so another device never shows a
//!   second "Player";
//! - what it owns that travels (a collection, a save version) is given, on
//!   the other device, to that device's household admin: its own stand-in,
//!   or else its oldest admin;
//! - a device's stand-in joins the first admin that arrives from another
//!   device: everything it owns moves to that admin, and it's removed.
//!
//! Once changed (renamed, a PIN, an avatar), it's an account like any
//! other and travels as one.

use serde_json::{Value, json};
use sqlx::SqliteConnection;

use super::LibraryStore;
use super::sync_log::field;
use crate::Result;
use crate::sync::change::{EXISTS, Entity};

/// SQL: whether account row `a` is an untouched default admin.
const UNTOUCHED: &str = "a.name='Player' AND a.avatar='avatar:default' AND a.role='admin' \
    AND a.pin_hash IS NULL AND a.max_age IS NULL AND a.unrated_needs_pin=0 \
    AND a.created_at_ms=a.updated_at_ms";

/// What an untouched default admin travels as.
pub fn marker() -> Value {
    json!({ "placeholder": true })
}

/// Whether `value` (an account's `""` field) is a stand-in's marker.
pub fn is_marker(value: &Value) -> bool {
    value.get("placeholder").and_then(Value::as_bool) == Some(true)
}

/// The untouched default admins here.
pub(super) async fn untouched(conn: &mut SqliteConnection) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT a.id FROM accounts a WHERE {UNTOUCHED} ORDER BY a.created_at_ms,a.id"
    )))
    .fetch_all(conn)
    .await?)
}

/// This device's household admin, other than `except`: its stand-in, or its
/// oldest admin.
async fn household(conn: &mut SqliteConnection, except: &str) -> Result<Option<String>> {
    if let Some(own) = untouched(conn).await?.into_iter().find(|id| id != except) {
        return Ok(Some(own));
    }
    Ok(sqlx::query_scalar(
        "SELECT id FROM accounts WHERE role='admin' AND id<>? ORDER BY created_at_ms,id LIMIT 1",
    )
    .bind(except)
    .fetch_optional(conn)
    .await?)
}

/// Gives everything `from` owns here to `into`.
async fn move_things(conn: &mut SqliteConnection, from: &str, into: &str) -> Result<()> {
    for sql in [
        "UPDATE collections SET account_id=?2 WHERE account_id=?1",
        "UPDATE OR IGNORE account_game_approvals SET account_id=?2 WHERE account_id=?1",
        "DELETE FROM account_game_approvals WHERE account_id=?1",
        "UPDATE store_accounts SET owner_id=?2 WHERE owner_id=?1",
        "UPDATE save_versions SET account_id=?2 WHERE account_id=?1",
        "UPDATE OR IGNORE save_heads SET account_id=?2 WHERE account_id=?1",
        "DELETE FROM save_heads WHERE account_id=?1",
    ] {
        sqlx::query(sql)
            .bind(from)
            .bind(into)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Another device's account `id` is a stand-in: a row for it here (from
/// before stand-ins stayed home) hands what it owns to this device's
/// household admin and goes.
pub(super) async fn retire(conn: &mut SqliteConnection, id: &str) -> Result<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?)")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if !exists {
        return Ok(());
    }
    let Some(into) = household(conn, id).await? else {
        return Ok(());
    };
    move_things(conn, id, &into).await?;
    sqlx::query("DELETE FROM accounts WHERE id=?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

impl LibraryStore {
    /// The account here that another device's account `id` is: itself, or
    /// this device's household admin when `id` is another device's
    /// stand-in. `None` until the account arrives.
    pub async fn sync_local_account(&self, id: &str) -> Result<Option<String>> {
        let mut conn = self.pool.acquire().await?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?)")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
        if exists {
            return Ok(Some(id.to_owned()));
        }
        let stand_in = field(&mut conn, Entity::Account, id, EXISTS)
            .await?
            .and_then(|state| state.value)
            .is_some_and(|value| is_marker(&value));
        if !stand_in {
            return Ok(None);
        }
        household(&mut conn, id).await
    }

    /// This device's stand-in joins the first admin that came from another
    /// device (`device` is this one): what it owns moves there and it's
    /// removed. Returns the two, when it happened.
    pub async fn sync_join_stand_in(&self, device: &str) -> Result<Option<(String, String)>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut own = None;
        for id in untouched(&mut tx).await? {
            let from_elsewhere = field(&mut tx, Entity::Account, &id, EXISTS)
                .await?
                .is_some_and(|state| state.stamp.origin != device);
            if !from_elsewhere {
                own = Some(id);
                break;
            }
        }
        let Some(own) = own else {
            return Ok(None);
        };
        let into: Option<String> = sqlx::query_scalar(
            "SELECT a.id FROM accounts a JOIN sync_fields f ON f.entity=? AND f.key=a.id AND f.field=? \
             WHERE a.role='admin' AND a.id<>? AND f.origin<>? AND f.value_json='true' \
             ORDER BY a.created_at_ms,a.id LIMIT 1",
        )
        .bind(Entity::Account.as_str())
        .bind(EXISTS)
        .bind(&own)
        .bind(device)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(into) = into else {
            return Ok(None);
        };
        move_things(&mut tx, &own, &into).await?;
        sqlx::query("DELETE FROM accounts WHERE id=?")
            .bind(&own)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some((own, into)))
    }
}
