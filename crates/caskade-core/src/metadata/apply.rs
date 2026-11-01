//! Filling a library game in from its IGDB game, and taking back what an
//! earlier match filled in.

use super::{Applied, IgdbMatch, Metadata, Progress};
use crate::{
    Result,
    artwork::CachedArtwork,
    bus::Event,
    igdb::{IgdbGame, age},
    model::{Artwork, ArtworkKind, Game},
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Screenshots kept per game: the game page's strip.
const SCREENSHOTS: usize = 4;
/// The model's limit on each list.
const MAX_LIST: usize = 256;
/// IGDB artwork's source, with the image id after it.
pub(crate) const SOURCE: &str = "igdb:";
/// IGDB's `artwork_types` id for a game's square icon.
const ICON_TYPE: u64 = 13;
/// Games whose images download at the same time.
const DOWNLOADS: usize = 8;

/// A downloaded image, or why it didn't come.
type Downloaded = std::result::Result<CachedArtwork, Failed>;
/// Images downloaded ahead of an apply, by URL.
pub(super) type Prefetched = HashMap<String, Downloaded>;

/// An image that didn't download.
#[derive(Debug, Clone)]
pub(super) struct Failed {
    message: String,
    retry: Retry,
}

/// When a failed image is worth trying again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "net"), allow(dead_code))]
enum Retry {
    /// Its host couldn't be reached: the rest of the run waits too.
    Offline,
    /// On the next run: the server had trouble, or the cache did.
    Later,
    /// Not until the match changes or is refreshed: a 404, or not an
    /// image. The game goes without it.
    Never,
}

impl Failed {
    #[cfg(feature = "net")]
    fn new(error: &crate::artwork::ArtworkError) -> Self {
        let retry = if error.unreachable() {
            Retry::Offline
        } else if error.permanent() {
            Retry::Never
        } else {
            Retry::Later
        };
        Self {
            message: error.to_string(),
            retry,
        }
    }
}

/// Whether an image of `prefetched` failed because its host couldn't be
/// reached.
pub(super) fn offline(prefetched: &Prefetched) -> bool {
    prefetched
        .values()
        .any(|downloaded| matches!(downloaded, Err(failed) if failed.retry == Retry::Offline))
}
/// An IGDB image a game should have: its kind, source and URL.
type Wanted<'a> = (ArtworkKind, String, &'a str);

