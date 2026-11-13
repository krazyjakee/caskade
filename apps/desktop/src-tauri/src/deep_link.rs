//! `caskade://install` deep links (#57): `?url=` from
//! websites, `?path=` from the CLI. Any web page can open these links, so a
//! deep link only becomes a pending install request. Nothing is resolved,
//! downloaded or run until the user confirms it in the app
//! (`confirm_install_request`).

use crate::commands::install_requests::{InstallRequest, InstallRequests};
use caskade_app::deep_link::{LinkServices, SCHEME};
use tauri::{Manager, Runtime, Url};

/// Queues each install input from outside the app (the magnet links and
/// torrent files the OS hands Caskade, `torrent_links.rs`) for
/// confirmation, tells the UI and brings the window up.
pub async fn queue<R: Runtime>(
    app: &tauri::AppHandle<R>,
    inputs: Vec<Result<String, String>>,
) -> Vec<InstallRequest> {
    let (queued, raise) = caskade_app::deep_link::queue(
        &crate::events::sink(app.clone()),
        app.try_state::<InstallRequests>().as_deref(),
        app.try_state::<caskade_core::accounts::AccountService>()
            .as_deref(),
        inputs,
    )
    .await;
    if raise {
        crate::main_window::raise(app);
    }
    queued
}

/// Opens `links` (`caskade_app::deep_link::open`) on the async runtime, for
/// callers on the main thread, and brings the window up when they ask. A
/// store sign-in callback (#165) brings it up too.
pub fn open_in_background<R: Runtime>(app: &tauri::AppHandle<R>, links: Vec<Url>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let requests = app.try_state::<InstallRequests>();
        let accounts = app.try_state::<caskade_core::accounts::AccountService>();
        let shares = app.try_state::<caskade_share::Shares>();
        let store_accounts = app.try_state::<caskade_core::store_accounts::StoreAccounts>();
        let opened = app.try_state::<crate::commands::sync::OpenedPairingLink>();
        let signed_in = store_accounts.is_some()
            && links
                .iter()
                .any(crate::commands::store_accounts::is_callback);
        let services = LinkServices {
            events: crate::events::sink(app.clone()),
            requests: requests.as_deref(),
            accounts: accounts.as_deref(),
            shares: shares.as_deref(),
            store_accounts: store_accounts.as_deref(),
            opened_pairing_link: opened.as_deref(),
        };
        if caskade_app::deep_link::open(services, links).await || signed_in {
            crate::main_window::raise(&app);
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

/// Handles links that started the app and links that arrive while it runs,
/// a second launch's included.
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
    // The handler follows Caskade when it moves (an AppImage, an install).
    tauri::async_runtime::spawn_blocking(crate::torrent_links::refresh);
}

#[cfg(test)]
#[path = "deep_link_tests.rs"]
mod tests;
