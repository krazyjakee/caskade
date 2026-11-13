//! Games added by hand (#174): what a program, shortcut or URI starts, games
//! that may be the same, and adding the record. Nothing on disk is copied,
//! moved or run.
use caskade_core::{
    bus::{Bus, Event},
    db::LibraryStore,
    manual::{DuplicateGame, ManualGame, ManualTarget, resolve_target},
};

use crate::error::{CommandError, report};

/// What the chosen program, shortcut or URI starts, with the title it
/// suggests. Refused with the reason when it can't start a game.
pub async fn manual_game_target(input: String) -> Result<ManualTarget, CommandError> {
    tokio::task::spawn_blocking(move || resolve_target(&input))
        .await
        .map_err(|error| report(caskade_core::Error::Invalid(error.to_string())))?
        .map_err(report)
}

/// Games already in the library that may be the one being added.
pub async fn manual_game_duplicates(
    store: &LibraryStore,
    title: &str,
    target: Option<&ManualTarget>,
) -> Result<Vec<DuplicateGame>, CommandError> {
    store.manual_duplicates(title, target).await.map_err(report)
}

/// Adds the game and returns its id: a new game, or the target as a
/// release of the game it was linked to.
pub async fn manual_game_add(
    store: &LibraryStore,
    bus: &Bus,
    game: ManualGame,
) -> Result<String, CommandError> {
    crate::permissions::enforce("manual_game_add", serde_json::Value::default)?;
    let id = store.add_manual_game(&game).await.map_err(report)?;
    bus.emit(if game.link_to.is_some() {
        Event::LibraryGameUpdated {
            game_id: id.clone(),
        }
    } else {
        Event::LibraryGameAdded {
            game_id: id.clone(),
        }
    });
    Ok(id)
}

#[cfg(test)]
#[path = "manual_games_tests.rs"]
mod tests;
