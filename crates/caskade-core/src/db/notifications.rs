//! The game additions journal's one write the library makes itself; the
//! notification feed's repository is in `caskade-achievements`.

use crate::Result;
use sqlx::SqliteConnection;

/// Marks `game_id`, about to be saved, as not new: a game split off another
/// (Separate) is not an addition to the library.
pub(super) async fn not_an_addition(conn: &mut SqliteConnection, game_id: &str) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO game_additions(game_id,added_at_ms,processed) VALUES(?,0,1)",
    )
    .bind(game_id)
    .execute(conn)
    .await?;
    Ok(())
}
