//! Web games: Add web game on the Library's Web tab, and
//! keeping one for offline play.
use caskade_core::web::WebGames;

use crate::error::{CommandError, report};

/// Adds the game at `url`, a page address, and returns its game id. A page
/// already in the library returns its game.
pub async fn add_web_game(web: &WebGames, url: &str) -> Result<String, CommandError> {
    crate::permissions::enforce("add_web_game", serde_json::Value::default)?;
    web.add(url).await.map_err(report)
}

/// Opens the guided capture that keeps `game_id`'s web game in `root_id`
/// for offline play. Returns once the window is open;
/// the library changes when the player saves it.
pub async fn save_web_game_offline(
    web: &WebGames,
    game_id: &str,
    root_id: &str,
) -> Result<(), CommandError> {
    crate::permissions::enforce("save_web_game_offline", serde_json::Value::default)?;
    web.save_offline(game_id, root_id).await.map_err(report)
}

/// Removes a web game from the library, with its offline copy and saves.
pub async fn remove_web_game(web: &WebGames, game_id: &str) -> Result<(), CommandError> {
    crate::permissions::enforce("remove_web_game", serde_json::Value::default)?;
    web.remove(game_id).await.map_err(report)
}

/// Deletes a web game's offline copy; it plays online again.
pub async fn remove_web_offline(web: &WebGames, game_id: &str) -> Result<(), CommandError> {
    crate::permissions::enforce("remove_web_offline", serde_json::Value::default)?;
    web.remove_offline(game_id).await.map_err(report)
}
