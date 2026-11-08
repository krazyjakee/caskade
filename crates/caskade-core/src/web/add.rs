//! Add web game: a page becomes a library game.

use super::{
    PLATFORM, WEB_SOURCE,
    page::{self, Picture},
    page_url,
};
use crate::{
    artwork::{ArtworkCache, CachedArtwork},
    bus::{Bus, Event},
    db::LibraryStore,
    launch::WebPlayer,
    model::{ArtworkKind, Game, GameEntry, LaunchSpec},
};
use caskade_discovery::InstallState;
use std::time::Duration;
use url::Url;

/// How much of a page is read for its details.
const PAGE_LIMIT: usize = 2 * 1024 * 1024;
/// How long reading the page may take.
const PAGE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long finding a picture may take, all candidates together.
const PICTURE_TIMEOUT: Duration = Duration::from_secs(30);
/// How many of the page's pictures are downloaded, at most.
const PICTURE_TRIES: usize = 6;
/// The artwork source of pictures from a game's page.
const SOURCE: &str = "web";

/// Adds web games to the library, and keeps them for offline play
/// (`offline.rs`).
#[derive(Clone)]
pub struct WebGames {
    pub(super) store: LibraryStore,
    artwork: Option<ArtworkCache>,
    pub(super) bus: Option<Bus>,
    pub(super) player: Option<WebPlayer>,
}

impl WebGames {
    pub fn new(store: LibraryStore) -> Self {
        Self {
            store,
            artwork: None,
            bus: None,
            player: None,
        }
    }

    /// Stores the page's picture as the game's cover and icon.
    pub fn with_artwork(mut self, artwork: ArtworkCache) -> Self {
        self.artwork = Some(artwork);
        self
    }

    /// The game window, which captures games for offline play.
    pub fn with_player(mut self, player: WebPlayer) -> Self {
        self.player = Some(player);
        self
    }

    /// Announces added games (`library.game.added`).
    pub fn with_bus(mut self, bus: Bus) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Adds the game at `input`, a page address, and returns its id. A page
    /// already in the library returns its game, with its pictures looked
    /// for again. A page that can't be read is still added, named after its
    /// host and without a picture; a store's page that says it has no game
    /// to play (it has to be bought, or it's a download) isn't.
    pub async fn add(&self, input: &str) -> crate::Result<String> {
        let url = page_url(input)?;
        if let Some(entry) = self
            .store
            .entry_by_source(WEB_SOURCE.as_str(), url.as_str())
            .await?
        {
            match read_page(&url).await {
                Ok(info) => {
                    self.pictures(&entry.game_id, &info).await;
                    if let Some(bus) = &self.bus {
                        bus.emit(Event::LibraryGameUpdated {
                            game_id: entry.game_id.clone(),
                        });
                    }
                }
                Err(error) => {
                    tracing::warn!(host = url.host_str(), %error, "web game page not read again");
                }
            }
            return Ok(entry.game_id);
        }
        let info = match read_page(&url).await {
            Ok(info) => {
                if let Some(why) = info.unplayable {
                    return Err(crate::Error::Invalid(why.message()));
                }
                info
            }
            Err(error) => {
                tracing::warn!(host = url.host_str(), %error, "web game page not read");
                page::PageInfo::default()
            }
        };
        let mut game = Game::new(info.title.clone().unwrap_or_else(|| host_title(&url)));
        game.platforms = vec![PLATFORM.to_owned()];
        game.description = info.description.clone().unwrap_or_default();
        let game = self.store.upsert_game(&game).await?;
        let mut entry = GameEntry::new(&game.id, WEB_SOURCE, url.as_str());
        entry.state = InstallState::Installed;
        entry.launch = Some(LaunchSpec::Web { url: url.into() });
        self.store.upsert_entry(&entry).await?;
        self.pictures(&game.id, &info).await;
        if let Some(bus) = &self.bus {
            bus.emit(Event::LibraryGameAdded {
                game_id: game.id.clone(),
            });
        }
        Ok(game.id)
    }

