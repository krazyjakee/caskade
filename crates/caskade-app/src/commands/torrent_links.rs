//! Settings → Installer's magnet links and torrent files (#159): whether
//! the OS opens them with Caskade (`torrent_links.rs`).
use crate::error::{CommandError, internal};
use crate::torrent_links::{self, TorrentLinks};
/// Runs `work`, which may wait on `xdg-mime` or the registry, off the
/// async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, CommandError> {
    crate::runtime::spawn_blocking(work)
        .await
        .map_err(internal)?
        .map_err(CommandError::from)
}

/// Whether the OS opens magnet links and torrent files with Caskade.
pub async fn torrent_links_status() -> Result<TorrentLinks, CommandError> {
    blocking(torrent_links::status).await
}

/// Asks the OS to open them with Caskade (`open`), or hands them back to
/// the apps that had them. On Windows, opening shows Default apps, where
/// the user chooses Caskade.
pub async fn torrent_links_set(open: bool) -> Result<TorrentLinks, CommandError> {
    blocking(move || torrent_links::set(open)).await
}

/// Windows: opens Default apps at Caskade's page.
pub async fn torrent_links_choose() -> Result<(), CommandError> {
    blocking(torrent_links::choose).await
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn only_windows_chooses_the_app_in_its_settings() {
        let error = crate::runtime::block_on(torrent_links_choose()).unwrap_err();
        assert_eq!(error.code, ErrorCode::Other);
        assert!(error.summary.contains("Only Windows"), "{error:?}");
    }
}
