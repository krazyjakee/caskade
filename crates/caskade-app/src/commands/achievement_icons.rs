//! Achievement icons for the game page (#190). The UI can't load a
//! service's images itself (its CSP allows local images only), and should
//! never be handed an address to fetch: it names an achievement of one of
//! the game's sets, and the icon that set lists comes through the artwork
//! cache, re-encoded as a WebP thumbnail.
use caskade_achievements::achievements::Achievements;
use caskade_core::{
    accounts::AccountService,
    artwork::{ArtworkCache, CachedArtwork},
};

use crate::error::{CommandError, report};

/// The cache achievement icons go through; `None` without one.
pub struct AchievementIcons(pub Option<ArtworkCache>);

/// The icon of `achievement_id` in the game's set of `store_account_id` at
/// `product_id`, as the signed-in account sees it: its locked icon when
/// `locked` (else its icon), or its icon. `None` when there is none or it
/// can't load; the page draws its own then.
#[allow(clippy::too_many_arguments)]
pub async fn game_achievement_icon(
    achievements: &Achievements,
    accounts: &AccountService,
    icons: &AchievementIcons,
    game_id: &str,
    store_account_id: &str,
    product_id: &str,
    achievement_id: &str,
    locked: bool,
) -> Result<Option<CachedArtwork>, CommandError> {
    let Some(cache) = &icons.0 else {
        return Ok(None);
    };
    let sets = achievements
        .sets(game_id, accounts.active_id().as_deref())
        .await
        .map_err(report)?;
    let Some(definition) = sets
        .iter()
        .filter(|set| {
            set.account.store_account_id == store_account_id && set.product_id == product_id
        })
        .flat_map(|set| &set.achievements)
        .find(|achievement| achievement.definition.id == achievement_id)
        .map(|achievement| &achievement.definition)
    else {
        return Ok(None);
    };
    let url = if locked {
        definition
            .locked_icon_url
            .as_ref()
            .or(definition.icon_url.as_ref())
    } else {
        definition.icon_url.as_ref()
    };
    let Some(url) = url else {
        return Ok(None);
    };
    match cache.download_metadata(url).await {
        Ok(cached) => Ok(Some(cached)),
        Err(error) => {
            tracing::debug!(%error, game = %game_id, "achievement icon not loaded");
            Ok(None)
        }
    }
}
