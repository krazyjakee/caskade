//! Launch by a stored entry id; the frontend supplies no raw process command.
use crate::error::{CommandError, report};
use caskade_core::{
    accounts::{AccountService, AgeRestriction, PinCheck},
    launch::{HostBusy, LaunchChoice, LaunchError, LaunchPrompt, LaunchService, LaunchTicket},
};
use serde::Serialize;
use specta::Type;

/// A launch either started or needs the user's answer first (ask-at-launch,
/// disc picker); answer by calling `launch_game_with` again with a choice.
/// An age limit holds it back until `game_age_approve` takes an admin's
/// PIN for it (#209). A device streaming another game asks before it is
/// stopped (#264): answer with `stopHost`.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LaunchOutcome {
    Started { ticket: LaunchTicket },
    Choose { prompt: LaunchPrompt },
    AgeRestricted { restriction: AgeRestriction },
    HostBusy { busy: HostBusy },
}

#[tauri::command]
#[specta::specta]
pub async fn launch_game(
    entry_id: String,
    service: tauri::State<'_, LaunchService>,
) -> Result<LaunchTicket, CommandError> {
    service
        .launch(&entry_id)
        .await
        .map(|handle| handle.ticket)
        .map_err(report)
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
    match service
        .launch_with(&entry_id, &choice.unwrap_or_default())
        .await
    {
        Ok(handle) => Ok(LaunchOutcome::Started {
            ticket: handle.ticket,
        }),
        Err(LaunchError::NeedsChoice(prompt)) => Ok(LaunchOutcome::Choose { prompt: *prompt }),
        Err(LaunchError::AgeRestricted(restriction)) => Ok(LaunchOutcome::AgeRestricted {
            restriction: *restriction,
        }),
        Err(LaunchError::HostBusy(busy)) => Ok(LaunchOutcome::HostBusy { busy: *busy }),
        Err(error) => Err(report(error)),
    }
}

/// Whether the signed-in account's age limit holds `game_id` back, for Play
/// to ask for an admin's PIN before an update or a download starts.
#[tauri::command]
#[specta::specta]
pub async fn game_age_check(
    game_id: String,
    accounts: tauri::State<'_, AccountService>,
) -> Result<Option<AgeRestriction>, CommandError> {
    accounts.age_restriction(&game_id).await.map_err(report)
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
    accounts
        .approve_play_for(&account_id, &game_id, pin, permanent)
        .await
        .map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn running_games(
    service: tauri::State<'_, LaunchService>,
) -> Result<Vec<LaunchTicket>, CommandError> {
    service.running_sessions().await.map_err(report)
}

#[cfg(all(test, target_os = "linux"))]
#[path = "launch_tests.rs"]
mod tests;