    /// Removes `game_id`'s web game: its offline copy and the game window's
    /// data (its saves) are deleted, then its entry, with its play sessions.
    /// A game left with no other entry leaves the library. Scans never
    /// bring a web game back, so this is how one goes, where other games
    /// are hidden.
    pub async fn remove(&self, game_id: &str) -> crate::Result<()> {
        let entry = self.web_entry(game_id).await?;
        if self
            .store
            .in_flight_sessions()
            .await?
            .iter()
            .any(|session| session.entry_id == entry.id)
        {
            return Err(crate::Error::Invalid("quit the game first".into()));
        }
        self.delete_bundle(&entry).await?;
        if let Some(player) = &self.player {
            match tokio::fs::remove_dir_all(player.data_dir(&entry.id)).await {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
        }
        self.store.delete_entry(&entry.id).await?;
        let removed = self.store.entries_for_game(game_id).await?.is_empty()
            && self.store.delete_game(game_id).await?;
        if let Some(bus) = &self.bus {
            let game_id = game_id.to_owned();
            bus.emit(if removed {
                Event::LibraryGameRemoved { game_id }
            } else {
                Event::LibraryGameUpdated { game_id }
            });
        }
        tracing::info!(game = game_id, removed, "web game removed");
        Ok(())
    }

    /// `game_id`'s web release.
    pub(super) async fn web_entry(&self, game_id: &str) -> crate::Result<GameEntry> {
        self.store
            .entries_for_game(game_id)
            .await?
            .into_iter()
            .find(|entry| entry.source_id == WEB_SOURCE)
            .ok_or_else(|| crate::Error::Invalid("that isn't a web game".into()))
    }

    /// Stores the page's pictures for the game, within [`PICTURE_TIMEOUT`];
    /// a failure leaves the game as it was.
    async fn pictures(&self, game_id: &str, info: &page::PageInfo) {
        let Some(artwork) = &self.artwork else {
            return;
        };
        match tokio::time::timeout(
            PICTURE_TIMEOUT,
            self.choose(artwork, game_id, &info.pictures),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(game = game_id, %error, "web game picture not stored"),
            Err(_) => tracing::warn!(game = game_id, "web game picture timed out"),
        }
    }

    /// The game's tile (cover), icon and hero from `pictures`, best first.
    /// The tile is the first that downloads and that no other game already
    /// has: sites often share one icon between their games. The hero is
    /// the first wide one; the icon the tile when it's square, else the
    /// first square one. They replace the pictures an earlier look at the
    /// page stored, and nothing else.
    async fn choose(
        &self,
        artwork: &ArtworkCache,
        game_id: &str,
        pictures: &[Picture],
    ) -> crate::Result<()> {
        let mut downloads = Downloads::new(artwork, pictures);
        let mut shared = None;
        let mut cover = None;
        while let Some((picture, cached)) = downloads.next().await {
            if self
                .store
                .artwork_shared(&cached.content_hash, game_id, ArtworkKind::Cover)
                .await?
            {
                shared.get_or_insert((picture, cached));
                continue;
            }
            cover = Some((picture, cached));
            break;
        }
        // Every picture the page has is another game's too: still better
        // than none.
        let Some((cover_picture, cover)) = cover.or(shared) else {
            return Ok(());
        };
        let wide =
            |cached: &CachedArtwork| u64::from(cached.width) * 2 >= u64::from(cached.height) * 3;
        let square = |cached: &CachedArtwork| {
            let (long, short) = (
                cached.width.max(cached.height),
                cached.width.min(cached.height),
            );
            short > 0 && u64::from(long) * 10 <= u64::from(short) * 12
        };
        let hero = match pictures
            .iter()
            .find(|picture| picture.wide() && picture.url != cover_picture.url)
        {
            Some(picture) => downloads.one(picture).await,
            None => None,
        }
        .or_else(|| wide(&cover).then(|| cover.clone()));
        let icon = if square(&cover) {
            Some(cover.clone())
        } else {
            match pictures
                .iter()
                .find(|picture| picture.square() && picture.url != cover_picture.url)
            {
                Some(picture) => downloads.one(picture).await,
                None => None,
            }
        }
        .unwrap_or_else(|| cover.clone());
        let mut chosen = vec![(ArtworkKind::Cover, cover), (ArtworkKind::Icon, icon)];
        if let Some(hero) = hero {
            chosen.push((ArtworkKind::Hero, hero));
        }
        let earlier = self.store.artwork_for_game(game_id).await?;
        for (kind, cached) in chosen {
            for old in earlier
                .iter()
                .filter(|old| old.kind == kind && old.source == SOURCE)
            {
                self.store.delete_artwork(&old.id).await?;
            }
            self.store
                .upsert_artwork(&cached.record(game_id, kind, SOURCE))
                .await?;
        }
        Ok(())
    }
}

/// The pictures of a page, downloaded in order, at most [`PICTURE_TRIES`];
/// it stops when their host can't be reached.
struct Downloads<'a> {
    artwork: &'a ArtworkCache,
    pictures: std::slice::Iter<'a, Picture>,
    tries: usize,
    offline: bool,
}

