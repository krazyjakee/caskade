//! The main window shows only the app's own pages (CSK-14). A navigation
//! anywhere else, or a request for a new window (`target="_blank"`), opens
//! `http(s)` pages in the system browser and drops anything else, so a bug
//! or a UI plugin can't turn the window into a remote page.

use tauri::{
    AppHandle, Runtime, Url, WebviewWindow, WebviewWindowBuilder, webview::NewWindowResponse,
};

/// Builds the main window from its `tauri.conf.json` entry (`"create": false`
/// there, so this is the only one) with the navigation guard.
pub fn create_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .unwrap_or_default();
    let dev_url = app.config().build.dev_url.clone();
    let builder = WebviewWindowBuilder::from_config(app, &config)?;
    let builder = match crate::webview_recovery::take_route() {
        Some(route) => {
            // Restore through a relative navigation in the bundled origin;
            // never load an external origin from the recovery file.
            let script = format!(
                "if (!sessionStorage.getItem('caskade.recovered')) {{ sessionStorage.setItem('caskade.recovered', '1'); location.replace({}); }}",
                serde_json::to_string(&route).unwrap()
            );
            builder.initialization_script(script)
        }
        None => builder,
    };
    builder
        .on_navigation(move |url| {
            let allowed = app_page(url, dev_url.as_ref());
            if !allowed {
                open_in_browser(url);
            }
            allowed
        })
        .on_new_window(|url, _| {
            open_in_browser(&url);
            NewWindowResponse::Deny
        })
        .build()
}

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
fn open_in_browser(url: &Url) {
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
