//! Collection commands: manual lists and smart collections (saved library
//! queries). Persistence stays in caskade-core. Collections are the active
//! account's (#206): another account's id answers as not found.
use caskade_core::{
    accounts::AccountService,
    db::LibraryStore,
    model::{Collection, CollectionSummary, GamePage, SaveCollection},
};

use crate::error::{CommandError, report};

/// The signed-in account, from app state; never from the UI.
fn account(accounts: &AccountService) -> Result<String, CommandError> {
    accounts.require_active().map_err(report)
}

/// Every collection as its card shows it: pinned ones first, then by name.
pub async fn list_collections(
    store: &LibraryStore,
    accounts: &AccountService,
) -> Result<Vec<CollectionSummary>, CommandError> {
    store
        .collection_summaries(&account(accounts)?)
        .await
        .map_err(report)
}

/// A collection, with a manual one's games in the order they were added.
pub async fn get_collection(
    store: &LibraryStore,
    accounts: &AccountService,
    id: &str,
) -> Result<Option<Collection>, CommandError> {
    store
        .get_collection(&account(accounts)?, id)
        .await
        .map_err(report)
}

/// A collection's games that aren't hidden: a smart one's in its saved
/// sort, a manual one's by title.
pub async fn collection_games(
    store: &LibraryStore,
    accounts: &AccountService,
    id: &str,
    offset: u32,
    limit: u32,
) -> Result<GamePage, CommandError> {
    store
        .collection_games(&account(accounts)?, id, offset, limit)
        .await
        .map_err(report)
}

/// Makes a collection, or changes one's name, description and rules.
pub async fn save_collection(
    store: &LibraryStore,
    accounts: &AccountService,
    collection: &SaveCollection,
) -> Result<Collection, CommandError> {
    store
        .save_collection(&account(accounts)?, collection)
        .await
        .map_err(report)
}

/// Pins a collection to the top of the list, or unpins it.
pub async fn set_collection_pinned(
    store: &LibraryStore,
    accounts: &AccountService,
    id: &str,
    pinned: bool,
) -> Result<(), CommandError> {
    store
        .set_collection_pinned(&account(accounts)?, id, pinned)
        .await
        .map_err(report)
}

/// Deletes a collection; its games stay in the library.
pub async fn delete_collection(
    store: &LibraryStore,
    accounts: &AccountService,
    id: &str,
) -> Result<bool, CommandError> {
    store
        .delete_collection(&account(accounts)?, id)
        .await
        .map_err(report)
}

/// Adds games to the end of a manual collection.
pub async fn add_to_collection(
    store: &LibraryStore,
    accounts: &AccountService,
    id: &str,
    game_ids: &[String],
) -> Result<(), CommandError> {
    store
        .add_to_collection(&account(accounts)?, id, game_ids)
        .await
        .map_err(report)
}

/// Takes games out of a manual collection.
pub async fn remove_from_collection(
    store: &LibraryStore,
    accounts: &AccountService,
    id: &str,
    game_ids: &[String],
) -> Result<(), CommandError> {
    store
        .remove_from_collection(&account(accounts)?, id, game_ids)
        .await
        .map_err(report)
}

/// The manual collections a game is in.
pub async fn game_collections(
    store: &LibraryStore,
    accounts: &AccountService,
    game_id: &str,
) -> Result<Vec<String>, CommandError> {
    store
        .game_collection_ids(&account(accounts)?, game_id)
        .await
        .map_err(report)
}
