//! Opening external links safely (CSK-14): [`open_in_browser`] opens only
//! `http(s)` pages in the system browser and drops anything else.

use url::Url;

/// Opens an `http(s)` page in the system browser; anything else is
/// dropped.
pub fn open_in_browser(url: &Url) {
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/open")
    } else {
        std::process::Command::new("xdg-open")
    };
    // The browser starts outside Caskade's AppImage (#274).
    caskade_core::managed::outside_own_appimage(&mut command);
    if let Err(error) = command.arg(url.as_str()).spawn() {
        tracing::warn!(%error, "couldn't open the page in the browser");
    }
}