impl Metadata {
    /// Applies `igdb` (nothing, for no match) to the row's game and saves
    /// the row, with the images in `prefetched`; it downloads nothing, so
    /// callers may hold the lock. Whether it all went in: an image that may
    /// come later (or wasn't prefetched) leaves the row pending, for the
    /// next run. One that can never come (a 404, not an image) doesn't.
    pub(super) async fn apply(
        &self,
        row: &mut IgdbMatch,
        igdb: Option<&IgdbGame>,
        prefetched: &Prefetched,
    ) -> Result<bool> {
        let store = &self.inner.store;
        // IGDB fills the game in beneath the fields the user set (#171).
        let old = &row.applied;
        let Some((applied, mut changed)) = store
            .update_automatic_game(&row.game_id, |game| {
                let before = game.clone();
                let applied = merge(game, old, igdb);
                if *game != before
                    && let Err(error) = game.validate()
                {
                    tracing::warn!(game = %game.id, %error, "IGDB metadata not applied");
                    *game = before;
                    return None;
                }
                Some(applied)
            })
            .await?
        else {
            return Ok(true);
        };
        if let Some(applied) = applied {
            row.applied = applied;
        }
        changed |= self.apply_age_ratings(row, igdb).await?;
        let (art_changed, complete) = self
            .apply_artwork(&row.game_id, igdb, false, prefetched)
            .await?;
        row.pending = !complete;
        store.save_igdb_match(row).await?;
        if changed || art_changed {
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: row.game_id.clone(),
            });
        }
        Ok(complete)
    }

    /// Writes `igdb`'s age ratings, and the minimum age they come to, where
    /// the game has none or has what the last apply wrote, like the other
    /// fields; records what it wrote in `row`. Whether they changed.
    async fn apply_age_ratings(
        &self,
        row: &mut IgdbMatch,
        igdb: Option<&IgdbGame>,
    ) -> Result<bool> {
        let store = &self.inner.store;
        let Some(current) = store.game_age_ratings(&row.game_id).await? else {
            return Ok(false);
        };
        if !current.ratings.is_empty() && row.applied.age_ratings.as_ref() != Some(&current.ratings)
        {
            // The game's own (a merge's, or the user's): left alone.
            row.applied.age_ratings = None;
            return Ok(false);
        }
        let ratings = igdb.map_or_else(Vec::new, |game| age::clean(&game.age_ratings));
        row.applied.age_ratings = (!ratings.is_empty()).then(|| ratings.clone());
        if ratings == current.ratings && current.min_age == age::min_age(&ratings) {
            return Ok(false);
        }
        store.set_game_age_ratings(&row.game_id, &ratings).await?;
        Ok(true)
    }

    /// Downloads the images applying `rows` will add, [`DOWNLOADS`] games
    /// at a time, counting each game on `progress` once its images are in.
    /// Stops starting downloads once a host can't be reached.
    pub(super) async fn prefetch(
        &self,
        rows: &[IgdbMatch],
        games: &HashMap<u64, IgdbGame>,
        progress: Option<&mut Progress>,
    ) -> Result<Prefetched> {
        let mut queued = Vec::with_capacity(rows.len());
        for row in rows {
            let game = row.igdb_id.and_then(|id| games.get(&id));
            queued.push(self.to_download(&row.game_id, game, false).await?);
        }
        Ok(self.download_all(queued, progress).await)
    }

    /// Downloads the images applying `igdb` to the game will add: also its
    /// hero and screenshots for the `page`.
    pub(super) async fn prefetch_game(
        &self,
        game_id: &str,
        igdb: Option<&IgdbGame>,
        page: bool,
    ) -> Result<Prefetched> {
        let urls = self.to_download(game_id, igdb, page).await?;
        Ok(self.download_all(vec![urls], None).await)
    }

    /// The URLs of the IGDB images the game lacks.
    async fn to_download(
        &self,
        game_id: &str,
        igdb: Option<&IgdbGame>,
        page: bool,
    ) -> Result<Vec<String>> {
        if igdb.is_none() || !self.can_download() {
            return Ok(Vec::new());
        }
        let existing = self.inner.store.artwork_for_game(game_id).await?;
        let wanted = wanted(igdb);
        Ok(missing(&existing, &wanted, page)
            .map(|(_, _, url)| (*url).to_owned())
            .collect())
    }

    /// Downloads each game's `queued` URLs, [`DOWNLOADS`] games at a time.
    async fn download_all(
        &self,
        queued: Vec<Vec<String>>,
        mut progress: Option<&mut Progress>,
    ) -> Prefetched {
        let offline = Arc::new(AtomicBool::new(false));
        let mut queued = queued.into_iter();
        let mut running = tokio::task::JoinSet::new();
        let mut prefetched = Prefetched::new();
        loop {
            while running.len() < DOWNLOADS
                && !offline.load(Ordering::Relaxed)
                && let Some(urls) = queued.next()
            {
                let this = self.clone();
                let offline = Arc::clone(&offline);
                running.spawn(async move {
                    let mut done = Vec::with_capacity(urls.len());
                    for url in urls {
                        if offline.load(Ordering::Relaxed) {
                            break;
                        }
                        if let Some(downloaded) = this.download(&url).await {
                            if matches!(&downloaded, Err(failed) if failed.retry == Retry::Offline)
                            {
                                offline.store(true, Ordering::Relaxed);
                            }
                            done.push((url, downloaded));
                        }
                    }
                    done
                });
            }
            let Some(done) = running.join_next().await else {
                break;
            };
            // A game whose task failed keeps its row pending.
            prefetched.extend(done.unwrap_or_default());
            if let Some(progress) = progress.as_deref_mut() {
                progress.advance();
            }
        }
        if offline.load(Ordering::Relaxed) {
            // The games never started still count as through.
            for _ in queued {
                if let Some(progress) = progress.as_deref_mut() {
                    progress.advance();
                }
            }
        }
        prefetched
    }

    /// Adds the IGDB cover, hero, icon and screenshots the game lacks and
    /// drops IGDB artwork it no longer has. A cover, hero or screenshots
    /// from elsewhere (RomM's) win; IGDB's icon is added beside an
    /// installer's, and the library shows IGDB's (`db::query`). The hero
    /// and screenshots only show on the game's page, so they download for
    /// it (`page`), not for the whole library. Images come from
    /// `prefetched` alone; nothing is downloaded here.
    /// Returns whether anything changed and whether every image came that
    /// may still come.
    pub(super) async fn apply_artwork(
        &self,
        game_id: &str,
        igdb: Option<&IgdbGame>,
        page: bool,
        prefetched: &Prefetched,
    ) -> Result<(bool, bool)> {
        let store = &self.inner.store;
        let wanted = wanted(igdb);
        let existing = store.artwork_for_game(game_id).await?;
        let mut changed = false;
        let mut complete = true;
        // The kind of each IGDB image wanted but not in yet, page-only
        // ones not asked for now included.
        let mut lacking: Vec<ArtworkKind> = missing(&existing, &wanted, true)
            .map(|(kind, _, _)| *kind)
            .collect();
        for (kind, source, url) in missing(&existing, &wanted, page) {
            match prefetched.get(*url) {
                Some(Ok(cached)) => {
                    store
                        .upsert_artwork(&cached.record(game_id, *kind, source.as_str()))
                        .await?;
                    changed = true;
                    take(&mut lacking, *kind);
                }
                Some(Err(failed)) => {
                    let error = &failed.message;
                    if failed.retry == Retry::Never {
                        tracing::warn!(game = game_id, %error, "IGDB artwork unavailable");
                    } else {
                        tracing::debug!(game = game_id, %error, "IGDB artwork not downloaded");
                        complete = false;
                    }
                }
                // Not tried (the network went, or the game's artwork
                // changed meanwhile): the next run tries.
                None => complete &= !self.can_download(),
            }
        }
        // IGDB images it no longer has go, except that each one still
        // lacking keeps one of its kind until it is in: a failed download
        // never leaves the game without a cover it had.
        for art in &existing {
            let stale = art.source.starts_with(SOURCE)
                && !wanted
                    .iter()
                    .any(|(kind, source, _)| *kind == art.kind && *source == art.source);
            if !stale {
                continue;
            }
            if take(&mut lacking, art.kind) {
                continue;
            }
            store.delete_artwork(&art.id).await?;
            changed = true;
        }
        Ok((changed, complete))
    }

    /// Whether images can be downloaded: there is a cache for them and
    /// network support.
    fn can_download(&self) -> bool {
        cfg!(feature = "net") && self.inner.artwork.is_some()
    }

    /// `None` without an artwork cache or network support.
    async fn download(&self, url: &str) -> Option<Downloaded> {
        #[cfg(feature = "net")]
        {
            let cache = self.inner.artwork.as_ref()?;
            Some(
                cache
                    .download_metadata(url)
                    .await
                    .map_err(|error| Failed::new(&error)),
            )
        }
        #[cfg(not(feature = "net"))]
        {
            // Without network support there is nothing to download into it.
            let _ = (url, &self.inner.artwork);
            None
        }
    }
}

