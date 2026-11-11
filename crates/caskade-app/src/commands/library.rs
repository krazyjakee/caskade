//! Library commands. Queries and persistence stay in caskade-core.
use caskade_core::{
    accounts::AccountService,
    bus::{Bus, Event},
    db::LibraryStore,
    launch::LaunchService,
    metadata::Metadata,
    model::{
        GameDetails, GameFilter, GamePage, LibraryStats, ListGames, PlatformSummary, PlaySession,
        TitleLetter,
    },
    romm::RommService,
    settings::SettingsService,
};

use crate::error::{CommandError, report};

/// A page of the library, as the signed-in account sees it.
pub async fn list_games(
    store: &LibraryStore,
    accounts: &AccountService,
    query: &ListGames,
) -> Result<GamePage, CommandError> {
    store
        .list_games_for_account(query, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// A game with its entries, or `None`.
pub async fn get_game(store: &LibraryStore, id: &str) -> Result<Option<GameDetails>, CommandError> {
    store.get_game(id).await.map_err(report)
}

/// Downloads the images only a game's page shows, the first time it
/// opens: RomM's screenshots, then IGDB's hero and any screenshots RomM
/// has none of. Library events refresh the page as they come.
pub async fn game_page_images(
    romm: &RommService,
    metadata: &Metadata,
    settings: &SettingsService,
    game_id: &str,
) -> Result<(), CommandError> {
    // One after the other: IGDB's screenshots only fill in where RomM's
    // aren't there. RomM's step skips offline servers and is quick.
    if let Err(error) = romm.page_images(game_id).await {
        tracing::warn!(game = %game_id, %error, "RomM screenshots not fetched");
    }
    if !crate::onboarding::page_images_online(settings).await {
        return Ok(());
    }
    metadata
        .page_images(game_id)
        .await
        .map(drop)
        .map_err(report)
}

/// The platforms of the games in a library filter, with their counts,
/// playtime and last play.
pub async fn library_platforms(
    store: &LibraryStore,
    accounts: &AccountService,
    filter: GameFilter,
) -> Result<Vec<PlatformSummary>, CommandError> {
    store
        .library_platforms_for_account(filter, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// Where each initial letter starts in a library query sorted by title,
/// for the alphabet bar.
pub async fn title_letters(
    store: &LibraryStore,
    accounts: &AccountService,
    query: &ListGames,
) -> Result<Vec<TitleLetter>, CommandError> {
    store
        .title_letters_for_account(query, accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// The library's totals, as the signed-in account sees it.
pub async fn library_stats(
    store: &LibraryStore,
    accounts: &AccountService,
) -> Result<LibraryStats, CommandError> {
    store
        .library_stats_for_account(accounts.active_id().as_deref())
        .await
        .map_err(report)
}

/// Play sessions, newest first: a game's, or every game's; streams running
/// now come first.
pub async fn list_sessions(
    store: &LibraryStore,
    launch: &LaunchService,
    game_id: Option<&str>,
    limit: u32,
) -> Result<Vec<PlaySession>, CommandError> {
    let stored = match game_id {
        Some(id) => store.sessions_for_game(id, limit).await,
        None => store.recent_sessions(limit).await,
    }
    .map_err(report)?;
    // Streams running now (#264): followed like sessions, never stored.
    let mut sessions = launch.running_streams(game_id);
    sessions.extend(stored);
    Ok(sessions)
}

/// Hides a game from the library, or shows it again. A hidden game stays
/// hidden when scans find it again; Settings lists it (`hidden_games`).
pub async fn set_game_hidden(
    store: &LibraryStore,
    bus: &Bus,
    id: &str,
    hidden: bool,
) -> Result<(), CommandError> {
    crate::permissions::enforce("set_game_hidden", serde_json::Value::default)?;
    store.set_game_hidden(id, hidden).await.map_err(report)?;
    bus.emit(Event::LibraryGameUpdated {
        game_id: id.to_owned(),
    });
    Ok(())
}

/// Marks a game as a favourite, or not.
pub async fn set_game_favorite(
    store: &LibraryStore,
    bus: &Bus,
    id: &str,
    favorite: bool,
) -> Result<(), CommandError> {
    crate::permissions::enforce("set_game_favorite", serde_json::Value::default)?;
    store
        .set_game_favorite(id, favorite)
        .await
        .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated {
        game_id: id.to_owned(),
    });
    Ok(())
}

/// Chooses the entry a game's Play starts (`None`: the first that can
/// launch).
pub async fn set_game_default_entry(
    store: &LibraryStore,
    bus: &Bus,
    id: &str,
    entry_id: Option<&str>,
) -> Result<(), CommandError> {
    crate::permissions::enforce("set_game_default_entry", serde_json::Value::default)?;
    store
        .set_game_default_entry(id, entry_id)
        .await
        .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated {
        game_id: id.to_owned(),
    });
    Ok(())
}

/// Opens the folder an entry's files are in, in the file manager. The UI
/// names an entry, never a path.
pub async fn reveal_game_entry(store: &LibraryStore, entry_id: &str) -> Result<(), CommandError> {
    crate::permissions::enforce("reveal_game_entry", serde_json::Value::default)?;
    let folder = store.entry_folder(entry_id).await.map_err(report)?;
    open_file_manager([&folder], &folder)
}

/// Where an entry's files are, for Copy path: a managed install's real
/// folder, which the entry's own location doesn't spell out.
pub async fn game_entry_path(store: &LibraryStore, entry_id: &str) -> Result<String, CommandError> {
    crate::permissions::enforce("game_entry_path", serde_json::Value::default)?;
    let path = store.entry_path(entry_id).await.map_err(report)?;
    Ok(path.display().to_string())
}

const FILE_MANAGER: &str = if cfg!(target_os = "windows") {
    "explorer"
} else if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

/// Starts the OS's file manager with `args`, to show `folder`.
pub fn open_file_manager(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
    folder: &std::path::Path,
) -> Result<(), CommandError> {
    let mut manager = std::process::Command::new(FILE_MANAGER);
    // Not inside Caskade's AppImage (#274).
    caskade_core::managed::outside_own_appimage(&mut manager);
    let mut child = manager.args(args).spawn().map_err(|error| {
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
pub async fn hidden_games(
    store: &LibraryStore,
    offset: u32,
    limit: u32,
) -> Result<GamePage, CommandError> {
    store.hidden_games(offset, limit).await.map_err(report)
}
