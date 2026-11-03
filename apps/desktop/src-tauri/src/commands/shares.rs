//! One-off shares (#246, #249): Share with someone…, the shares this device
//! runs and who received them, and receiving a share from its link. A link
//! opened or pasted becomes an install request (kind `share`), with what
//! its sharer says it shares; nothing is fetched until it's confirmed.

use caskade_core::install::JobSnapshot;
use caskade_core::share::{ReceivedShare, ShareExpiry, ShareInfo, ShareRecord, Shares};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event;

use crate::error::{CommandError, report};

/// A share just made, and its link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareCreated {
    pub share: ShareRecord,
    pub link: String,
}

/// The shares, or what's received, changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct SharesChanged;

/// Shares release `entry_id` until `expiry`.
#[tauri::command]
#[specta::specta]
pub async fn share_game(
    shares: State<'_, Shares>,
    entry_id: String,
    expiry: ShareExpiry,
) -> Result<ShareCreated, CommandError> {
    let share = shares.share(&entry_id, expiry).await.map_err(report)?;
    let link = shares.link(&share.id).await.map_err(report)?;
    Ok(ShareCreated { share, link })
}

/// Every share this device made, the newest first.
#[tauri::command]
#[specta::specta]
pub async fn shares_list(shares: State<'_, Shares>) -> Result<Vec<ShareRecord>, CommandError> {
    shares.list().await.map_err(report)
}

/// A running share's link again, to copy.
#[tauri::command]
#[specta::specta]
pub async fn share_link(shares: State<'_, Shares>, id: String) -> Result<String, CommandError> {
    shares.link(&id).await.map_err(report)
}

/// A running share's link as a QR code (an SVG document), for a phone or
/// another screen to read in console mode (#247).
#[tauri::command]
#[specta::specta]
pub async fn share_qr(shares: State<'_, Shares>, id: String) -> Result<String, CommandError> {
    let link = shares.link(&id).await.map_err(report)?;
    qr_svg(&link).map_err(report)
}

/// `text` as a QR code's SVG, dark modules on a light ground with a quiet
/// zone, as scanners expect whatever the theme.
fn qr_svg(text: &str) -> caskade_core::Result<String> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M).map_err(|_| {
        caskade_core::Error::Invalid(caskade_core::tr!("the link is too long for a QR code"))
    })?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .build())
}

/// Stops share `id`: its link fails at once.
#[tauri::command]
#[specta::specta]
pub async fn share_stop(shares: State<'_, Shares>, id: String) -> Result<(), CommandError> {
    shares.stop(&id).await.map_err(report)
}

/// Forgets a stopped or expired share.
#[tauri::command]
#[specta::specta]
pub async fn share_forget(shares: State<'_, Shares>, id: String) -> Result<(), CommandError> {
    shares.forget(&id).await.map_err(report)
}

/// What a share link shares, asked of its sharer; nothing is fetched.
#[tauri::command]
#[specta::specta]
pub async fn share_inspect(
    shares: State<'_, Shares>,
    link: String,
) -> Result<ShareInfo, CommandError> {
    shares.inspect(&link).await.map_err(report)
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
    shares.receive(&link, root_id).await.map_err(report)
}

/// Shares received here that are arriving or helping others receive them.
#[tauri::command]
#[specta::specta]
pub fn shares_received(shares: State<'_, Shares>) -> Vec<ReceivedShare> {
    shares.received()
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
    shares.keep_helping(&job, on).map_err(report)
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
    let mut changes = shares.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            changes.recv().await
        {
            let _ = SharesChanged.emit(&handle);
        }
    });
    let resumed = shares.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = resumed.resume().await {
            tracing::warn!(%error, "shares didn't start again");
        }
    });
}

/// Whether `link` is a share link (`caskade://receive?t=…`).
pub fn is_share_link(link: &tauri::Url) -> bool {
    caskade_core::share::is_link(link.as_str())
}

/// Asks a share link's sharer what it shares, then queues it for
/// confirmation and brings the window up; when the link doesn't work, the
/// UI is told why. Never logs the link: it's a credential.
pub async fn open_share_link<R: Runtime>(app: &AppHandle<R>, link: String) {
    if let Err(error) = queue_share_link(app, link).await {
        tracing::info!("an opened share link didn't queue");
        let _ = super::install_requests::InstallRefused(error).emit(app);
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
    let info = shares.inspect(&link).await.map_err(|e| e.to_string())?;
    requests.queue_share(app, link, info)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_link_makes_a_qr_code() {
        let link = format!("caskade://receive?t=caskadeshare{}", "a".repeat(400));
        let svg = super::qr_svg(&link).unwrap();
        assert!(svg.starts_with("<?xml"), "{}", &svg[..40]);
        assert!(svg.contains("<svg") && svg.contains("<path"));
        assert!(super::qr_svg(&"a".repeat(5000)).is_err());
    }
}