/// Takes one `kind` out of `kinds`; whether there was one.
fn take(kinds: &mut Vec<ArtworkKind>, kind: ArtworkKind) -> bool {
    match kinds.iter().position(|other| *other == kind) {
        Some(index) => {
            kinds.swap_remove(index);
            true
        }
        None => false,
    }
}

/// The IGDB images `igdb` gives a game: its cover, hero, icon and first
/// screenshots.
fn wanted(igdb: Option<&IgdbGame>) -> Vec<Wanted<'_>> {
    let Some(game) = igdb else {
        return Vec::new();
    };
    let images = &game.images;
    let icon = images
        .artworks
        .iter()
        .filter(|art| art.image_type.as_ref().and_then(|kind| kind.as_u64()) == Some(ICON_TYPE))
        .max_by_key(|art| u64::from(art.width.unwrap_or(0)) * u64::from(art.height.unwrap_or(0)));
    [
        (ArtworkKind::Cover, images.cover.as_ref()),
        (ArtworkKind::Hero, images.hero.as_ref()),
        (ArtworkKind::Icon, icon),
    ]
    .into_iter()
    .filter_map(|(kind, image)| Some((kind, image?)))
    .chain(
        images
            .screenshots
            .iter()
            .take(SCREENSHOTS)
            .map(|image| (ArtworkKind::Screenshot, image)),
    )
    .map(|(kind, image)| {
        (
            kind,
            format!("{SOURCE}{}", image.image_id),
            image.url.as_str(),
        )
    })
    .collect()
}

