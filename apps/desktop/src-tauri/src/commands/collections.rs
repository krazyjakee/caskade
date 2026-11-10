//! Collection commands: manual lists and smart collections (saved library
//! queries). Persistence stays in caskade-core. Collections are the active
//! account's (#206): another account's id answers as not found. Forwarded
//! to `caskade_app::commands::collections`.
use crate::error::CommandError;
use caskade_app::commands::collections as app;
use caskade_core::{
    accounts::AccountService,
    db::LibraryStore,
    model::{Collection, CollectionSummary, GamePage, SaveCollection},
};
use tauri::State;

/// Every collection as its card shows it: pinned ones first, then by name.
#[tauri::command]
#[specta::specta]
pub async fn list_collections(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
) -> Result<Vec<CollectionSummary>, CommandError> {
    app::list_collections(&store, &accounts).await
}

/// A collection, with a manual one's games in the order they were added.
#[tauri::command]
#[specta::specta]
pub async fn get_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
) -> Result<Option<Collection>, CommandError> {
    app::get_collection(&store, &accounts, &id).await
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
    app::collection_games(&store, &accounts, &id, offset, limit).await
}

/// Makes a collection, or changes one's name, description and rules.
#[tauri::command]
#[specta::specta]
pub async fn save_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    collection: SaveCollection,
) -> Result<Collection, CommandError> {
    app::save_collection(&store, &accounts, &collection).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_collection_pinned(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
    pinned: bool,
) -> Result<(), CommandError> {
    app::set_collection_pinned(&store, &accounts, &id, pinned).await
}

/// Deletes a collection; its games stay in the library.
#[tauri::command]
#[specta::specta]
pub async fn delete_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
) -> Result<bool, CommandError> {
    app::delete_collection(&store, &accounts, &id).await
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
    app::add_to_collection(&store, &accounts, &id, &game_ids).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_from_collection(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    id: String,
    game_ids: Vec<String>,
) -> Result<(), CommandError> {
    app::remove_from_collection(&store, &accounts, &id, &game_ids).await
}

/// The manual collections a game is in.
#[tauri::command]
#[specta::specta]
pub async fn game_collections(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    game_id: String,
) -> Result<Vec<String>, CommandError> {
    app::game_collections(&store, &accounts, &game_id).await
}

#[cfg(test)]
#[path = "collections_tests.rs"]
mod tests;
