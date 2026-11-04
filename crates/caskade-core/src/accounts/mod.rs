//! Local accounts (#202): who is using Caskade, what they may change, and
//! PIN checks. Accounts are local to the machine; the PIN is a household
//! lock against other people at the keyboard, not protection for the data
//! files (`docs/accounts.md`).
//!
//! [`AccountService`] holds the active account and decides who may change
//! what; the store keeps the rules every change must leave intact (an
//! admin remains; while standard accounts exist, every admin has a PIN).
//! A PIN never reaches logs, events, errors or the webview: only its
//! argon2id hash is stored, and checks answer [`PinCheck`].

mod age;
mod permission;
mod pin;
#[cfg(test)]
mod tests;

pub use age::AgeRestriction;
pub use permission::Permission;
pub(crate) use pin::Throttle;

use crate::{Error, Result, bus::Bus, db::LibraryStore, model::new_id, read, write};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, RwLock};

/// The avatars an account may pick: the set `@caskade/ui` ships
/// (`packages/ui/src/avatars.ts`), in the same order. An id keeps its image
/// across releases; a removed one would show initials. The first is the
/// one new accounts and the migrated admin get.
pub const AVATARS: &[&str] = &[
    DEFAULT_AVATAR,
    "avatar:astronaut",
    "avatar:droid",
    "avatar:rocket",
    "avatar:planet",
    "avatar:moon",
    "avatar:comet",
    "avatar:atom",
    "avatar:satellite",
    "avatar:ufo",
    "avatar:alien",
    "avatar:star",
    "avatar:galaxy",
    "avatar:pulsar",
    "avatar:crystal",
    "avatar:radar",
    "avatar:chip",
    "avatar:ghost",
    "avatar:mech",
    "avatar:bolt",
    "avatar:sentinel",
    "avatar:starfighter",
    "avatar:portal",
    "avatar:cybercat",
    "avatar:gamepad",
];
pub const DEFAULT_AVATAR: &str = "avatar:default";

/// Settings sections (the Settings tabs' ids) a standard account may be
/// allowed to change. Accounts management is for admins only.
pub const SETTINGS_SECTIONS: &[&str] = &[
    "general",
    "library-discovery",
    "launchers",
    "emulators",
    "appearance",
    "storage",
    "installer",
    "plugins",
];

const NAME_MAX: usize = 64;
/// The highest minimum age an age limit can name.
const MAX_AGE: u8 = 99;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AccountRole {
    /// Changes everything, other accounts included.
    Admin,
    /// Changes what its permissions allow.
    Standard,
}

/// What a standard account may change. An admin's are always all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
// The plugin host has a `Permissions` too; the bindings need one name each.
#[serde(rename = "AccountPermissions", rename_all = "camelCase", default)]
pub struct Permissions {
    /// Settings sections it may change, from [`SETTINGS_SECTIONS`].
    pub settings: Vec<String>,
    /// Installing, uninstalling and moving games.
    pub install: bool,
}

impl Permissions {
    pub fn all() -> Self {
        Self {
            settings: SETTINGS_SECTIONS.iter().map(|&s| s.to_owned()).collect(),
            install: true,
        }
    }
}

/// An account as the app shows it; its PIN stays in the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    /// One of [`AVATARS`].
    pub avatar: String,
    pub role: AccountRole,
    /// Whether a PIN locks it.
    pub has_pin: bool,
    pub permissions: Permissions,
    /// The highest minimum age it may play without an admin PIN; `None`
    /// for no limit.
    pub max_age: Option<u8>,
    /// Whether a game without an age rating needs an admin PIN while it has
    /// an age limit; without one, unrated games play.
    pub unrated_needs_pin: bool,
    /// While wrong PINs lock it: when tries are allowed again (Unix ms).
    pub locked_until_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// An account's role, permissions and age limit: what only an admin sets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountAccess {
    pub role: AccountRole,
    /// Ignored for an admin, who may change everything.
    pub permissions: Permissions,
    pub max_age: Option<u8>,
    /// Whether unrated games need an admin PIN under its age limit.
    pub unrated_needs_pin: bool,
}

/// A new account; only an admin makes one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewAccount {
    pub name: String,
    pub avatar: String,
    pub access: AccountAccess,
    /// 4–8 digits. An admin made while standard accounts exist needs one.
    pub pin: Option<String>,
}

/// An account's new name and avatar, and (admins only) its access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdate {
    pub name: String,
    pub avatar: String,
    /// `None` keeps it; an account changing itself may only send `None`.
    pub access: Option<AccountAccess>,
}

/// The answer to a PIN check: never which account matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PinCheck {
    Matched,
    Wrong,
    /// Too many wrong PINs: no tries until then (Unix ms).
    Locked {
        until_ms: i64,
    },
}

/// Accounts, the active one and PIN checks. Cheap to clone.
#[derive(Debug, Clone)]
pub struct AccountService {
    store: LibraryStore,
    bus: Bus,
    /// The signed-in account, kept current as it changes, so permission
    /// checks need no lookup.
    active: Arc<RwLock<Option<Account>>>,
    /// One PIN check or change at a time, so concurrent tries all count.
    pins: Arc<tokio::sync::Mutex<()>>,
    /// The one launch an admin's PIN approved over an age limit (`age`).
    approval: Arc<std::sync::Mutex<Option<age::Approval>>>,
}

