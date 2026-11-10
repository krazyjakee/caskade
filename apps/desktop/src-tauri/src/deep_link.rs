//! `caskade://install` deep links (#57): `?url=` from
//! websites, `?path=` from the CLI. Any web page can open these links, so a
//! deep link only becomes a pending install request. Nothing is resolved,
//! downloaded or run until the user confirms it in the app
//! (`confirm_install_request`).

use crate::commands::install_requests::{InstallRequest, InstallRequests};
use caskade_core::accounts::Permission;
use tauri::{Manager, Runtime, Url};

pub use caskade_app::deep_link::{SCHEME, install_input};

/// Queues each link for confirmation, tells the UI and brings the window up.
/// A `?path=` is checked off the caller's thread, so await this on the async
/// runtime ([`open_in_background`] from the main thread).
pub async fn open<R: Runtime>(
    app: &tauri::AppHandle<R>,
    links: impl IntoIterator<Item = Url>,
) -> Vec<InstallRequest> {
    queue(
        app,
        links.into_iter().map(|link| install_input(&link)).collect(),
    )
    .await
}

/// Queues each install input from outside the app (deep links, and the
/// magnet links and torrent files the OS hands Caskade, `torrent_links.rs`)
/// for confirmation, tells the UI and brings the window up.
pub async fn queue<R: Runtime>(
    app: &tauri::AppHandle<R>,
    inputs: Vec<Result<String, String>>,
) -> Vec<InstallRequest> {
    let Some(requests) = app.try_state::<InstallRequests>() else {
        tracing::warn!("install requests unavailable; deep links ignored");
        return Vec::new();
    };
    // Only an account that may install queues one (#205); the UI shows why
    // not, since a link has no caller to answer.
    if let Err(error) = crate::permissions::require(app, Permission::Install) {
        tracing::warn!(%error, "refused deep links");
        if let Err(error) = crate::events::sink(app.clone()).emit(
            caskade_app::events::Event::InstallRefused(caskade_app::events::InstallRefused(error)),
        ) {
            tracing::warn!(%error, "couldn't tell the UI about refused deep links");
        }
        crate::main_window::raise(app);
        return Vec::new();
    }
    let mut queued = Vec::new();
    for input in inputs {
        let result = match input {
            Ok(input) => {
                requests
                    .request_and_emit(&crate::events::sink(app.clone()), input)
                    .await
            }
            Err(error) => Err(error),
        };
        // The link may carry tokens: log the outcome, never the link.
        match result {
            Ok(request) => {
                tracing::info!(request_id = %request.request_id, kind = ?request.kind, "deep link awaits confirmation");
                queued.push(request);
            }
            Err(error) => tracing::warn!(%error, "ignored a deep link"),
        }
    }
    if !queued.is_empty() {
        crate::main_window::raise(app);
    }
    queued
}

/// [`open`] on the async runtime, for callers on the main thread.
/// Store sign-in callbacks (#165) go to the store accounts, the rest are
/// install requests.
pub fn open_in_background<R: Runtime>(app: &tauri::AppHandle<R>, links: Vec<Url>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (pairs, links): (Vec<_>, Vec<_>) = links
            .into_iter()
            .partition(crate::commands::sync::is_pair_link);
        // Pairing links (#226) wait for an admin in the app.
        for link in pairs {
            crate::commands::sync::open_pair_link(&app, &link);
        }
        // Share links (#246) ask their sharer, then wait for confirmation.
        let (shares, links): (Vec<_>, Vec<_>) = links
            .into_iter()
            .partition(crate::commands::shares::is_share_link);
        for link in shares {
            crate::commands::shares::open_share_link(&app, link.to_string()).await;
        }
        let (callbacks, installs): (Vec<_>, Vec<_>) = links
            .into_iter()
            .partition(crate::commands::store_accounts::is_callback);
        for link in callbacks {
            crate::commands::store_accounts::callback(&app, link).await;
        }
        if !installs.is_empty() {
            open(&app, installs).await;
        }
    });
}

/// [`queue`] on the async runtime, for a magnet link or torrent file the
/// OS started Caskade with.
pub fn queue_in_background<R: Runtime>(app: &tauri::AppHandle<R>, input: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        queue(&app, vec![Ok(input)]).await;
    });
}

/// Handles links that started the app and links that arrive while it runs.
/// The single-instance plugin forwards a second launch's link here, and
/// its magnet link or torrent file (`torrent_links::input_from_args`).
pub fn listen<R: Runtime>(app: &tauri::AppHandle<R>) {
    use tauri_plugin_deep_link::DeepLinkExt;
    // Installed builds register the scheme through their bundle; dev runs
    // register it for the current executable.
    #[cfg(all(debug_assertions, any(windows, target_os = "linux")))]
    if let Err(error) = app.deep_link().register_all() {
        tracing::warn!(%error, "could not register the {SCHEME}: scheme");
    }
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        open_in_background(&handle, event.urls());
    });
    match app.deep_link().get_current() {
        Ok(Some(links)) => open_in_background(app, links),
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "could not read the launch deep link"),
    }
    let args = std::env::args().collect::<Vec<_>>();
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Some(input) = crate::torrent_links::input_from_args(&args, &cwd) {
        queue_in_background(app, input);
    }
    // The handler follows Caskade when it moves (an AppImage, an install).
    tauri::async_runtime::spawn_blocking(crate::torrent_links::refresh);
}

#[cfg(test)]
#[path = "deep_link_tests.rs"]
mod tests;
