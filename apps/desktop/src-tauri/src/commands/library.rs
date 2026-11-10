//! Library commands. Queries and persistence stay in caskade-core.
//! Forwarded to `caskade_app::commands::library`.
use crate::error::CommandError;
use caskade_app::commands::library as app;

/// The file manager, for the other commands that show a folder.
pub(super) use app::open_file_manager;
use caskade_core::{
    accounts::AccountService,
    bus::Bus,
    db::LibraryStore,
    metadata::Metadata,
    model::{
        GameDetails, GameFilter, GamePage, LibraryStats, ListGames, PlatformSummary, PlaySession,
        TitleLetter,
    },
    romm::RommService,
    settings::SettingsService,
};
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn list_games(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    query: ListGames,
) -> Result<GamePage, CommandError> {
    app::list_games(&store, &accounts, &query).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_game(
    store: State<'_, LibraryStore>,
    id: String,
) -> Result<Option<GameDetails>, CommandError> {
    app::get_game(&store, &id).await
}

/// Downloads the images only a game's page shows, the first time it
/// opens: RomM's screenshots, then IGDB's hero and any screenshots RomM
/// has none of. Library events refresh the page as they come.
#[tauri::command]
#[specta::specta]
pub async fn game_page_images(
    romm: State<'_, RommService>,
    metadata: State<'_, Metadata>,
    settings: State<'_, SettingsService>,
    game_id: String,
) -> Result<(), CommandError> {
    app::game_page_images(&romm, &metadata, &settings, &game_id).await
}

/// The platforms of the games in a library filter, with their counts,
/// playtime and last play.
#[tauri::command]
#[specta::specta]
pub async fn library_platforms(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    filter: GameFilter,
) -> Result<Vec<PlatformSummary>, CommandError> {
    app::library_platforms(&store, &accounts, filter).await
}

/// Where each initial letter starts in a library query sorted by title,
/// for the alphabet bar.
#[tauri::command]
#[specta::specta]
pub async fn title_letters(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
    query: ListGames,
) -> Result<Vec<TitleLetter>, CommandError> {
    app::title_letters(&store, &accounts, &query).await
}

#[tauri::command]
#[specta::specta]
pub async fn library_stats(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
) -> Result<LibraryStats, CommandError> {
    app::library_stats(&store, &accounts).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_sessions(
    store: State<'_, LibraryStore>,
    launch: State<'_, caskade_core::launch::LaunchService>,
    game_id: Option<String>,
    limit: u32,
) -> Result<Vec<PlaySession>, CommandError> {
    app::list_sessions(&store, &launch, game_id.as_deref(), limit).await
}

/// Hides a game from the library, or shows it again. A hidden game stays
/// hidden when scans find it again; Settings lists it (`hidden_games`).
#[tauri::command]
#[specta::specta]
pub async fn set_game_hidden(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    id: String,
    hidden: bool,
) -> Result<(), CommandError> {
    app::set_game_hidden(&store, &bus, &id, hidden).await
}

/// Marks a game as a favourite, or not.
#[tauri::command]
#[specta::specta]
pub async fn set_game_favorite(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    id: String,
    favorite: bool,
) -> Result<(), CommandError> {
    app::set_game_favorite(&store, &bus, &id, favorite).await
}

/// Chooses the entry a game's Play starts (`null`: the first that can
/// launch).
#[tauri::command]
#[specta::specta]
pub async fn set_game_default_entry(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    id: String,
    entry_id: Option<String>,
) -> Result<(), CommandError> {
    app::set_game_default_entry(&store, &bus, &id, entry_id.as_deref()).await
}

/// Opens the folder an entry's files are in, in the file manager. The
/// webview names an entry, never a path.
#[tauri::command]
#[specta::specta]
pub async fn reveal_game_entry(
    store: State<'_, LibraryStore>,
    entry_id: String,
) -> Result<(), CommandError> {
    app::reveal_game_entry(&store, &entry_id).await
}

/// Where an entry's files are, for Copy path: a managed install's real
/// folder, which the entry's own location doesn't spell out.
#[tauri::command]
#[specta::specta]
pub async fn game_entry_path(
    store: State<'_, LibraryStore>,
    entry_id: String,
) -> Result<String, CommandError> {
    app::game_entry_path(&store, &entry_id).await
}

/// The hidden games, by title.
#[tauri::command]
#[specta::specta]
pub async fn hidden_games(
    store: State<'_, LibraryStore>,
    offset: u32,
    limit: u32,
) -> Result<GamePage, CommandError> {
    app::hidden_games(&store, offset, limit).await
}