impl AccountService {
    /// A single account without a PIN is signed in at once, so a library
    /// that never set accounts up starts as before; otherwise nobody is.
    pub async fn open(store: LibraryStore, bus: Bus) -> Result<Self> {
        let accounts = store.accounts().await?;
        let active = match accounts.as_slice() {
            [only] if !only.has_pin => Some(only.clone()),
            _ => None,
        };
        Ok(Self {
            store,
            bus,
            active: Arc::new(RwLock::new(active)),
            pins: Arc::default(),
            approval: Arc::default(),
        })
    }

    pub fn active_id(&self) -> Option<String> {
        read(&self.active)
            .as_ref()
            .map(|account| account.id.clone())
    }

    /// The signed-in account's id, for what it owns (its collections).
    pub fn require_active(&self) -> Result<String> {
        self.active_id()
            .ok_or_else(|| Error::Invalid(crate::tr!("no account is signed in")))
    }

    pub async fn active(&self) -> Result<Option<Account>> {
        match self.active_id() {
            Some(id) => self.store.get_account(&id).await,
            None => Ok(None),
        }
    }

    /// Every account: admins first, then by name.
    pub async fn list(&self) -> Result<Vec<Account>> {
        self.store.accounts().await
    }

    pub async fn create(&self, new: NewAccount) -> Result<Account> {
        self.admin().await?;
        let now = crate::now_ms();
        let mut account = Account {
            id: new_id(),
            name: name(&new.name)?,
            avatar: avatar(&new.avatar)?,
            role: AccountRole::Standard,
            has_pin: new.pin.is_some(),
            permissions: Permissions::default(),
            max_age: None,
            unrated_needs_pin: false,
            locked_until_ms: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        apply_access(&mut account, new.access)?;
        let hash = match new.pin {
            Some(pin) => Some(pin::hash(pin).await?),
            None => None,
        };
        self.store.insert_account(&account, hash.as_deref()).await?;
        self.changed();
        self.get(&account.id).await
    }

    /// Changes an account's name and avatar (an admin, or the account
    /// itself), and its access (an admin).
    pub async fn update(&self, id: &str, update: AccountUpdate) -> Result<Account> {
        let actor = self.actor().await?;
        if actor.role != AccountRole::Admin && (actor.id != id || update.access.is_some()) {
            return Err(admin_only());
        }
        let mut account = self.get(id).await?;
        account.name = name(&update.name)?;
        account.avatar = avatar(&update.avatar)?;
        if let Some(access) = update.access {
            apply_access(&mut account, access)?;
        }
        account.updated_at_ms = crate::now_ms();
        self.store.update_account(&account).await?;
        let account = self.get(id).await?;
        self.refresh(&account);
        self.changed();
        Ok(account)
    }

    /// Deletes an account and its collections; signs it out if active.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.admin().await?;
        let deleted = self.store.delete_account(id).await?;
        if deleted {
            let mut active = write(&self.active);
            if active.as_ref().is_some_and(|account| account.id == id) {
                *active = None;
            }
        }
        self.changed();
        Ok(deleted)
    }

    /// Sets an account's PIN (4–8 digits), or clears it with `None`: an
    /// admin, or the account itself.
    pub async fn set_pin(&self, id: &str, pin: Option<String>) -> Result<Account> {
        let actor = self.actor().await?;
        if actor.role != AccountRole::Admin && actor.id != id {
            return Err(admin_only());
        }
        let hash = match pin {
            Some(pin) => Some(pin::hash(pin).await?),
            None => None,
        };
        let _pins = self.pins.lock().await;
        self.store
            .set_account_pin(id, hash.as_deref(), crate::now_ms())
            .await?;
        let account = self.get(id).await?;
        self.refresh(&account);
        self.changed();
        Ok(account)
    }

