//! One-off shares (#246, #249): Share with someone…, the shares this device
//! runs and who received them, and receiving a share from its link. A link
//! opened or pasted becomes an install request (kind `share`), with what
//! its sharer says it shares; nothing is fetched until it's confirmed.
//!
//! Forwarded to `caskade_app::commands::shares`, but for opening and
//! queueing share links, which go to the webview's install confirmations.
use caskade_app::commands::shares as app;
use caskade_core::install::JobSnapshot;
use caskade_share::{ReceivedShare, ShareExpiry, ShareInfo, ShareRecord, Shares};
use tauri::{AppHandle, Manager, Runtime, State};

use crate::error::{CommandError, report};

pub use app::{ShareCreated, is_share_link};

/// Shares release `entry_id` until `expiry`.
#[tauri::command]
#[specta::specta]
pub async fn share_game(
    shares: State<'_, Shares>,
    entry_id: String,
    expiry: ShareExpiry,
) -> Result<ShareCreated, CommandError> {
    app::share_game(&shares, &entry_id, expiry).await
}

/// Every share this device made, the newest first.
#[tauri::command]
#[specta::specta]
pub async fn shares_list(shares: State<'_, Shares>) -> Result<Vec<ShareRecord>, CommandError> {
    app::shares_list(&shares).await
}

/// A running share's link again, to copy.
#[tauri::command]
#[specta::specta]
pub async fn share_link(shares: State<'_, Shares>, id: String) -> Result<String, CommandError> {
    app::share_link(&shares, &id).await
}

/// A running share's link as a QR code (an SVG document), for a phone or
/// another screen to read in console mode (#247).
#[tauri::command]
#[specta::specta]
pub async fn share_qr(shares: State<'_, Shares>, id: String) -> Result<String, CommandError> {
    app::share_qr(&shares, &id).await
}

/// Stops share `id`: its link fails at once.
#[tauri::command]
#[specta::specta]
pub async fn share_stop(shares: State<'_, Shares>, id: String) -> Result<(), CommandError> {
    app::share_stop(&shares, &id).await
}

/// Forgets a stopped or expired share.
#[tauri::command]
#[specta::specta]
pub async fn share_forget(shares: State<'_, Shares>, id: String) -> Result<(), CommandError> {
    app::share_forget(&shares, &id).await
}

/// What a share link shares, asked of its sharer; nothing is fetched.
#[tauri::command]
#[specta::specta]
pub async fn share_inspect(
    shares: State<'_, Shares>,
    link: String,
) -> Result<ShareInfo, CommandError> {
    app::share_inspect(&shares, &link).await
}

