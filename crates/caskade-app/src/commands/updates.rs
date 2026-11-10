//! Game updates: newer releases of games installed from GitHub, checked
//! without an account (`caskade_core::updates`). Updates are install jobs
//! (`kind: update`), followed like any other.
use caskade_core::{
    install::JobSnapshot,
    launch::LaunchService,
    updates::{GameUpdate, Updates},
};

use crate::error::{CommandError, report};

/// The updates of a game's followed installs; empty when none is followed.
pub async fn game_updates(
    updates: &Updates,
    game_id: &str,
) -> Result<Vec<GameUpdate>, CommandError> {
    updates.game(game_id).await.map_err(report)
}

/// The installs with a newer release out.
pub async fn updates_available(updates: &Updates) -> Result<Vec<GameUpdate>, CommandError> {
    updates.available().await.map_err(report)
}

/// Asks the install's provider for its newest release now.
pub async fn game_update_check(
    updates: &Updates,
    entry_id: &str,
) -> Result<GameUpdate, CommandError> {
    updates.check(entry_id).await.map_err(report)
}

/// The update to offer before the install is played; `None` to play
/// straight away.
pub async fn game_update_before_play(
    updates: &Updates,
    entry_id: &str,
) -> Result<Option<GameUpdate>, CommandError> {
    updates.before_play(entry_id).await.map_err(report)
}

/// Stops offering release `version`; `None` offers every release again.
pub async fn game_update_skip(
    updates: &Updates,
    entry_id: &str,
    version: Option<String>,
) -> Result<GameUpdate, CommandError> {
    updates.skip(entry_id, version).await.map_err(report)
}

/// Offers prereleases too, or stops, and checks again.
pub async fn game_update_prereleases(
    updates: &Updates,
    entry_id: &str,
    prereleases: bool,
) -> Result<GameUpdate, CommandError> {
    updates
        .set_prereleases(entry_id, prereleases)
        .await
        .map_err(report)
}

/// Installs the newest release over the game, keeping the player's files.
pub async fn game_update_install(
    updates: &Updates,
    launch: &LaunchService,
    entry_id: &str,
) -> Result<JobSnapshot, CommandError> {
    updates
        .install(entry_id, launch.clone())
        .await
        .map_err(report)
}
