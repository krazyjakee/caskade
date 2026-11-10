//! Play a game on another device (#234): asking one, and starting what
//! another asks for here, through the app's own Play (age limits, the admin
//! PIN, console mode). Forwarded to `caskade_app::commands::remote_play`.
use caskade_app::commands::remote_play as app;
use caskade_core::remote_play::RemotePlay;
use tauri::State;

use crate::error::CommandError;

/// Starts `game_id` on `device_id`, waking it first when it's out of reach
/// and can be woken. Answers once it started there, or why not.
#[tauri::command]
#[specta::specta]
pub async fn game_play_on(
    play: State<'_, RemotePlay>,
    game_id: String,
    device_id: String,
) -> Result<(), CommandError> {
    app::game_play_on(&play, &game_id, &device_id).await
}

/// How starting a game another device asked for went here.
#[tauri::command]
#[specta::specta]
pub fn remote_play_finished(
    play: State<'_, RemotePlay>,
    request_id: String,
    started: bool,
    reason: Option<String>,
) {
    app::remote_play_finished(&play, &request_id, started, reason)
}

