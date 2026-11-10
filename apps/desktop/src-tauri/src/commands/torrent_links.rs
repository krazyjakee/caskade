//! Settings → Installer's magnet links and torrent files (#159): whether
//! the OS opens them with Caskade (`torrent_links.rs`).
use crate::error::CommandError;
use crate::torrent_links::TorrentLinks;
use caskade_app::commands::torrent_links as app;
/// Whether the OS opens magnet links and torrent files with Caskade.
#[tauri::command]
#[specta::specta]
pub async fn torrent_links_status() -> Result<TorrentLinks, CommandError> {
    app::torrent_links_status().await
}

/// Asks the OS to open them with Caskade (`open`), or hands them back to
/// the apps that had them. On Windows, opening shows Default apps, where
/// the user chooses Caskade.
#[tauri::command]
#[specta::specta]
pub async fn torrent_links_set(open: bool) -> Result<TorrentLinks, CommandError> {
    app::torrent_links_set(open).await
}

/// Windows: opens Default apps at Caskade's page.
#[tauri::command]
#[specta::specta]
pub async fn torrent_links_choose() -> Result<(), CommandError> {
    app::torrent_links_choose().await
}