/// Receives the share `link` links to into storage root `root_id` (the
/// default when absent), as a copy job.
#[tauri::command]
#[specta::specta]
pub async fn share_receive(
    shares: State<'_, Shares>,
    link: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, CommandError> {
    app::share_receive(&shares, &link, root_id).await
}

/// Shares received here that are arriving or helping others receive them.
#[tauri::command]
#[specta::specta]
pub fn shares_received(shares: State<'_, Shares>) -> Vec<ReceivedShare> {
    app::shares_received(&shares)
}

/// Whether the share received as job `job` goes on helping the share's
/// other recipients once it's here (#249).
#[tauri::command]
#[specta::specta]
pub fn share_keep_helping(
    shares: State<'_, Shares>,
    job: String,
    on: bool,
) -> Result<(), CommandError> {
    app::share_keep_helping(&shares, &job, on)
}

/// Confirms a share request (#247) whose link shares a collection: it joins
/// the signed-in account's collections. The link never leaves the adapter.
#[tauri::command]
#[specta::specta]
pub async fn confirm_share_collection(
    requests: State<'_, super::install_requests::InstallRequests>,
    accounts: State<'_, caskade_core::accounts::AccountService>,
    request_id: String,
) -> Result<caskade_core::model::Collection, CommandError> {
    let account = accounts.require_active().map_err(report)?;
    requests
        .confirm_collection(&request_id, &account)
        .await
        .map_err(|reason| report(caskade_core::Error::Invalid(reason)))
}

/// Runs the shares that were running, and tells the UI of changes.
pub fn setup<R: Runtime>(app: &AppHandle<R>, shares: &Shares) {
    app::setup(crate::events::sink(app.clone()), shares);
}

/// Asks a share link's sharer what it shares, then queues it for
/// confirmation and brings the window up; when the link doesn't work, the
/// UI is told why. Never logs the link: it's a credential.
pub async fn open_share_link<R: Runtime>(app: &AppHandle<R>, link: String) {
    if let Err(error) = queue_share_link(app, link).await {
        tracing::info!("an opened share link didn't queue");
        crate::events::sink(app.clone()).send(caskade_app::events::Event::InstallRefused(
            caskade_app::events::InstallRefused(error),
        ));
    }
    crate::main_window::raise(app);
}

/// Queues `link` (a share link) as an install request; the reason when its
/// sharer refuses it, it expired, or nobody may install here.
pub async fn queue_share_link<R: Runtime>(
    app: &AppHandle<R>,
    link: String,
) -> Result<super::install_requests::InstallRequest, String> {
    crate::permissions::require(app, caskade_core::accounts::Permission::Install)?;
    let (Some(shares), Some(requests)) = (
        app.try_state::<Shares>(),
        app.try_state::<super::install_requests::InstallRequests>(),
    ) else {
        return Err(caskade_core::tr!("receiving shares isn't available"));
    };
    app::queue_share_link(&crate::events::sink(app.clone()), &shares, &requests, link).await
}

#[cfg(test)]
mod tests {
    use super::super::install_requests::InstallRequests;
    use crate::commands::test_ipc::invoke;
    use caskade_share::{ShareContents, ShareInfo, ShareReach};
    use serde_json::json;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    fn info() -> ShareInfo {
        ShareInfo {
            sharer_id: "a".repeat(64),
            sharer: "Alex's Desk".into(),
            kind: "game".into(),
            title: "Hades".into(),
            size: Some(15_000_000_000),
            windows: true,
            extras: Vec::new(),
            expires_at_ms: None,
            reach: ShareReach::Direct,
            contents: ShareContents::Game,
        }
    }

    #[test]
    fn a_share_request_shows_what_it_shares_and_never_its_link() {
        let queue = InstallRequests::default();
        let store =
            tauri::async_runtime::block_on(caskade_core::db::LibraryStore::in_memory()).unwrap();
        let service = caskade_core::install::InstallService::new(
            store,
            caskade_core::bus::Bus::default(),
            caskade_core::install::InstallOptions::default(),
        );
        let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
            .dangerously_cast_bigints_to_number()
            .commands(tauri_specta::collect_commands![
                crate::commands::install_requests::pending_install_requests,
                crate::commands::install_requests::confirm_install_request
            ])
            .events(tauri_specta::collect_events![
                super::super::install_requests::InstallRequested
            ]);
        let app = mock_builder()
            .manage(service.clone())
            .manage(queue.clone())
            .invoke_handler(specta.invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        specta.mount_events(&app);
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let link = "caskade://receive?t=caskadesharesecretgrantbytes";
        let events = crate::events::sink(app.handle().clone());
        let first = queue.queue_share(&events, link.to_owned(), info()).unwrap();
        // The same link twice is one request.
        let again = queue.queue_share(&events, link.to_owned(), info()).unwrap();
        assert_eq!(first.request_id, again.request_id);
        let pending = invoke(&window, "pending_install_requests", json!({})).unwrap();
        assert_eq!(pending.as_array().unwrap().len(), 1);
        assert_eq!(pending[0]["kind"], "share");
        assert_eq!(pending[0]["share"]["sharer"], "Alex's Desk");
        assert_eq!(pending[0]["value"], "Hades");
        assert!(!pending.to_string().contains("secretgrant"), "{pending}");
        // Nothing to receive it with: refused, and it still waits.
        let refused = invoke(
            &window,
            "confirm_install_request",
            json!({"requestId": first.request_id, "rootId": null}),
        );
        assert!(refused.is_err());
        assert!(service.jobs().is_empty());
        assert_eq!(queue.pending().len(), 1);
    }
}
