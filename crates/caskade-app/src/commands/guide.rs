//! The guide quick menu's actions (#149): back to the running game, or stop
//! it. Pressing guide raises Caskade (`input::raise`); these undo that or
//! end the game.

use caskade_core::launch::LaunchService;
use std::time::Duration;
/// How long a game gets to quit before it is ended.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Stops the running game of `session_id`; its session ends with the
/// playtime so far. Returns how many of its processes were running.
pub async fn guide_stop_game(session_id: String, service: &LaunchService) -> Result<u32, String> {
    crate::permissions::enforce("guide_stop_game", serde_json::Value::default)
        .map_err(|refusal| refusal.summary)?;
    service
        .stop(&session_id, STOP_GRACE)
        .await
        .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
        .map_err(|error| error.to_string())
}