impl<'a> Downloads<'a> {
    fn new(artwork: &'a ArtworkCache, pictures: &'a [Picture]) -> Self {
        Self {
            artwork,
            pictures: pictures.iter(),
            tries: 0,
            offline: false,
        }
    }

    async fn next(&mut self) -> Option<(&'a Picture, CachedArtwork)> {
        while let Some(picture) = self.pictures.next() {
            if let Some(cached) = self.one(picture).await {
                return Some((picture, cached));
            }
            if self.offline || self.tries >= PICTURE_TRIES {
                return None;
            }
        }
        None
    }

    /// `picture`, when it downloads and the budget allows.
    async fn one(&mut self, picture: &Picture) -> Option<CachedArtwork> {
        if self.offline || self.tries >= PICTURE_TRIES {
            return None;
        }
        self.tries += 1;
        match self.artwork.download(picture.url.as_str()).await {
            Ok(cached) => Some(cached),
            Err(error) => {
                tracing::debug!(host = picture.url.host_str(), %error, "web game picture skipped");
                self.offline = error.unreachable();
                None
            }
        }
    }
}

/// The page's details, from at most [`PAGE_LIMIT`] bytes of its HTML. A page
/// with no picture made to present it also gets its oEmbed thumbnail and
/// its web app manifest's pictures.
pub(super) async fn read_page(url: &Url) -> crate::Result<page::PageInfo> {
    let client = reqwest::Client::builder()
        .timeout(PAGE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!(
            "Mozilla/5.0 (compatible; Caskade/",
            env!("CARGO_PKG_VERSION"),
            ")"
        ))
        .build()
        .map_err(crate::error::net)?;
    let (base, html) = fetch(&client, url, "text/html,application/xhtml+xml").await?;
    // Relative links resolve against where the page ended up.
    let mut info = page::parse(&html, &base);
    if !info.has_preview() {
        if let Some(endpoint) = info.oembed.clone() {
            match fetch(&client, &endpoint, "application/json").await {
                Ok((base, json)) => info.add_pictures(page::oembed(&json, &base)),
                Err(error) => tracing::debug!(%error, "web game oEmbed not read"),
            }
        }
        if let Some(manifest) = info.manifest.clone() {
            match fetch(
                &client,
                &manifest,
                "application/manifest+json,application/json",
            )
            .await
            {
                Ok((base, json)) => info.add_pictures(page::manifest(&json, &base)),
                Err(error) => tracing::debug!(%error, "web game manifest not read"),
            }
        }
    }
    Ok(info)
}

/// At most [`PAGE_LIMIT`] bytes of `url`, as text, and where it ended up.
async fn fetch(client: &reqwest::Client, url: &Url, accept: &str) -> crate::Result<(Url, String)> {
    let mut response = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT, accept)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(crate::error::net)?;
    let base = response.url().clone();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(crate::error::net)? {
        let room = PAGE_LIMIT - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() == PAGE_LIMIT {
            break;
        }
    }
    Ok((base, String::from_utf8_lossy(&body).into_owned()))
}

/// `www.example.com` → `example.com`, for a page with no title.
fn host_title(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    host.strip_prefix("www.").unwrap_or(host).to_owned()
}
