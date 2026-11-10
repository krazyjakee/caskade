//! Games added by hand (#174): what a program, shortcut or URI starts, games
//! that may be the same, and adding the record. Nothing on disk is copied,
//! moved or run. Forwarded to `caskade_app::commands::manual_games`.
use crate::error::CommandError;
use caskade_app::commands::manual_games as app;
#[cfg(test)]
use caskade_core::bus::Event;
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    manual::{DuplicateGame, ManualGame, ManualTarget},
};
use tauri::State;

/// What the chosen program, shortcut or URI starts, with the title it
/// suggests. Refused with the reason when it can't start a game.
#[tauri::command]
#[specta::specta]
pub async fn manual_game_target(input: String) -> Result<ManualTarget, CommandError> {
    app::manual_game_target(input).await
}

/// Games already in the library that may be the one being added.
#[tauri::command]
#[specta::specta]
pub async fn manual_game_duplicates(
    store: State<'_, LibraryStore>,
    title: String,
    target: Option<ManualTarget>,
) -> Result<Vec<DuplicateGame>, CommandError> {
    app::manual_game_duplicates(&store, &title, target.as_ref()).await
}

/// Adds the game and returns its id: a new game, or the target as a
/// release of the game it was linked to.
#[tauri::command]
#[specta::specta]
pub async fn manual_game_add(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game: ManualGame,
) -> Result<String, CommandError> {
    app::manual_game_add(&store, &bus, game).await
}

#[cfg(test)]
#[path = "manual_games_tests.rs"]
mod tests;
