//! A web game's offline copy between devices (#76) and in shares (#248):
//! its bundle goes as a folder and becomes the offline copy of the web game
//! at the same page here. It is never installed: a bundle has no program,
//! and the game window plays it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::rom;
use crate::db::LibraryStore;
use crate::install::Placed;
use crate::model::{GameEntry, InstallLocation, InstallState, LaunchSpec};
use crate::web::{WEB_SOURCE, bundle};
use crate::{Error, Result};

/// What an offer says about a web game's offline copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebCopy {
    /// The page the game plays.
    pub url: String,
}

/// Whether `entry` is a web game's release.
pub(crate) fn is_web(entry: &GameEntry) -> bool {
    entry.source_id == WEB_SOURCE || matches!(entry.launch, Some(LaunchSpec::Web { .. }))
}

/// What web release `entry` sends; refused when it plays from its page
/// alone, with nothing to copy.
pub(crate) fn describe(entry: &GameEntry) -> Result<WebCopy> {
    let Some(LaunchSpec::Web { url }) = &entry.launch else {
        return Err(Error::Invalid(crate::tr!("that isn't a web game")));
    };
    if !matches!(entry.location, Some(InstallLocation::Managed { .. })) {
        return Err(Error::Invalid(no_offline_copy()));
    }
    Ok(WebCopy { url: url.clone() })
}

/// Why a web game played from its page can't be copied.
pub(crate) fn no_offline_copy() -> String {
    crate::tr!(
        "it plays from its web page and has no offline copy there; save it for offline play on that device first"
    )
}

/// What arrived in `folder`, when it's a web game's offline copy an offer
/// didn't say was one (a device on an older Caskade): the page its bundle
/// starts at.
pub(crate) fn in_folder(folder: &Path) -> Option<WebCopy> {
    if !folder.join(bundle::MANIFEST).is_file() {
        return None;
    }
    let start = bundle::Bundle::open(folder).ok()?.start();
    crate::web::page_url(&start)
        .ok()
        .map(|url| WebCopy { url: url.into() })
}

/// Makes the bundle in `folder` the offline copy of the web game at `url`,
/// moved into the storage root at `root`. The game is the one with that
/// page here, else `game_id` when it's here, else a new one named `title`.
/// A game already offline here keeps its own copy.
pub async fn place(
    store: &LibraryStore,
    url: &str,
    folder: &Path,
    root: &Path,
    title: &str,
    game_id: Option<&str>,
) -> Result<Placed> {
    let url = crate::web::page_url(url)?.to_string();
    let root = rom::root_at(store, root).await?;
    let existing = store.entry_by_source(WEB_SOURCE.as_str(), &url).await?;
    if let Some(entry) = existing.as_ref().filter(|entry| entry.location.is_some()) {
        // Theirs stays: it's already offline here.
        return Ok(Placed {
            game_id: entry.game_id.clone(),
            added: false,
        });
    }
    let target = match game_id {
        Some(id) => store.game(id).await?.map(|game| game.id),
        None => None,
    };
    let (rel_dir, size) = {
        let (folder, base) = (folder.to_owned(), root.path.clone());
        let name = super::folder_name(title);
        tokio::task::spawn_blocking(move || {
            let rel_dir = rom::free_folder(&base, &name)?;
            let target = base.join(&rel_dir);
            rom::move_path(&folder, &target)?;
            let bundle = bundle::Bundle::open(&target).map_err(|_| {
                let _ = std::fs::remove_dir_all(&target);
                Error::Invalid(crate::tr!("that isn't a web game's offline copy"))
            })?;
            Ok::<_, Error>((rel_dir, bundle.stats().1))
        })
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))??
    };
    let location = Some(InstallLocation::Managed {
        root_id: root.id.clone(),
        rel_dir,
    });
    let web_entry = |game_id: &str| {
        let mut entry = GameEntry::new(game_id, WEB_SOURCE, url.as_str());
        entry.state = InstallState::Installed;
        entry.launch = Some(LaunchSpec::Web { url: url.clone() });
        entry
    };
    let (mut entry, added) = match (existing, target) {
        (Some(entry), _) => (entry, false),
        (None, Some(game_id)) => (web_entry(&game_id), false),
        (None, None) => {
            let mut game = crate::model::Game::new(if title.trim().is_empty() {
                "Web game"
            } else {
                title.trim()
            });
            game.platforms = vec![crate::web::PLATFORM.to_owned()];
            let game = store.upsert_game(&game).await?;
            (web_entry(&game.id), true)
        }
    };
    entry.location = location;
    entry.size = Some(size);
    store.upsert_entry(&entry).await?;
    Ok(Placed {
        game_id: entry.game_id,
        added,
    })
}