    /// Signs an account in; one with a PIN needs it. Wrong PINs are
    /// throttled per account, across restarts.
    pub async fn sign_in(&self, id: &str, pin: Option<String>) -> Result<PinCheck> {
        let _pins = self.pins.lock().await;
        let row = self
            .store
            .account_row(id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("This account no longer exists.")))?;
        let check = match row.pin_hash {
            None => PinCheck::Matched,
            Some(hash) => {
                self.check(id, row.throttle, &hash, pin.unwrap_or_default())
                    .await?
            }
        };
        if check == PinCheck::Matched {
            *write(&self.active) = Some(row.account);
            self.changed();
        }
        Ok(check)
    }

    /// Library sync removed `from`, this device's untouched default admin,
    /// for `into`, an admin from another device it joined (#273): whoever
    /// was signed in as `from` is `into` now, unless `into` has a PIN.
    pub async fn joined(&self, from: &str, into: &str) {
        if self.active_id().as_deref() != Some(from) {
            return;
        }
        let next = match self.store.get_account(into).await {
            Ok(Some(account)) if !account.has_pin => Some(account),
            _ => None,
        };
        *write(&self.active) = next;
        self.changed();
    }

    pub fn sign_out(&self) {
        *write(&self.active) = None;
        self.changed();
    }

    /// Checks a PIN against every admin's and answers only whether one
    /// matched. A wrong one counts against every admin it was tried on.
    pub async fn verify_admin_pin(&self, pin: String) -> Result<PinCheck> {
        let _pins = self.pins.lock().await;
        let now = crate::now_ms();
        let admins: Vec<_> = self
            .store
            .account_rows()
            .await?
            .into_iter()
            .filter(|row| row.account.role == AccountRole::Admin)
            .filter_map(|row| Some((row.account.id, row.pin_hash?, row.throttle)))
            .collect();
        let open: Vec<_> = admins
            .iter()
            .filter(|(_, _, throttle)| !throttle.locked(now))
            .collect();
        if open.is_empty() {
            return Ok(
                match admins.iter().map(|(_, _, t)| t.locked_until_ms).min() {
                    Some(until_ms) => PinCheck::Locked { until_ms },
                    None => PinCheck::Wrong,
                },
            );
        }
        if pin::valid(&pin) {
            for (id, hash, _) in &open {
                if pin::verify(pin.clone(), hash.clone()).await {
                    self.store
                        .save_pin_throttle(id, &Throttle::default())
                        .await?;
                    return Ok(PinCheck::Matched);
                }
            }
        }
        let mut locked = false;
        for (id, _, throttle) in open {
            let throttle = throttle.wrong(now);
            locked |= throttle.locked(now);
            self.store.save_pin_throttle(id, &throttle).await?;
        }
        if locked {
            self.changed();
        }
        Ok(PinCheck::Wrong)
    }

    /// One try at an account's PIN, counted.
    async fn check(
        &self,
        id: &str,
        throttle: Throttle,
        hash: &str,
        pin: String,
    ) -> Result<PinCheck> {
        let now = crate::now_ms();
        if throttle.locked(now) {
            return Ok(PinCheck::Locked {
                until_ms: throttle.locked_until_ms,
            });
        }
        if pin::valid(&pin) && pin::verify(pin, hash.to_owned()).await {
            self.store
                .save_pin_throttle(id, &Throttle::default())
                .await?;
            return Ok(PinCheck::Matched);
        }
        let throttle = throttle.wrong(now);
        self.store.save_pin_throttle(id, &throttle).await?;
        if throttle.locked(now) {
            self.changed();
        }
        Ok(PinCheck::Wrong)
    }

    async fn get(&self, id: &str) -> Result<Account> {
        self.store
            .get_account(id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("This account no longer exists.")))
    }

    /// The signed-in account, which every change is made as.
    async fn actor(&self) -> Result<Account> {
        let id = self.require_active()?;
        self.store
            .get_account(&id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("no account is signed in")))
    }

    async fn admin(&self) -> Result<Account> {
        let actor = self.actor().await?;
        if actor.role != AccountRole::Admin {
            return Err(admin_only());
        }
        Ok(actor)
    }

    /// Keeps the signed-in account current when `account` is it.
    fn refresh(&self, account: &Account) {
        let mut active = write(&self.active);
        if active
            .as_ref()
            .is_some_and(|active| active.id == account.id)
        {
            *active = Some(account.clone());
        }
    }

    fn changed(&self) {
        self.drop_approval();
        self.bus.emit(crate::bus::Event::AccountChanged {
            active_id: self.active_id(),
        });
    }
}

fn admin_only() -> Error {
    permission::forbidden(Permission::Admin)
}

fn name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX || name.chars().any(char::is_control) {
        return Err(Error::Invalid(crate::tr!(
            "an account name is 1 to {max} characters",
            max = NAME_MAX
        )));
    }
    Ok(name.to_owned())
}

fn avatar(avatar: &str) -> Result<String> {
    if !AVATARS.contains(&avatar) {
        return Err(Error::Invalid(crate::tr!(
            "Choose one of the avatars Caskade offers."
        )));
    }
    Ok(avatar.to_owned())
}

fn apply_access(account: &mut Account, access: AccountAccess) -> Result<()> {
    if access.max_age.is_some_and(|age| age > MAX_AGE) {
        return Err(Error::Invalid(crate::tr!(
            "an age limit is at most {max}",
            max = MAX_AGE
        )));
    }
    let mut settings = access.permissions.settings;
    if settings
        .iter()
        .any(|section| !SETTINGS_SECTIONS.contains(&section.as_str()))
    {
        return Err(Error::Invalid(crate::tr!(
            "One of the chosen settings sections doesn't exist."
        )));
    }
    settings.sort();
    settings.dedup();
    account.role = access.role;
    account.permissions = match access.role {
        AccountRole::Admin => Permissions::all(),
        AccountRole::Standard => Permissions {
            settings,
            install: access.permissions.install,
        },
    };
    account.max_age = access.max_age;
    account.unrated_needs_pin = access.unrated_needs_pin;
    Ok(())
}
