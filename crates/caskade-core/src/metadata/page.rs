//! What the game page reads and changes: the match, set by hand, matched
//! again or refreshed, the images only the page shows, and Separate.

use super::{GameIgdb, IgdbMatch, MatchSource, Metadata, RunReport};
use crate::{Error, Result, bus::Event};
use std::collections::HashSet;

impl Metadata {
    /// Moves `entry_id` out of its game into a game of its own, which is
    /// never merged with the games of its IGDB id again: undoes a merge
    /// of games that aren't the same. The new game's id.
    pub async fn separate(&self, entry_id: &str) -> Result<String> {
        let _lock = self.inner.lock.lock().await;
        let store = &self.inner.store;
        let entry = store.get_entry(entry_id).await?.ok_or_else(|| {
            Error::Invalid(crate::tr!("entry {entry} was not found", entry = entry_id))
        })?;
        let game = store.game(&entry.game_id).await?.ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "game {game} was not found",
                game = entry.game_id
            ))
        })?;
        // Emulated entries name their platform; the game's other platforms
        // are its PC (or unknown) entries'.
        let catalog = &self.inner.catalog;
        let platform = |entry: &crate::model::GameEntry| match &entry.launch {
            Some(crate::model::LaunchSpec::Emulator { platform_id, .. }) => Some(
                catalog
                    .platform(platform_id)
                    .map_or_else(|| platform_id.clone(), |platform| platform.name.clone()),
            ),
            _ => None,
        };
        let others: Vec<_> = store
            .entries_for_game(&game.id)
            .await?
            .into_iter()
            .filter(|other| other.id != entry.id)
            .collect();
        let emulated: HashSet<String> =
            others.iter().chain([&entry]).filter_map(platform).collect();
        let pc: Vec<String> = game
            .platforms
            .iter()
            .filter(|name| !emulated.contains(*name))
            .cloned()
            .collect();
        let platforms = platform(&entry).map_or_else(|| pc.clone(), |name| vec![name]);
        let mut left: Vec<String> = others.iter().filter_map(platform).collect();
        if others.iter().any(|other| platform(other).is_none()) {
            left.extend(pc);
        }
        // In the game's own order, then the ones it didn't list.
        let mut ordered: Vec<String> = game
            .platforms
            .iter()
            .filter(|name| left.contains(name))
            .cloned()
            .collect();
        for name in left {
            if !ordered.contains(&name) {
                ordered.push(name);
            }
        }
        let separated = store.separate_entry(entry_id, &platforms, &ordered).await?;
        self.inner
            .bus
            .emit(Event::LibraryGameUpdated { game_id: game.id });
        self.inner.bus.emit(Event::LibraryGameAdded {
            game_id: separated.id.clone(),
        });
        Ok(separated.id)
    }

    /// The game's match and its IGDB game; `None` before matching has
    /// looked at the game. Read from the caches alone, so it is quick and
    /// works offline: applying a match caches its IGDB game, and until then
    /// (a pending match) the page goes without.
    pub async fn game(&self, game_id: &str) -> Result<Option<GameIgdb>> {
        let Some(row) = self.inner.store.igdb_match(game_id).await? else {
            return Ok(None);
        };
        let game = match row.igdb_id {
            Some(id) => self.inner.igdb.cached_game(id).await?,
            None => None,
        };
        Ok(Some(GameIgdb {
            game_id: game_id.to_owned(),
            igdb_id: row.igdb_id,
            source: row.source,
            pending: row.pending,
            game,
        }))
    }

    /// Matches the game to IGDB game `igdb_id`, or to none, by hand. The
    /// game is filled in from it straight away; automatic matching leaves
    /// it alone from now on. When that makes it the same game as another,
    /// they merge, and the match is the merged game's.
    pub async fn set(&self, game_id: &str, igdb_id: Option<u64>) -> Result<GameIgdb> {
        let game = match igdb_id {
            Some(id) => {
                if i64::try_from(id).is_err() || id == 0 {
                    return Err(Error::Invalid(crate::tr!(
                        "{id} is not an IGDB id",
                        id = id
                    )));
                }
                let lookup = self.inner.igdb.games(&[id]).await?;
                match lookup.games.into_iter().next() {
                    Some(game) => Some(game),
                    None if lookup.not_found.contains(&id) => {
                        return Err(Error::Invalid(crate::tr!("IGDB has no game {id}", id = id)));
                    }
                    None => {
                        return Err(Error::Invalid(crate::tr!(
                            "the metadata server sent no readable game {id}; try again later",
                            id = id
                        )));
                    }
                }
            }
            None => None,
        };
        let store = &self.inner.store;
        if store.game(game_id).await?.is_none() {
            return Err(Error::Invalid(crate::tr!(
                "game {game} was not found",
                game = game_id
            )));
        }
        // Before the lock, so nothing waits for the download.
        let prefetched = self.prefetch_game(game_id, game.as_ref(), false).await?;
        let lock = self.inner.lock.lock().await;
        if store.game(game_id).await?.is_none() {
            return Err(Error::Invalid(crate::tr!(
                "game {game} was not found",
                game = game_id
            )));
        }
        let mut row = IgdbMatch::new(game_id, igdb_id, MatchSource::Manual, "");
        if let Some(old) = store.igdb_match(game_id).await? {
            row.applied = old.applied;
        }
        store.save_igdb_match(&row).await?;
        if let Some(igdb) = &game {
            self.rename(game_id, &igdb.title).await?;
        }
        self.apply(&mut row, game.as_ref(), &prefetched).await?;
        drop(lock);
        // A game matched by hand is the one the others merge into, unless
        // another was matched by hand too.
        let dumps = self.dumps().await;
        let moved = self
            .merge_same(dumps.as_deref(), &mut RunReport::default())
            .await?;
        if let Some(keep) = moved.get(game_id)
            && let Some(kept) = self.game(keep).await?
        {
            return Ok(kept);
        }
        Ok(GameIgdb {
            game_id: game_id.to_owned(),
            igdb_id,
            source: row.source,
            pending: row.pending,
            game,
        })
    }

    /// Gives the game IGDB's `title`, for a match made by hand: the user
    /// chose that game, so the library goes by its name. The sort title
    /// follows unless the user set one of their own.
    async fn rename(&self, game_id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            return Ok(());
        }
        // A title the user set stays theirs; IGDB's goes beneath (#171).
        let renamed = self
            .inner
            .store
            .update_automatic_game(game_id, |game| {
                if game.title == title {
                    return;
                }
                let before = game.clone();
                if game.sort_title == game.title {
                    title.clone_into(&mut game.sort_title);
                }
                title.clone_into(&mut game.title);
                game.aliases.retain(|alias| alias != title);
                if let Err(error) = game.validate() {
                    tracing::warn!(game = %game.id, %error, "IGDB title not applied");
                    *game = before;
                }
            })
            .await?;
        if !matches!(renamed, Some(((), true))) {
            return Ok(());
        }
        self.inner.bus.emit(Event::LibraryGameUpdated {
            game_id: game_id.to_owned(),
        });
        Ok(())
    }

    /// Drops the game's match, made by hand or not, and matches it again
    /// now. When that makes it the same game as another, they merge, and
    /// the match is the merged game's.
    pub async fn rematch(&self, game_id: &str) -> Result<Option<GameIgdb>> {
        {
            let _lock = self.inner.lock.lock().await;
            let store = &self.inner.store;
            if store.game(game_id).await?.is_none() {
                return Err(Error::Invalid(crate::tr!(
                    "game {game} was not found",
                    game = game_id
                )));
            }
            if let Some(mut row) = store.igdb_match(game_id).await? {
                // A 'none' that is due, so matching looks again; what was
                // applied stays recorded so the new match replaces it.
                row.igdb_id = None;
                row.source = MatchSource::None;
                row.pending = false;
                row.checked_at_ms = 0;
                row.dumps.clear();
                store.save_igdb_match(&row).await?;
            }
        }
        let dumps = self.dumps().await;
        let mut report = RunReport::default();
        self.run_games(&[game_id.to_owned()], dumps.as_deref(), &mut report, None)
            .await?;
        let moved = self.merge_same(dumps.as_deref(), &mut report).await?;
        self.game(moved.get(game_id).map_or(game_id, String::as_str))
            .await
    }

    /// Fetches the game's IGDB game again and applies it, unless the match
    /// changed meanwhile.
    pub async fn refresh(&self, game_id: &str) -> Result<Option<GameIgdb>> {
        let store = &self.inner.store;
        let Some(row) = store.igdb_match(game_id).await? else {
            return Ok(None);
        };
        let game = match row.igdb_id {
            Some(id) => {
                let lookup = self.inner.igdb.refresh(&[id]).await?;
                // Only IGDB's "no such game" may undo the last apply.
                if lookup.pending.contains(&id) {
                    return Err(Error::Invalid(crate::tr!(
                        "the metadata server sent no readable game {id}",
                        id = id
                    )));
                }
                lookup.games.into_iter().next()
            }
            None => None,
        };
        let prefetched = self.prefetch_game(game_id, game.as_ref(), false).await?;
        {
            let _lock = self.inner.lock.lock().await;
            if let Some(mut now) = store.igdb_match(game_id).await?
                && (now.igdb_id, now.source) == (row.igdb_id, row.source)
            {
                now.pending = true;
                self.apply(&mut now, game.as_ref(), &prefetched).await?;
            }
        }
        self.game(game_id).await
    }

    /// Downloads the IGDB hero and screenshots the game's page shows, the
    /// first time it opens. Reads the match from the caches only, so a game
    /// that isn't matched or applied yet gets nothing. Whether any came.
    pub async fn page_images(&self, game_id: &str) -> Result<bool> {
        let store = &self.inner.store;
        let Some(id) = store.igdb_match(game_id).await?.and_then(|row| row.igdb_id) else {
            return Ok(false);
        };
        let Some(game) = self.inner.igdb.cached_game(id).await? else {
            return Ok(false);
        };
        let prefetched = self.prefetch_game(game_id, Some(&game), true).await?;
        let _lock = self.inner.lock.lock().await;
        // Matched to another game meanwhile: its images wait for its page.
        if store.igdb_match(game_id).await?.and_then(|row| row.igdb_id) != Some(id) {
            return Ok(false);
        }
        let (changed, _) = self
            .apply_artwork(game_id, Some(&game), true, &prefetched)
            .await?;
        if changed {
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: game_id.to_owned(),
            });
        }
        Ok(changed)
    }
}
