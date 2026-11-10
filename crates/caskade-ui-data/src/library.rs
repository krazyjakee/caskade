//! Library data for views: pages, search, details, totals and sessions,
//! and the changes a view makes. Every call goes through the app layer's
//! commands (`caskade_app::commands`), so the rules, the signed-in account
//! and the bus events are the same as the webview's.

use caskade_app::commands::{collections, library};
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    db::LibraryStore,
    error::ErrorReport,
    launch::LaunchService,
    model::{
        Collection, CollectionSummary, GameDetails, GameFilter, GamePage, LibraryStats, ListGames,
        PlatformSummary, PlaySession, SaveCollection, TitleLetter,
    },
};

/// The most summaries one page holds (#107): a view pages through the rest.
pub const PAGE_LIMIT: u32 = 100;

/// The results the top bar's search shows.
pub const SEARCH_LIMIT: u32 = 6;

type Result<T> = std::result::Result<T, ErrorReport>;

/// What library views read and change, over one app's services.
#[derive(Clone)]
pub struct Library {
    store: LibraryStore,
    accounts: AccountService,
    launch: LaunchService,
    bus: Bus,
}

impl Library {
    pub fn new(
        store: LibraryStore,
        accounts: AccountService,
        launch: LaunchService,
        bus: Bus,
    ) -> Self {
        Self {
            store,
            accounts,
            launch,
            bus,
        }
    }

    /// The library's data in a running app.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(
            services.store.clone(),
            services.accounts.clone(),
            services.launch.clone(),
            services.bus.clone(),
        )
    }

    pub(crate) fn store(&self) -> &LibraryStore {
        &self.store
    }

    pub(crate) fn bus(&self) -> &Bus {
        &self.bus
    }

    /// A page of the library as the signed-in account sees it; at most
    /// [`PAGE_LIMIT`] summaries, whatever `query.limit` asks.
    pub async fn page(&self, query: &ListGames) -> Result<GamePage> {
        let query = ListGames {
            limit: query.limit.clamp(1, PAGE_LIMIT),
            ..query.clone()
        };
        library::list_games(&self.store, &self.accounts, &query).await
    }

    /// The top bar's search: the best [`SEARCH_LIMIT`] matches for `text`
    /// across the whole library.
    pub async fn search(&self, text: &str) -> Result<GamePage> {
        let query = ListGames {
            query: Some(text.to_owned()),
            limit: SEARCH_LIMIT,
            ..ListGames::default()
        };
        library::list_games(&self.store, &self.accounts, &query).await
    }

    /// A game with its releases; `None` when there is no such game.
    pub async fn game(&self, id: &str) -> Result<Option<GameDetails>> {
        library::get_game(&self.store, id).await
    }

    /// The library's totals.
    pub async fn stats(&self) -> Result<LibraryStats> {
        library::library_stats(&self.store, &self.accounts).await
    }

    /// The platforms of a filter's games.
    pub async fn platforms(&self, filter: GameFilter) -> Result<Vec<PlatformSummary>> {
        library::library_platforms(&self.store, &self.accounts, filter).await
    }

    /// Where each initial letter starts in a title-sorted query.
    pub async fn letters(&self, query: &ListGames) -> Result<Vec<TitleLetter>> {
        library::title_letters(&self.store, &self.accounts, query).await
    }

    /// Play sessions, newest first: a game's, or the most recent ones.
    pub async fn sessions(&self, game_id: Option<&str>, limit: u32) -> Result<Vec<PlaySession>> {
        library::list_sessions(&self.store, &self.launch, game_id, limit).await
    }

    /// A page of the hidden games, by title.
    pub async fn hidden(&self, offset: u32, limit: u32) -> Result<GamePage> {
        library::hidden_games(&self.store, offset, limit.clamp(1, PAGE_LIMIT)).await
    }

    /// Marks a game as a favourite, or not.
    pub async fn set_favorite(&self, id: &str, favorite: bool) -> Result<()> {
        library::set_game_favorite(&self.store, &self.bus, id, favorite).await
    }

    /// Hides a game, or shows it again.
    pub async fn set_hidden(&self, id: &str, hidden: bool) -> Result<()> {
        library::set_game_hidden(&self.store, &self.bus, id, hidden).await
    }

    /// The release a game's Play starts; `None` for the first that can.
    pub async fn set_default_entry(&self, id: &str, entry_id: Option<&str>) -> Result<()> {
        library::set_game_default_entry(&self.store, &self.bus, id, entry_id).await
    }

    /// The signed-in account's collections, as their cards show them.
    pub async fn collections(&self) -> Result<Vec<CollectionSummary>> {
        collections::list_collections(&self.store, &self.accounts).await
    }

    /// A page of a collection's games.
    pub async fn collection_games(&self, id: &str, offset: u32, limit: u32) -> Result<GamePage> {
        collections::collection_games(
            &self.store,
            &self.accounts,
            id,
            offset,
            limit.clamp(1, PAGE_LIMIT),
        )
        .await
    }

    /// Makes a collection or changes one.
    pub async fn save_collection(&self, collection: &SaveCollection) -> Result<Collection> {
        collections::save_collection(&self.store, &self.accounts, collection).await
    }

    /// Adds games to the end of a manual collection.
    pub async fn add_to_collection(&self, id: &str, game_ids: &[String]) -> Result<()> {
        collections::add_to_collection(&self.store, &self.accounts, id, game_ids).await
    }
}
