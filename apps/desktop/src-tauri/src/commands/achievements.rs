//! Achievements (#185): a game's sets for the signed-in account, a per-game
//! refresh that can be stopped, and the events that follow fetches.
use crate::error::{CommandError, report};
use caskade_core::{
    accounts::AccountService,
    achievements::{AchievementLink, AchievementSet, AchievementSummary, Achievements, LinkMethod},
    bus::{Bus, Event},
    db::LibraryStore,
};
use tauri::{AppHandle, Manager, Runtime, State};

/// Manages `achievements` and starts its background refreshes.
pub fn setup<R: Runtime>(app: &AppHandle<R>, achievements: Achievements) {
    crate::background::supervise("achievements", {
        let achievements = achievements.clone();
        move || achievements.watch()
    });
    app.manage(achievements);
}

/// The game's achievement sets the signed-in account sees, as last fetched.
#[tauri::command]
#[specta::specta]
pub async fn game_achievements(
    achievements: State<'_, Achievements>,
    accounts: State<'_, AccountService>,
    game_id: String,
) -> Result<Vec<AchievementSet>, CommandError> {
    achievements
        .sets(&game_id, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// The game's counts: how many sets, unlocked and total.
#[tauri::command]
#[specta::specta]
pub async fn game_achievement_summary(
    achievements: State<'_, Achievements>,
    accounts: State<'_, AccountService>,
    game_id: String,
) -> Result<AchievementSummary, CommandError> {
    achievements
        .summary(&game_id, accounts.active_id().as_deref())
        .await
        .map_err(report)
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
    achievements
        .refresh_game_now(&game_id, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// Stops the game's refresh under way; false when there is none.
#[tauri::command]
#[specta::specta]
pub async fn game_achievements_cancel(
    achievements: State<'_, Achievements>,
    game_id: String,
) -> Result<bool, CommandError> {
    Ok(achievements.cancel_refresh(&game_id))
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
    achievements.links(&game_id).await.map_err(report)
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
    release_of(&store, &game_id, &entry_id).await?;
    if product_id.trim().is_empty() {
        return Err(report(caskade_core::Error::Invalid(caskade_core::tr!(
            "choose the game to link"
        ))));
    }
    store
        .save_achievement_link(&AchievementLink {
            entry_id,
            provider,
            product_id: product_id.trim().into(),
            method: LinkMethod::Manual,
            linked_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |at| at.as_millis().min(i64::MAX as u128) as i64),
        })
        .await
        .map_err(report)?;
    bus.emit(Event::AchievementsChanged { game_id });
    Ok(())
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
    release_of(&store, &game_id, &entry_id).await?;
    let removed = store
        .delete_achievement_link(&entry_id, &provider)
        .await
        .map_err(report)?;
    bus.emit(Event::AchievementsChanged { game_id });
    Ok(removed)
}

async fn release_of(
    store: &LibraryStore,
    game_id: &str,
    entry_id: &str,
) -> Result<(), CommandError> {
    match store.get_entry(entry_id).await.map_err(report)? {
        Some(entry) if entry.game_id == game_id => Ok(()),
        _ => Err(report(caskade_core::Error::Invalid(caskade_core::tr!(
            "entry {id} was not found",
            id = entry_id
        )))),
    }
}

#[cfg(test)]
#[path = "achievements_tests.rs"]
mod tests;
