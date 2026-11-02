//! Library commands. Queries and persistence stay in caskade-core.
use crate::error::{CommandError, report};
use caskade_core::{
    accounts::AccountService,
    bus::{Bus, Event},
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
    store
        .list_games_for_account(&query, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn get_game(
    store: State<'_, LibraryStore>,
    id: String,
) -> Result<Option<GameDetails>, CommandError> {
    store.get_game(&id).await.map_err(report)
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
    // One after the other: IGDB's screenshots only fill in where RomM's
    // aren't there. RomM's step skips offline servers and is quick.
    if let Err(error) = romm.page_images(&game_id).await {
        tracing::warn!(game = %game_id, %error, "RomM screenshots not fetched");
    }
    if !crate::onboarding::page_images_online(&settings).await {
        return Ok(());
    }
    metadata
        .page_images(&game_id)
        .await
        .map(drop)
        .map_err(report)
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
    store
        .library_platforms_for_account(filter, accounts.active_id().as_deref())
        .await
        .map_err(report)
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
    store
        .title_letters_for_account(&query, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn library_stats(
    store: State<'_, LibraryStore>,
    accounts: State<'_, AccountService>,
) -> Result<LibraryStats, CommandError> {
    store
        .library_stats_for_account(accounts.active_id().as_deref())
        .await
        .map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn list_sessions(
    store: State<'_, LibraryStore>,
    launch: State<'_, caskade_core::launch::LaunchService>,
    game_id: Option<String>,
    limit: u32,
) -> Result<Vec<PlaySession>, CommandError> {
    let stored = match &game_id {
        Some(id) => store.sessions_for_game(id, limit).await,
        None => store.recent_sessions(limit).await,
    }
    .map_err(report)?;
    // Streams running now (#264): followed like sessions, never stored.
    let mut sessions = launch.running_streams(game_id.as_deref());
    sessions.extend(stored);
    Ok(sessions)
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
    store.set_game_hidden(&id, hidden).await.map_err(report)?;
    bus.emit(Event::LibraryGameUpdated { game_id: id });
    Ok(())
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
    store
        .set_game_favorite(&id, favorite)
        .await
        .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated { game_id: id });
    Ok(())
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
    store
        .set_game_default_entry(&id, entry_id.as_deref())
        .await
        .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated { game_id: id });
    Ok(())
}

/// Opens the folder an entry's files are in, in the file manager. The
/// webview names an entry, never a path.
#[tauri::command]
#[specta::specta]
pub async fn reveal_game_entry(
    store: State<'_, LibraryStore>,
    entry_id: String,
) -> Result<(), CommandError> {
    let folder = store.entry_folder(&entry_id).await.map_err(report)?;
    open_file_manager([&folder], &folder)
}

const FILE_MANAGER: &str = if cfg!(target_os = "windows") {
    "explorer"
} else if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

/// Starts the OS's file manager with `args`, to show `folder`.
pub(super) fn open_file_manager(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
    folder: &std::path::Path,
) -> Result<(), CommandError> {
    let mut child = std::process::Command::new(FILE_MANAGER)
        .args(args)
        .spawn()
        .map_err(|error| {
            // Name what couldn't be shown, keep the OS's reason as detail.
            let mut failure = CommandError::from_error(&error);
            failure.summary = caskade_core::tr!(
                "Couldn't open the file manager ({opener}) to show “{folder}”.",
                opener = FILE_MANAGER,
                folder = folder.display()
            );
            crate::error::logged(failure)
        })?;
    // Reaped off the async runtime; the file manager outlives it.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The hidden games, by title.
#[tauri::command]
#[specta::specta]
pub async fn hidden_games(
    store: State<'_, LibraryStore>,
    offset: u32,
    limit: u32,
) -> Result<GamePage, CommandError> {
    store.hidden_games(offset, limit).await.map_err(report)
}
