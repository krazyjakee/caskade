//! Caskade's apps on a host (STREAMING-PLAN §4.3, §3.4): each runs
//! `caskade --launch <game id>`, which is also how Caskade tells its own
//! apps from the others. On a user's own Sunshine (#258) Caskade changes
//! only those, never the user's other apps or settings.

use super::api::{App, HostApi};
use crate::Result;
use std::path::Path;

/// A game to offer: its name and library id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub title: String,
    pub game_id: String,
}

/// The command that plays `game_id` through `caskade` (quoted, since the
/// program's path may hold spaces).
pub fn launch_command(caskade: &Path, game_id: &str) -> String {
    format!("\"{}\" --launch {game_id}", caskade.display())
}

/// The game an app plays, when it is one of Caskade's (`caskade` started
/// with `--launch`).
pub fn caskade_game(app: &App, caskade: &Path) -> Option<String> {
    let prefix = format!("\"{}\" --launch ", caskade.display());
    let rest = app.cmd.as_deref()?.strip_prefix(&prefix)?;
    (!rest.is_empty() && !rest.contains(char::is_whitespace)).then(|| rest.to_owned())
}

/// What a sync changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Published {
    pub added: usize,
    pub removed: usize,
}

/// Makes Caskade's apps on the host exactly `offers`: missing ones added,
/// ones no longer offered removed. Other apps stay as they are. Indexes
/// are read again after each change: the host sorts its list.
pub async fn sync(api: &HostApi, caskade: &Path, offers: &[Offer]) -> Result<Published> {
    let mut published = Published::default();
    loop {
        let apps = api.apps().await?;
        let stale = apps.iter().position(|app| {
            caskade_game(app, caskade).is_some_and(|game| {
                !offers
                    .iter()
                    .any(|offer| offer.game_id == game && offer.title == app.name)
            })
        });
        let Some(index) = stale else { break };
        api.delete_app(index).await?;
        published.removed += 1;
    }
    let apps = api.apps().await?;
    for offer in offers {
        let there = apps
            .iter()
            .any(|app| caskade_game(app, caskade).as_deref() == Some(offer.game_id.as_str()));
        if !there {
            api.add_app(&App::command(
                &offer.title,
                &launch_command(caskade, &offer.game_id),
            ))
            .await?;
            published.added += 1;
        }
    }
    Ok(published)
}

/// Removes every one of Caskade's apps, and only those.
pub async fn withdraw(api: &HostApi, caskade: &Path) -> Result<usize> {
    Ok(sync(api, caskade, &[]).await?.removed)
}