/// Those of `wanted` to download: not in `existing`, and not page-only
/// (hero, screenshots) unless for the `page`.
fn missing<'a>(
    existing: &'a [Artwork],
    wanted: &'a [Wanted<'a>],
    page: bool,
) -> impl Iterator<Item = &'a Wanted<'a>> {
    wanted.iter().filter(move |(kind, source, _)| {
        let present = existing.iter().any(|art| {
            art.kind == *kind
                && (art.source == *source
                    || (*kind != ArtworkKind::Icon
                        && !art.source.starts_with(SOURCE)
                        // The user's choice shows over IGDB's, which
                        // still comes beneath it for Reset (#175).
                        && art.source != crate::model::USER_ARTWORK_SOURCE))
        });
        let page_only = matches!(kind, ArtworkKind::Hero | ArtworkKind::Screenshot);
        !present && (page || !page_only)
    })
}

/// Writes `igdb`'s fields into `game` where the game's own value is empty
/// or is what the last apply (`old`) wrote. Returns what it wrote.
pub(super) fn merge(game: &mut Game, old: &Applied, igdb: Option<&IgdbGame>) -> Applied {
    let empty = IgdbGame::default();
    let igdb = igdb.unwrap_or(&empty);
    let description = text(
        &mut game.description,
        &old.description,
        igdb.description.trim(),
    );
    let genres = list(&mut game.genres, &old.genres, clean(&igdb.genres));
    let tags = list(&mut game.tags, &old.tags, clean(&igdb.tags));
    let date = igdb
        .release_date
        .clone()
        .filter(|date| crate::model::validation::validate_date(date).is_ok());
    let release_date = match &game.release_date {
        Some(current) if old.release_date.as_ref() != Some(current) => None,
        _ => {
            game.release_date.clone_from(&date);
            date
        }
    };
    game.aliases.retain(|alias| !old.aliases.contains(alias));
    let mut aliases = Vec::new();
    for alias in clean(&igdb.aliases) {
        if alias != game.title && !game.aliases.contains(&alias) && game.aliases.len() < MAX_LIST {
            game.aliases.push(alias.clone());
            aliases.push(alias);
        }
    }
    Applied {
        description,
        genres,
        tags,
        release_date,
        aliases,
        // `Metadata::apply_age_ratings`'s: the ratings live beside the record.
        age_ratings: old.age_ratings.clone(),
    }
}

fn text(current: &mut String, old: &Option<String>, new: &str) -> Option<String> {
    if !current.is_empty() && old.as_deref() != Some(current.as_str()) {
        return None;
    }
    *current = new.to_owned();
    (!new.is_empty()).then(|| new.to_owned())
}

fn list(
    current: &mut Vec<String>,
    old: &Option<Vec<String>>,
    new: Vec<String>,
) -> Option<Vec<String>> {
    if !current.is_empty() && old.as_ref() != Some(current) {
        return None;
    }
    *current = new.clone();
    (!new.is_empty()).then_some(new)
}

/// Trimmed, non-empty, without duplicates, within the model's limits.
fn clean(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty()
            && value.len() <= 4096
            && !value.contains('\0')
            && !out.iter().any(|v| v == value)
        {
            out.push(value.to_owned());
        }
        if out.len() == MAX_LIST {
            break;
        }
    }
    out
}
