//! Web games (WEB-GAMES-PLAN): Add web game on the Library's Web tab, and
//! keeping one for offline play.
use crate::error::{CommandError, report};
use caskade_core::web::WebGames;
use tauri::State;

/// Adds the game at `url`, a page address, and returns its game id. A page
/// already in the library returns its game.
#[tauri::command]
#[specta::specta]
pub async fn add_web_game(web: State<'_, WebGames>, url: String) -> Result<String, CommandError> {
    web.add(&url).await.map_err(report)
}

/// Opens the guided capture that keeps `game_id`'s web game in `root_id`
/// for offline play (WEB-GAMES-PLAN §6). Returns once the window is open;
/// the library changes when the player saves it.
#[tauri::command]
#[specta::specta]
pub async fn save_web_game_offline(
    web: State<'_, WebGames>,
    game_id: String,
    root_id: String,
) -> Result<(), CommandError> {
    web.save_offline(&game_id, &root_id).await.map_err(report)
}

/// Removes a web game from the library, with its offline copy and saves.
#[tauri::command]
#[specta::specta]
pub async fn remove_web_game(
    web: State<'_, WebGames>,
    game_id: String,
) -> Result<(), CommandError> {
    web.remove(&game_id).await.map_err(report)
}

/// Deletes a web game's offline copy; it plays online again.
#[tauri::command]
#[specta::specta]
pub async fn remove_web_offline(
    web: State<'_, WebGames>,
    game_id: String,
) -> Result<(), CommandError> {
    web.remove_offline(&game_id).await.map_err(report)
}
