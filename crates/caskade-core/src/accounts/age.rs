//! Age limits on play (#209). A standard account with an age limit starts
//! a game whose minimum age is above it only once an admin's PIN approves
//! that launch. The launch pipeline asks through [`PlayGate`], so every way
//! of starting a game is checked, not only the UI's.
//!
//! An unrated game needs an approval only when the account's admin chose
//! that (`Account::unrated_needs_pin`).
//!
//! An approval is held here, never the PIN: it covers one start of one game
//! by the account that was signed in when it was given. It is spent when
//! the game starts, dropped by any change to the accounts or who is signed
//! in, and lapses after [`APPROVAL_MS`]. Permanent approvals are separate
//! per-account game exceptions in the database and apply to browsing too.

use super::{AccountRole, AccountService, PinCheck};
use crate::{
    Result,
    igdb::{IgdbAgeRating, age},
    launch::{LaunchError, PlayGate},
    lock,
};
use serde::Serialize;
use specta::Type;
use std::fmt;

#[cfg(test)]
#[path = "age_tests.rs"]
mod tests;

/// How long an approval waits for its game to start: long enough for an
/// update before play.
const APPROVAL_MS: i64 = 30 * 60_000;

/// One launch an admin approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Approval {
    account_id: String,
    game_id: String,
    until_ms: i64,
}

/// Why the signed-in account can't start a game without an admin's PIN.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgeRestriction {
    pub account_id: String,
    pub account_name: String,
    pub game_id: String,
    pub title: String,
    pub ratings: Vec<IgdbAgeRating>,
    /// The game's minimum age; `None` when it is unrated.
    pub min_age: Option<u8>,
    /// The account's age limit.
    pub max_age: u8,
}

impl fmt::Display for AgeRestriction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let title = &self.title;
        f.write_str(&match (self.min_age, age::shown(&self.ratings, None)) {
            (None, _) => crate::tr!(
                "an admin PIN is needed to play {game}, which is unrated",
                game = title
            ),
            (Some(_), Some(rating)) => crate::tr!(
                "an admin PIN is needed to play {game}, rated {board} {rating}",
                game = title,
                board = rating.board.to_ascii_uppercase(),
                rating = rating.rating
            ),
            (Some(_), None) => {
                crate::tr!("an admin PIN is needed to play {game}", game = title)
            }
        })
    }
}

impl AccountService {
    /// What holds `game_id` back for the signed-in account, temporary approvals
    /// aside; `None` when it may play. Admins, accounts without an age
    /// limit and nobody signed in are never held back.
    pub async fn age_restriction(&self, game_id: &str) -> Result<Option<AgeRestriction>> {
        let Some(account) = self.signed_in().ok() else {
            return Ok(None);
        };
        let Some(max_age) = account
            .max_age
            .filter(|_| account.role == AccountRole::Standard)
        else {
            return Ok(None);
        };
        if self.store.game_age_approved(&account.id, game_id).await? {
            return Ok(None);
        }
        let Some(ratings) = self.store.game_age_ratings(game_id).await? else {
            return Ok(None);
        };
        let held = match ratings.min_age {
            Some(min_age) => min_age > max_age,
            None => account.unrated_needs_pin,
        };
        if !held {
            return Ok(None);
        }
        let title = self
            .store
            .game(game_id)
            .await?
            .map_or_else(|| game_id.to_owned(), |game| game.title);
        Ok(Some(AgeRestriction {
            account_id: account.id,
            account_name: account.name,
            game_id: game_id.to_owned(),
            title,
            ratings: ratings.ratings,
            min_age: ratings.min_age,
            max_age,
        }))
    }

    /// An admin's PIN for one start of `game_id` by the signed-in account.
    /// Wrong PINs count as in [`Self::verify_admin_pin`]. A new approval
    /// replaces the last.
    #[cfg(test)]
    pub async fn approve_play(&self, game_id: &str, pin: String) -> Result<PinCheck> {
        let account_id = self.require_active()?;
        self.approve_play_for(&account_id, game_id, pin, false)
            .await
    }

    /// Approve only the account the dialog named. Permanent exceptions are
    /// persisted; temporary approvals remain scoped to one start.
    pub async fn approve_play_for(
        &self,
        account_id: &str,
        game_id: &str,
        pin: String,
        permanent: bool,
    ) -> Result<PinCheck> {
        if self.require_active()? != account_id {
            return Err(crate::Error::Forbidden(crate::tr!(
                "the signed-in account changed; ask for approval again"
            )));
        }
        let check = self.verify_admin_pin(pin).await?;
        if check == PinCheck::Matched {
            if self.require_active()? != account_id {
                return Err(crate::Error::Forbidden(crate::tr!(
                    "the signed-in account changed; ask for approval again"
                )));
            }
            if permanent {
                self.store.approve_game_age(account_id, game_id).await?;
                self.changed();
                return Ok(check);
            }
            *lock(&self.approval) = Some(Approval {
                account_id: account_id.to_owned(),
                game_id: game_id.to_owned(),
                until_ms: crate::now_ms().saturating_add(APPROVAL_MS),
            });
        }
        Ok(check)
    }

    /// Whether an approval covers `game_id` for the signed-in account now.
    fn approved(&self, game_id: &str) -> bool {
        let active = self.active_id();
        lock(&self.approval).as_ref().is_some_and(|approval| {
            approval.game_id == game_id
                && Some(&approval.account_id) == active.as_ref()
                && approval.until_ms > crate::now_ms()
        })
    }

    /// Drops any approval: the accounts or who is signed in changed.
    pub(crate) fn drop_approval(&self) {
        lock(&self.approval).take();
    }
}

#[async_trait::async_trait]
impl PlayGate for AccountService {
    async fn admit(&self, game_id: &str) -> crate::launch::Result<()> {
        let restriction = self
            .age_restriction(game_id)
            .await
            .map_err(|error| LaunchError::Store(error.to_string()))?;
        match restriction {
            Some(restriction) if !self.approved(game_id) => {
                Err(LaunchError::AgeRestricted(Box::new(restriction)))
            }
            _ => Ok(()),
        }
    }

    fn started(&self, game_id: &str) {
        let mut approval = lock(&self.approval);
        if approval
            .as_ref()
            .is_some_and(|approval| approval.game_id == game_id)
        {
            *approval = None;
        }
    }
}
