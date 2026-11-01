//! What achievements are as Caskade keeps them: a service's definitions,
//! an account's state, and how a set's last fetch went.

use crate::error::ErrorReport;
use serde::{Deserialize, Serialize};
use specta::Type;

/// One achievement as its service defines it. Whatever the service doesn't
/// supply is `None`, never made up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementDefinition {
    /// The service's own id (Steam's API name, RetroAchievements' number).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// The service hides it until unlocked: its name, description and icon
    /// are spoilers.
    #[serde(default)]
    pub hidden: bool,
    /// Its icon once unlocked, and while locked, as the service's URLs.
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub locked_icon_url: Option<String>,
    /// The progress that unlocks it (a stat's target), when it has one.
    #[serde(default)]
    pub progress_max: Option<f64>,
    /// Share of players who unlocked it, 0–100, when the service says.
    #[serde(default)]
    pub rarity_percent: Option<f32>,
    /// Points or XP it is worth, when the service gives any.
    #[serde(default)]
    pub points: Option<u32>,
}

/// An account's state for one achievement.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementState {
    pub unlocked: bool,
    /// When, if the service says.
    #[serde(default)]
    pub unlocked_at_ms: Option<i64>,
    /// Progress towards `progress_max`, when the service reports it.
    #[serde(default)]
    pub progress: Option<f64>,
    /// Services with a hardcore mode (RetroAchievements) keep its unlock
    /// apart; `None` where there is no such mode.
    #[serde(default)]
    pub hardcore: Option<HardcoreUnlock>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HardcoreUnlock {
    pub unlocked: bool,
    #[serde(default)]
    pub unlocked_at_ms: Option<i64>,
}

impl AchievementState {
    /// This state, never losing an unlock `known` had: services don't
    /// relock, so a locked answer after an unlock is a partial answer.
    pub fn keeping(mut self, known: &Self) -> Self {
        if known.unlocked {
            self.unlocked = true;
            self.unlocked_at_ms = self.unlocked_at_ms.or(known.unlocked_at_ms);
        }
        if let Some(old) = &known.hardcore
            && old.unlocked
        {
            let hardcore = self.hardcore.get_or_insert_with(Default::default);
            hardcore.unlocked = true;
            hardcore.unlocked_at_ms = hardcore.unlocked_at_ms.or(old.unlocked_at_ms);
        }
        if let (Some(old), Some(new)) = (known.progress, self.progress)
            && new < old
            && self.unlocked
        {
            self.progress = Some(old);
        }
        self
    }
}

/// What a set's last fetch found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SetStatus {
    /// Never fetched yet.
    #[default]
    NotFetched,
    /// Fetched: the achievements below are the service's.
    Ready,
    /// The service says the game has none.
    NoAchievements,
    /// The service doesn't offer this game's achievements.
    Unsupported,
    /// The account's progress is private or otherwise not shown.
    Private,
    /// The account is disconnected or its sign-in expired: connect again.
    SignInNeeded,
}

/// How a set's fetches went; kept with the set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct SetSync {
    pub status: SetStatus,
    /// The last fetch that brought the service's answer.
    pub fetched_at_ms: Option<i64>,
    pub last_attempt_ms: Option<i64>,
    /// Why the last fetch failed or came back incomplete; what is shown is
    /// then the last good answer.
    pub error: Option<ErrorReport>,
    pub failures: u32,
    /// No fetch before this: the service asked Caskade to wait.
    pub retry_at_ms: Option<i64>,
    /// A complete answer has been seen: later unlocks are new ones, not
    /// history coming in.
    pub baseline: bool,
}

/// One achievement with the account's state, for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub definition: AchievementDefinition,
    pub state: AchievementState,
}

/// Whose achievements a set holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetAccount {
    pub store_account_id: String,
    /// The provider (`steam`, `retroachievements`, …).
    pub provider: String,
    /// The provider's name for people.
    pub provider_name: String,
    pub display_name: String,
}

/// One account's achievements for one of a game's releases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementSet {
    /// `None` until first fetched.
    pub id: Option<String>,
    pub account: SetAccount,
    /// The service's id for the game.
    pub product_id: String,
    /// The game's releases this set belongs to.
    pub entry_ids: Vec<String>,
    pub sync: SetSync,
    /// What the service can tell, so the UI can leave out what it can't.
    pub capabilities: AchievementCapabilities,
    pub unlocked: u32,
    pub total: u32,
    /// Hardcore unlocks, where the service has a hardcore mode.
    pub hardcore_unlocked: Option<u32>,
    pub achievements: Vec<Achievement>,
}

/// A game's achievement counts, without the achievements: cards, lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementSummary {
    pub game_id: String,
    pub sets: u32,
    pub unlocked: u32,
    pub total: u32,
}

/// What an achievement service supplies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementCapabilities {
    /// Unlock times.
    pub unlock_times: bool,
    /// Progress towards an achievement.
    pub progress: bool,
    /// How many players unlocked each.
    pub rarity: bool,
    /// A hardcore mode kept apart from the standard one.
    pub hardcore: bool,
    /// Points or XP.
    pub points: bool,
}

/// A release's identity at an achievement service that its store ids don't
/// give (#189).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementLink {
    pub entry_id: String,
    pub provider: String,
    /// The service's game id.
    pub product_id: String,
    pub method: LinkMethod,
    pub linked_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum LinkMethod {
    /// The service recognised this hash of the release's ROM.
    Hash { hash: String },
    /// The user chose the game.
    Manual,
    /// No game could be verified: an unsupported system or file, a ROM the
    /// service doesn't know (another revision or region), or more than one
    /// game. `product_id` is empty. Tried again after a while; never
    /// guessed from the title.
    Unresolved { reason: String },
}
