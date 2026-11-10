//! Web games: Add web game on the Library's Web tab, and
//! keeping one for offline play. Forwarded to `caskade_app::commands::web`.
use crate::error::CommandError;
use caskade_app::commands::web as app;
use caskade_core::web::WebGames;
use tauri::State;

/// Adds the game at `url`, a page address, and returns its game id. A page
/// already in the library returns its game.
#[tauri::command]
#[specta::specta]
pub async fn add_web_game(web: State<'_, WebGames>, url: String) -> Result<String, CommandError> {
    app::add_web_game(&web, &url).await
}

/// Opens the guided capture that keeps `game_id`'s web game in `root_id`
/// for offline play. Returns once the window is open;
/// the library changes when the player saves it.
#[tauri::command]
#[specta::specta]
pub async fn save_web_game_offline(
    web: State<'_, WebGames>,
    game_id: String,
    root_id: String,
) -> Result<(), CommandError> {
    app::save_web_game_offline(&web, &game_id, &root_id).await
}

/// Removes a web game from the library, with its offline copy and saves.
#[tauri::command]
#[specta::specta]
pub async fn remove_web_game(
    web: State<'_, WebGames>,
    game_id: String,
) -> Result<(), CommandError> {
    app::remove_web_game(&web, &game_id).await
}

/// Deletes a web game's offline copy; it plays online again.
#[tauri::command]
#[specta::specta]
pub async fn remove_web_offline(
    web: State<'_, WebGames>,
    game_id: String,
) -> Result<(), CommandError> {
    app::remove_web_offline(&web, &game_id).await
}
