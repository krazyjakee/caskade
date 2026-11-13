//! Achievements (#185): a game's sets for the signed-in account, a per-game
//! refresh that can be stopped, and the events that follow fetches.
use crate::error::{CommandError, report};
use caskade_achievements::achievements::{AchievementSet, Achievements};
use caskade_core::accounts::AccountService;
/// The game's achievement sets the signed-in account sees, as last fetched.
pub async fn game_achievements(
    achievements: &Achievements,
    accounts: &AccountService,
    game_id: String,
) -> Result<Vec<AchievementSet>, CommandError> {
    achievements
        .sets(&game_id, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// Fetches the game's sets now, then answers them. A set that can't be
/// fetched keeps its last answer and says why.
pub async fn game_achievements_refresh(
    achievements: &Achievements,
    accounts: &AccountService,
    game_id: String,
) -> Result<Vec<AchievementSet>, CommandError> {
    crate::permissions::enforce("game_achievements_refresh", serde_json::Value::default)?;
    achievements
        .refresh_game_now(&game_id, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// Stops the game's refresh under way; false when there is none.
pub async fn game_achievements_cancel(
    achievements: &Achievements,
    game_id: String,
) -> Result<bool, CommandError> {
    crate::permissions::enforce("game_achievements_cancel", serde_json::Value::default)?;
    Ok(achievements.cancel_refresh(&game_id))
}
