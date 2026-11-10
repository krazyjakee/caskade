//! Achievement icons for the game page (#190). The webview can't load a
//! service's images itself (its CSP allows local images only), and should
//! never be handed an address to fetch: it names an achievement of one of
//! the game's sets, and the icon that set lists comes through the artwork
//! cache, re-encoded as a WebP thumbnail. Forwarded to
//! `caskade_app::commands::achievement_icons`.
use crate::error::CommandError;
use caskade_achievements::achievements::Achievements;
use caskade_app::commands::achievement_icons as app;
#[cfg(test)]
use caskade_core::artwork::ArtworkCache;
use caskade_core::{accounts::AccountService, artwork::CachedArtwork};
use tauri::State;

pub use app::AchievementIcons;

/// The icon of `achievement_id` in the game's set of `store_account_id` at
/// `product_id`, as the signed-in account sees it: its locked icon when
/// `locked` (else its icon), or its icon. `null` when there is none or it
/// can't load; the page draws its own then.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn game_achievement_icon(
    achievements: State<'_, Achievements>,
    accounts: State<'_, AccountService>,
    icons: State<'_, AchievementIcons>,
    game_id: String,
    store_account_id: String,
    product_id: String,
    achievement_id: String,
    locked: bool,
) -> Result<Option<CachedArtwork>, CommandError> {
    app::game_achievement_icon(
        &achievements,
        &accounts,
        &icons,
        &game_id,
        &store_account_id,
        &product_id,
        &achievement_id,
        locked,
    )
    .await
}

#[cfg(test)]
#[path = "achievement_icons_tests.rs"]
mod tests;
