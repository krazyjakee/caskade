//! Launch by a stored entry id; the frontend supplies no raw process command.
//! Forwarded to `caskade_app::commands::launch`.
use crate::error::CommandError;
use caskade_app::commands::launch as app;
use caskade_core::{
    accounts::{AccountService, AgeRestriction, PinCheck},
    launch::{LaunchChoice, LaunchService, LaunchTicket},
};

pub use app::LaunchOutcome;

#[tauri::command]
#[specta::specta]
pub async fn launch_game(
    entry_id: String,
    service: tauri::State<'_, LaunchService>,
) -> Result<LaunchTicket, CommandError> {
    app::launch_game(&service, &entry_id).await
}

/// Like `launch_game`, but an emulated game that must ask returns its
/// prompt instead of failing.
#[tauri::command]
#[specta::specta]
pub async fn launch_game_with(
    entry_id: String,
    choice: Option<LaunchChoice>,
    service: tauri::State<'_, LaunchService>,
) -> Result<LaunchOutcome, CommandError> {
    app::launch_game_with(&service, &entry_id, choice).await
}

/// Whether the signed-in account's age limit holds `game_id` back, for Play
/// to ask for an admin's PIN before an update or a download starts.
#[tauri::command]
#[specta::specta]
pub async fn game_age_check(
    game_id: String,
    accounts: tauri::State<'_, AccountService>,
) -> Result<Option<AgeRestriction>, CommandError> {
    app::game_age_check(&accounts, &game_id).await
}

/// An admin's PIN approves this game for the named, signed-in account.
/// `permanent` persists an exception; otherwise its next start spends it.
#[tauri::command]
#[specta::specta]
pub async fn game_age_approve(
    game_id: String,
    account_id: String,
    pin: String,
    permanent: bool,
    accounts: tauri::State<'_, AccountService>,
) -> Result<PinCheck, CommandError> {
    app::game_age_approve(&accounts, &game_id, &account_id, pin, permanent).await
}

#[tauri::command]
#[specta::specta]
pub async fn running_games(
    service: tauri::State<'_, LaunchService>,
) -> Result<Vec<LaunchTicket>, CommandError> {
    app::running_games(&service).await
}

#[cfg(all(test, target_os = "linux"))]
#[path = "launch_tests.rs"]
mod tests;
