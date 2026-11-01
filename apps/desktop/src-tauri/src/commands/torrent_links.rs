//! Settings → Installer's magnet links and torrent files (#159): whether
//! the OS opens them with Caskade (`torrent_links.rs`).
use crate::torrent_links::{self, TorrentLinks};

/// Runs `work`, which may wait on `xdg-mime` or the registry, off the
/// async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

/// Whether the OS opens magnet links and torrent files with Caskade.
#[tauri::command]
#[specta::specta]
pub async fn torrent_links_status() -> Result<TorrentLinks, String> {
    blocking(torrent_links::status).await
}

/// Asks the OS to open them with Caskade (`open`), or hands them back to
/// the apps that had them. On Windows, opening shows Default apps, where
/// the user chooses Caskade.
#[tauri::command]
#[specta::specta]
pub async fn torrent_links_set(open: bool) -> Result<TorrentLinks, String> {
    blocking(move || torrent_links::set(open)).await
}

/// Windows: opens Default apps at Caskade's page.
#[tauri::command]
#[specta::specta]
pub async fn torrent_links_choose() -> Result<(), String> {
    blocking(torrent_links::choose).await
}
