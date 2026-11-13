//! Achievements (#185): a game's sets for the signed-in account, a per-game
//! refresh that can be stopped, and the events that follow fetches.
use crate::error::CommandError;
use caskade_achievements::achievements::{AchievementSet, Achievements};
use caskade_app::commands::achievements as app;
use caskade_core::accounts::AccountService;
use tauri::State;
/// The game's achievement sets the signed-in account sees, as last fetched.
#[tauri::command]
#[specta::specta]
pub async fn game_achievements(
    achievements: State<'_, Achievements>,
    accounts: State<'_, AccountService>,
    game_id: String,
) -> Result<Vec<AchievementSet>, CommandError> {
    app::game_achievements(&achievements, &accounts, game_id).await
}

/// Fetches the game's sets now, then answers them. A set that can't be
/// fetched keeps its last answer and says why.
#[tauri::command]
#[specta::specta]
pub async fn game_achievements_refresh(
    achievements: State<'_, Achievements>,
    accounts: State<'_, AccountService>,
    game_id: String,
) -> Result<Vec<AchievementSet>, CommandError> {
    app::game_achievements_refresh(&achievements, &accounts, game_id).await
}

/// Stops the game's refresh under way; false when there is none.
#[tauri::command]
#[specta::specta]
pub async fn game_achievements_cancel(
    achievements: State<'_, Achievements>,
    game_id: String,
) -> Result<bool, CommandError> {
    app::game_achievements_cancel(&achievements, game_id).await
}

#[cfg(test)]
#[path = "achievements_tests.rs"]
mod tests;
