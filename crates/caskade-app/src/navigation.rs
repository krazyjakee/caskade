//! Where the main window may go (CSK-14): only the app's own pages. A
//! navigation anywhere else, or a request for a new window, opens `http(s)`
//! pages in the system browser ([`open_in_browser`]) and drops anything
//! else, so a bug or a UI plugin can't turn the window into a remote page.

use url::Url;

/// The app's own origins: its bundled pages (`tauri://localhost`, or
/// `http(s)://tauri.localhost` on Windows), its artwork and theme schemes,
/// and in debug builds the dev server.
pub fn app_page(url: &Url, dev_url: Option<&Url>) -> bool {
    const SCHEMES: [&str; 3] = ["tauri", "caskade-asset", "caskade-theme"];
    match url.scheme() {
        scheme if SCHEMES.contains(&scheme) => true,
        "http" | "https" => {
            let bundled = url.port().is_none()
                && url
                    .host_str()
                    .and_then(|host| host.strip_suffix(".localhost"))
                    .is_some_and(|name| SCHEMES.contains(&name));
            let dev_server =
                cfg!(debug_assertions) && dev_url.is_some_and(|dev| dev.origin() == url.origin());
            bundled || dev_server
        }
        _ => url.as_str() == "about:blank",
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_apps_own_pages_load_in_the_main_window() {
        let dev = Url::parse("http://localhost:1420").unwrap();
        let page = |url: &str| app_page(&Url::parse(url).unwrap(), Some(&dev));
        for own in [
            "tauri://localhost/library?filter=installed",
            "http://tauri.localhost/settings",
            "https://tauri.localhost/",
            "caskade-asset://localhost/abc.webp",
            "http://caskade-theme.localhost/caskade.default/theme.css",
            "about:blank",
        ] {
            assert!(page(own), "{own}");
        }
        assert_eq!(
            page("http://localhost:1420/library"),
            cfg!(debug_assertions)
        );
        for elsewhere in [
            "https://evil.example/login",
            "http://localhost:8080/",
            "https://tauri.localhost.evil.example/",
            "http://tauri.localhost:8080/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "caskade-plugin://localhost/x/ui.js",
            "about:srcdoc",
        ] {
            assert!(!page(elsewhere), "{elsewhere}");
        }
    }
}
