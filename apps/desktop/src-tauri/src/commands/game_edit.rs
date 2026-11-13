//! Game editing (#171): the editor's view of a game and validated patches.
//! Fields the user sets stay theirs through automatic refreshes until reset.
use crate::error::CommandError;
pub use app::*;
use caskade_app::commands::game_edit as app;
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    db::LibraryStore,
    metadata::{Metadata, RefreshMode, RefreshStatus},
    model::{
        ArtworkKind, ArtworkSlot, BulkGameResult, Game, GameEdit, GameEditOutcome, GameEditState,
        ListGames,
    },
};
use tauri::State;
/// The game as the editor shows it: its values, the automatic values
/// beneath, and which fields the user set. `null` when it is gone.
#[tauri::command]
#[specta::specta]
pub async fn game_edit_state(
    store: State<'_, LibraryStore>,
    game_id: String,
) -> Result<Option<GameEditState>, CommandError> {
    app::game_edit_state(&store, game_id).await
}

/// Saves the changed fields, all or none. `base` is the game as the
/// editor loaded it: a field changed since is a conflict, not overwritten.
#[tauri::command]
#[specta::specta]
pub async fn game_edit(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_id: String,
    edit: GameEdit,
    base: Option<Game>,
) -> Result<GameEditOutcome, CommandError> {
    app::game_edit(&store, &bus, game_id, edit, base).await
}

/// The game's cover, background and icon as the editor shows them: who
/// chose each, what shows, and the automatic images to choose from. `null`
/// when the game is gone.
#[tauri::command]
#[specta::specta]
pub async fn game_artwork(
    store: State<'_, LibraryStore>,
    game_id: String,
) -> Result<Option<Vec<ArtworkSlot>>, CommandError> {
    app::game_artwork(&store, game_id).await
}

/// Changes one kind of the game's artwork; refreshes never undo it. An
/// image that fails to import changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn game_artwork_set(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    cache: State<'_, EditArtwork>,
    game_id: String,
    kind: ArtworkKind,
    change: ArtworkChange,
) -> Result<Vec<ArtworkSlot>, CommandError> {
    app::game_artwork_set(&store, &bus, &cache, game_id, kind, change).await
}

/// One edit on many games (#173): a preview (`apply` false) saves nothing;
/// applied, each game saves on its own, and the result says which did.
/// The library hears of the changed games (merged into few events).
#[tauri::command]
#[specta::specta]
pub async fn games_bulk_edit(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_ids: Vec<String>,
    edit: GameEdit,
    apply: bool,
) -> Result<Vec<BulkGameResult>, CommandError> {
    app::games_bulk_edit(&store, &bus, game_ids, edit, apply).await
}

/// Every game a library query finds, in its order, for "select all".
#[tauri::command]
#[specta::specta]
pub async fn library_game_ids(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    query: ListGames,
) -> Result<Vec<String>, CommandError> {
    app::library_game_ids(&store, &accounts, query).await
}

/// Refreshes the chosen games' details and artwork from the built-in
/// sources, in the background; Activity follows it. Refused while one runs.
#[tauri::command]
#[specta::specta]
pub async fn games_refresh_start(
    metadata: State<'_, Metadata>,
    game_ids: Vec<String>,
    mode: RefreshMode,
) -> Result<(), CommandError> {
    app::games_refresh_start(&metadata, game_ids, mode).await
}

/// Stops the refresh after its current batch; whether one was running.
#[tauri::command]
#[specta::specta]
pub fn games_refresh_cancel(metadata: State<'_, Metadata>) -> bool {
    app::games_refresh_cancel(&metadata)
}

/// The last refresh's status, or `null` when none ran.
#[tauri::command]
#[specta::specta]
pub fn games_refresh_status(metadata: State<'_, Metadata>) -> Option<RefreshStatus> {
    app::games_refresh_status(&metadata)
}

#[cfg(test)]
#[path = "game_edit_tests.rs"]
mod tests;
