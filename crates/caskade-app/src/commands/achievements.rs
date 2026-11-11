//! Achievements (#185): a game's sets for the signed-in account, a per-game
//! refresh that can be stopped, and the events that follow fetches.
use crate::error::{CommandError, report};
use caskade_achievements::achievements::{
    AchievementLink, AchievementSet, AchievementSummary, Achievements, LinkMethod,
};
use caskade_core::{
    accounts::AccountService,
    bus::{Bus, Event},
    db::LibraryStore,
};
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

/// The game's counts: how many sets, unlocked and total.
pub async fn game_achievement_summary(
    achievements: &Achievements,
    accounts: &AccountService,
    game_id: String,
) -> Result<AchievementSummary, CommandError> {
    achievements
        .summary(&game_id, accounts.active_id().as_deref())
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

/// How the game's releases are linked to achievement services that their
/// store ids don't give (#189): verified by hash, by hand, or unresolved
/// with why.
pub async fn game_achievement_links(
    achievements: &Achievements,
    game_id: String,
) -> Result<Vec<AchievementLink>, CommandError> {
    achievements.links(&game_id).await.map_err(report)
}

/// Links one of the game's releases to `product_id` at `provider` by hand,
/// replacing a verified or unresolved link; matching never changes it.
pub async fn game_achievement_link_set(
    store: &LibraryStore,
    bus: &Bus,
    game_id: String,
    entry_id: String,
    provider: String,
    product_id: String,
) -> Result<(), CommandError> {
    crate::permissions::enforce("game_achievement_link_set", serde_json::Value::default)?;
    release_of(store, &game_id, &entry_id).await?;
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
pub async fn game_achievement_link_clear(
    store: &LibraryStore,
    bus: &Bus,
    game_id: String,
    entry_id: String,
    provider: String,
) -> Result<bool, CommandError> {
    crate::permissions::enforce("game_achievement_link_clear", serde_json::Value::default)?;
    release_of(store, &game_id, &entry_id).await?;
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
            "This copy of the game is no longer in the library."
        )))),
    }
}
