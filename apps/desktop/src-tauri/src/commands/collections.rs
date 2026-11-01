//! Collection commands: manual lists and smart collections (saved library
//! queries). Persistence stays in caskade-core. Collections are the active
//! account's (#206): another account's id answers as not found.
use crate::error::{CommandError, report};
use caskade_core::{
    accounts::AccountService,
    db::LibraryStore,
    model::{Collection, CollectionSummary, GamePage, SaveCollection},
};
use tauri::State;

/// The signed-in account, from app state; never from the webview.
fn account(accounts: &AccountService) -> Result<String, CommandError> {
    accounts.require_active().map_err(report)
}

/// Every collection as its card shows it: pinned ones first, then by name.
#[tauri::command]
#[specta::specta]
pub async fn list_collections(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
) -> Result<Vec<CollectionSummary>, CommandError> {
    store
        .collection_summaries(&account(&accounts)?)
        .await
        .map_err(report)
}

/// A collection, with a manual one's games in the order they were added.
#[tauri::command]
#[specta::specta]
pub async fn get_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
) -> Result<Option<Collection>, CommandError> {
    store
        .get_collection(&account(&accounts)?, &id)
        .await
        .map_err(report)
}

/// A collection's games that aren't hidden: a smart one's in its saved
/// sort, a manual one's by title.
#[tauri::command]
#[specta::specta]
pub async fn collection_games(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
    offset: u32,
    limit: u32,
) -> Result<GamePage, CommandError> {
    store
        .collection_games(&account(&accounts)?, &id, offset, limit)
        .await
        .map_err(report)
}

/// Makes a collection, or changes one's name, description and rules.
#[tauri::command]
#[specta::specta]
pub async fn save_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    collection: SaveCollection,
) -> Result<Collection, CommandError> {
    store
        .save_collection(&account(&accounts)?, &collection)
        .await
        .map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn set_collection_pinned(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
    pinned: bool,
) -> Result<(), CommandError> {
    store
        .set_collection_pinned(&account(&accounts)?, &id, pinned)
        .await
        .map_err(report)
}

/// Deletes a collection; its games stay in the library.
#[tauri::command]
#[specta::specta]
pub async fn delete_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
) -> Result<bool, CommandError> {
    store
        .delete_collection(&account(&accounts)?, &id)
        .await
        .map_err(report)
}

/// Adds games to the end of a manual collection.
#[tauri::command]
#[specta::specta]
pub async fn add_to_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
    game_ids: Vec<String>,
) -> Result<(), CommandError> {
    store
        .add_to_collection(&account(&accounts)?, &id, &game_ids)
        .await
        .map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_from_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
    game_ids: Vec<String>,
) -> Result<(), CommandError> {
    store
        .remove_from_collection(&account(&accounts)?, &id, &game_ids)
        .await
        .map_err(report)
}

/// The manual collections a game is in.
#[tauri::command]
#[specta::specta]
pub async fn game_collections(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    game_id: String,
) -> Result<Vec<String>, CommandError> {
    store
        .game_collection_ids(&account(&accounts)?, &game_id)
        .await
        .map_err(report)
}

#[cfg(test)]
#[path = "collections_tests.rs"]
mod tests;
