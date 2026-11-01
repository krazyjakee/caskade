//! The guide quick menu's actions (#149): back to the running game, or stop
//! it. Pressing guide raises Caskade (`input::raise`); these undo that or
//! end the game.

use caskade_core::launch::LaunchService;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

/// How long a game gets to quit before it is ended.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Returns to the game by getting Caskade out of its way: the main window
/// minimizes, and the window manager gives the game focus back.
#[tauri::command]
#[specta::specta]
pub fn guide_return_to_game<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| caskade_core::tr!("the main window is gone"))?;
    window.minimize().map_err(|error| error.to_string())
}

/// Stops the running game of `session_id`; its session ends with the
/// playtime so far. Returns how many of its processes were running.
#[tauri::command]
#[specta::specta]
pub async fn guide_stop_game(
    session_id: String,
    service: tauri::State<'_, LaunchService>,
) -> Result<u32, String> {
    service
        .stop(&session_id, STOP_GRACE)
        .await
        .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
        .map_err(|error| error.to_string())
}
