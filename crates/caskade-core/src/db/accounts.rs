//! Local accounts (#202): rows, PIN hashes and the wrong-PIN throttle. Who
//! may change what is decided in `crate::accounts`; the rules every change
//! must leave intact are checked here, in the change's transaction.
use super::{LibraryStore, decode, missing};
use crate::{
    Error, Result,
    accounts::{Account, AccountRole, Permissions, Throttle},
    model::validation as v,
};

/// An account with what never leaves the core.
pub(crate) struct AccountRow {
    pub account: Account,
    pub pin_hash: Option<String>,
    pub throttle: Throttle,
}

const COLUMNS: &str = "id,name,avatar,role,pin_hash,permissions_json,max_age,unrated_needs_pin,pin_failures,pin_lockouts,pin_locked_until_ms,created_at_ms,updated_at_ms";
type Row = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<i64>,
    bool,
    i64,
    i64,
    i64,
    i64,
    i64,
);

impl LibraryStore {
    pub(crate) async fn game_age_approved(&self, account_id: &str, game_id: &str) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM account_game_approvals WHERE account_id=? AND game_id=?)",
        )
        .bind(account_id)
        .bind(game_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub(crate) async fn approve_game_age(&self, account_id: &str, game_id: &str) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO account_game_approvals(account_id,game_id) VALUES(?,?)")
            .bind(account_id)
            .bind(game_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Every account: admins first, then by name.
    pub async fn accounts(&self) -> Result<Vec<Account>> {
        Ok(self
            .account_rows()
            .await?
            .into_iter()
            .map(|row| row.account)
            .collect())
    }
    pub async fn get_account(&self, id: &str) -> Result<Option<Account>> {
        Ok(self.account_row(id).await?.map(|row| row.account))
    }
    pub(crate) async fn account_rows(&self) -> Result<Vec<AccountRow>> {
        sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM accounts ORDER BY role='admin' DESC,name COLLATE NOCASE,id"
        )))
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(account_from_row)
        .collect()
    }
    pub(crate) async fn account_row(&self, id: &str) -> Result<Option<AccountRow>> {
        sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM accounts WHERE id=?"
        )))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .map(account_from_row)
        .transpose()
    }
    pub(crate) async fn insert_account(
        &self,
        account: &Account,
        pin_hash: Option<&str>,
    ) -> Result<()> {
        v::id(&account.id)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO accounts(id,name,avatar,role,pin_hash,permissions_json,max_age,unrated_needs_pin,created_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(&account.id)
            .bind(&account.name)
            .bind(&account.avatar)
            .bind(role_text(account.role))
            .bind(pin_hash)
            .bind(v::json(&account.permissions)?)
            .bind(account.max_age.map(i64::from))
            .bind(account.unrated_needs_pin)
            .bind(account.created_at_ms)
            .bind(account.updated_at_ms)
            .execute(&mut *tx)
            .await?;
        check_accounts(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
    /// Writes an account's name, avatar, role, permissions and age limit.
    pub(crate) async fn update_account(&self, account: &Account) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query("UPDATE accounts SET name=?,avatar=?,role=?,permissions_json=?,max_age=?,unrated_needs_pin=?,updated_at_ms=? WHERE id=?")
            .bind(&account.name)
            .bind(&account.avatar)
            .bind(role_text(account.role))
            .bind(v::json(&account.permissions)?)
            .bind(account.max_age.map(i64::from))
            .bind(account.unrated_needs_pin)
            .bind(account.updated_at_ms)
            .bind(&account.id)
            .execute(&mut *tx)
            .await?;
        if updated.rows_affected() == 0 {
            return Err(missing("account", &account.id));
        }
        check_accounts(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
    /// Sets or clears an account's PIN hash, and forgets wrong tries.
    pub(crate) async fn set_account_pin(
        &self,
        id: &str,
        pin_hash: Option<&str>,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query("UPDATE accounts SET pin_hash=?,pin_failures=0,pin_lockouts=0,pin_locked_until_ms=0,updated_at_ms=? WHERE id=?")
            .bind(pin_hash)
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if updated.rows_affected() == 0 {
            return Err(missing("account", id));
        }
        check_accounts(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
    /// Deletes an account and, through the reference, what it owns: its
    /// collections and their members, in the same transaction.
    pub(crate) async fn delete_account(&self, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let deleted = sqlx::query("DELETE FROM accounts WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        check_accounts(&mut tx).await?;
        tx.commit().await?;
        Ok(deleted)
    }
    pub(crate) async fn save_pin_throttle(&self, id: &str, throttle: &Throttle) -> Result<()> {
        sqlx::query(
            "UPDATE accounts SET pin_failures=?,pin_lockouts=?,pin_locked_until_ms=? WHERE id=?",
        )
        .bind(i64::from(throttle.failures))
        .bind(i64::from(throttle.lockouts))
        .bind(throttle.locked_until_ms)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// What every change must leave: an admin, and while standard accounts
/// exist, a PIN on every admin (an unlocked admin would open them all).
async fn check_accounts(tx: &mut sqlx::SqliteConnection) -> Result<()> {
    let (admins, standard, unlocked): (i64, i64, i64) = sqlx::query_as(
        "SELECT coalesce(sum(role='admin'),0),coalesce(sum(role='standard'),0),coalesce(sum(role='admin' AND pin_hash IS NULL),0) FROM accounts",
    )
    .fetch_one(&mut *tx)
    .await?;
    if admins == 0 {
        return Err(Error::Invalid(crate::tr!(
            "the last admin can't be removed or made standard"
        )));
    }
    if standard > 0 && unlocked > 0 {
        return Err(Error::Invalid(crate::tr!(
            "every admin needs a PIN while there are standard accounts"
        )));
    }
    Ok(())
}

fn role_text(role: AccountRole) -> &'static str {
    match role {
        AccountRole::Admin => "admin",
        AccountRole::Standard => "standard",
    }
}

fn account_from_row(
    (
        id,
        name,
        avatar,
        role,
        pin_hash,
        permissions,
        max_age,
        unrated_needs_pin,
        failures,
        lockouts,
        locked_until_ms,
        created_at_ms,
        updated_at_ms,
    ): Row,
) -> Result<AccountRow> {
    let role = if role == "admin" {
        AccountRole::Admin
    } else {
        AccountRole::Standard
    };
    let permissions = match role {
        AccountRole::Admin => Permissions::all(),
        AccountRole::Standard => decode(permissions)?,
    };
    let now = crate::now_ms();
    Ok(AccountRow {
        account: Account {
            id,
            name,
            avatar,
            role,
            has_pin: pin_hash.is_some(),
            permissions,
            max_age: max_age.and_then(|age| u8::try_from(age).ok()),
            unrated_needs_pin,
            locked_until_ms: (locked_until_ms > now).then_some(locked_until_ms),
            created_at_ms,
            updated_at_ms,
        },
        pin_hash,
        throttle: Throttle {
            failures: u32::try_from(failures).unwrap_or(0),
            lockouts: u32::try_from(lockouts).unwrap_or(0),
            locked_until_ms,
        },
    })
}
