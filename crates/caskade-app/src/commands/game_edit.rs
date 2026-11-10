//! Game editing (#171): the editor's view of a game and validated patches.
//! Fields the user sets stay theirs through automatic refreshes until reset.
use crate::error::{CommandError, report};
use caskade_core::{
    accounts::AccountService,
    artwork::ArtworkCache,
    bus::{Bus, Event},
    db::LibraryStore,
    metadata::{Metadata, RefreshMode, RefreshStatus},
    model::{
        ArtworkKind, ArtworkSlot, BulkGameResult, BulkStatus, Game, GameEdit, GameEditOutcome,
        GameEditState, ListGames,
    },
};
use std::path::PathBuf;
/// The artwork cache images the user imports go into; `None` when it
/// couldn't open, and then imports fail.
pub struct EditArtwork(pub Option<ArtworkCache>);

/// The game as the editor shows it: its values, the automatic values
/// beneath, and which fields the user set. `null` when it is gone.
pub async fn game_edit_state(
    store: &LibraryStore,
    game_id: String,
) -> Result<Option<GameEditState>, CommandError> {
    store.game_edit_state(&game_id).await.map_err(report)
}

/// Saves the changed fields, all or none. `base` is the game as the
/// editor loaded it: a field changed since is a conflict, not overwritten.
pub async fn game_edit(
    store: &LibraryStore,
    bus: &Bus,
    game_id: String,
    edit: GameEdit,
    base: Option<Game>,
) -> Result<GameEditOutcome, CommandError> {
    let outcome = store
        .edit_game(&game_id, &edit, base.as_ref())
        .await
        .map_err(report)?;
    if let GameEditOutcome::Saved { changed, .. } = &outcome
        && !changed.is_empty()
    {
        bus.emit(Event::LibraryGameUpdated { game_id });
    }
    Ok(outcome)
}

/// The game's cover, background and icon as the editor shows them: who
/// chose each, what shows, and the automatic images to choose from. `null`
/// when the game is gone.
pub async fn game_artwork(
    store: &LibraryStore,
    game_id: String,
) -> Result<Option<Vec<ArtworkSlot>>, CommandError> {
    store.artwork_slots(&game_id).await.map_err(report)
}

/// What the user does with one kind of a game's artwork.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ArtworkChange {
    /// Shows one of the game's images of that kind.
    Choose { artwork_id: String },
    /// Imports an image file and shows it.
    Import { path: PathBuf },
    /// Shows none.
    Remove,
    /// Back to the automatic image.
    Reset,
}

/// Changes one kind of the game's artwork; refreshes never undo it. An
/// image that fails to import changes nothing.
pub async fn game_artwork_set(
    store: &LibraryStore,
    bus: &Bus,
    cache: &EditArtwork,
    game_id: String,
    kind: ArtworkKind,
    change: ArtworkChange,
) -> Result<Vec<ArtworkSlot>, CommandError> {
    match change {
        ArtworkChange::Choose { artwork_id } => {
            store.choose_artwork(&game_id, kind, &artwork_id).await
        }
        ArtworkChange::Import { path } => {
            let Some(cache) = &cache.0 else {
                return Err(report(caskade_core::Error::Invalid(caskade_core::tr!(
                    "the artwork cache isn't available"
                ))));
            };
            store
                .import_user_artwork(cache, &game_id, kind, &path)
                .await
                .map(drop)
        }
        ArtworkChange::Remove => store.remove_artwork(&game_id, kind).await,
        ArtworkChange::Reset => store.reset_artwork(&game_id, kind).await,
    }
    .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated {
        game_id: game_id.clone(),
    });
    Ok(store
        .artwork_slots(&game_id)
        .await
        .map_err(report)?
        .unwrap_or_default())
}

/// One edit on many games (#173): a preview (`apply` false) saves nothing;
/// applied, each game saves on its own, and the result says which did.
/// The library hears of the changed games (merged into few events).
pub async fn games_bulk_edit(
    store: &LibraryStore,
    bus: &Bus,
    game_ids: Vec<String>,
    edit: GameEdit,
    apply: bool,
) -> Result<Vec<BulkGameResult>, CommandError> {
    let results = store
        .bulk_edit_games(&game_ids, &edit, apply)
        .await
        .map_err(report)?;
    if apply {
        for result in &results {
            if result.status == BulkStatus::Changed {
                bus.emit(Event::LibraryGameUpdated {
                    game_id: result.game_id.clone(),
                });
            }
        }
    }
    Ok(results)
}

/// Every game a library query finds, in its order, for "select all".
pub async fn library_game_ids(
    store: &LibraryStore,
    accounts: &AccountService,
    query: ListGames,
) -> Result<Vec<String>, CommandError> {
    store
        .game_ids_for_account(&query, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// Refreshes the chosen games' details and artwork from the built-in
/// sources, in the background; Activity follows it. Refused while one runs.
pub async fn games_refresh_start(
    metadata: &Metadata,
    game_ids: Vec<String>,
    mode: RefreshMode,
) -> Result<(), CommandError> {
    metadata.start_refresh(game_ids, mode).map_err(report)
}

/// Stops the refresh after its current batch; whether one was running.
pub fn games_refresh_cancel(metadata: &Metadata) -> bool {
    metadata.cancel_refresh()
}

/// The last refresh's status, or `null` when none ran.
pub fn games_refresh_status(metadata: &Metadata) -> Option<RefreshStatus> {
    metadata.refresh_updates().borrow().clone()
}
