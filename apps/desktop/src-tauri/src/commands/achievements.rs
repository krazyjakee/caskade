//! Achievements (#185): a game's sets for the signed-in account, a per-game
//! refresh that can be stopped, and the events that follow fetches.
use crate::error::CommandError;
use caskade_achievements::achievements::{
    AchievementLink, AchievementSet, AchievementSummary, Achievements,
};
use caskade_app::commands::achievements as app;
use caskade_core::{accounts::AccountService, bus::Bus, db::LibraryStore};
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

/// The game's counts: how many sets, unlocked and total.
#[tauri::command]
#[specta::specta]
pub async fn game_achievement_summary(
    achievements: State<'_, Achievements>,
    accounts: State<'_, AccountService>,
    game_id: String,
) -> Result<AchievementSummary, CommandError> {
    app::game_achievement_summary(&achievements, &accounts, game_id).await
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

/// How the game's releases are linked to achievement services that their
/// store ids don't give (#189): verified by hash, by hand, or unresolved
/// with why.
#[tauri::command]
#[specta::specta]
pub async fn game_achievement_links(
    achievements: State<'_, Achievements>,
    game_id: String,
) -> Result<Vec<AchievementLink>, CommandError> {
    app::game_achievement_links(&achievements, game_id).await
}

/// Links one of the game's releases to `product_id` at `provider` by hand,
/// replacing a verified or unresolved link; matching never changes it.
#[tauri::command]
#[specta::specta]
pub async fn game_achievement_link_set(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_id: String,
    entry_id: String,
    provider: String,
    product_id: String,
) -> Result<(), CommandError> {
    app::game_achievement_link_set(&store, &bus, game_id, entry_id, provider, product_id).await
}

/// Removes a release's link: the next refresh looks for a verified one.
#[tauri::command]
#[specta::specta]
pub async fn game_achievement_link_clear(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_id: String,
    entry_id: String,
    provider: String,
) -> Result<bool, CommandError> {
    app::game_achievement_link_clear(&store, &bus, game_id, entry_id, provider).await
}

#[cfg(test)]
#[path = "achievements_tests.rs"]
mod tests;
