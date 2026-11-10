//! The main window shows only the app's own pages (CSK-14). A navigation
//! anywhere else, or a request for a new window (`target="_blank"`), opens
//! `http(s)` pages in the system browser and drops anything else, so a bug
//! or a UI plugin can't turn the window into a remote page.

use caskade_app::navigation::{app_page, open_in_browser};
use tauri::{AppHandle, Runtime, WebviewWindow, WebviewWindowBuilder, webview::NewWindowResponse};

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
