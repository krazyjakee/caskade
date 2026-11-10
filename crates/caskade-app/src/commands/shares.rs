//! One-off shares (#246, #249): Share with someone…, the shares this device
//! runs and who received them, and receiving a share from its link. A link
//! opened or pasted becomes an install request (kind `share`), with what
//! its sharer says it shares; nothing is fetched until it's confirmed.

use caskade_core::install::JobSnapshot;
use caskade_share::{ReceivedShare, ShareExpiry, ShareInfo, ShareRecord, Shares};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{CommandError, report};
use crate::events::{Event, Events};

/// A share just made, and its link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareCreated {
    pub share: ShareRecord,
    pub link: String,
}

/// The shares, or what's received, changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SharesChanged;

/// Shares release `entry_id` until `expiry`.
pub async fn share_game(
    shares: &Shares,
    entry_id: &str,
    expiry: ShareExpiry,
) -> Result<ShareCreated, CommandError> {
    let share = shares.share(entry_id, expiry).await.map_err(report)?;
    let link = shares.link(&share.id).await.map_err(report)?;
    Ok(ShareCreated { share, link })
}

/// Every share this device made, the newest first.
pub async fn shares_list(shares: &Shares) -> Result<Vec<ShareRecord>, CommandError> {
    shares.list().await.map_err(report)
}

/// A running share's link again, to copy.
pub async fn share_link(shares: &Shares, id: &str) -> Result<String, CommandError> {
    shares.link(id).await.map_err(report)
}

/// A running share's link as a QR code (an SVG document), for a phone or
/// another screen to read in console mode (#247).
pub async fn share_qr(shares: &Shares, id: &str) -> Result<String, CommandError> {
    let link = shares.link(id).await.map_err(report)?;
    qr_svg(&link).map_err(report)
}

/// `text` as a QR code's SVG, dark modules on a light ground with a quiet
/// zone, as scanners expect whatever the theme.
fn qr_svg(text: &str) -> caskade_core::Result<String> {
    let code =
        qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M).map_err(|_| {
            caskade_core::Error::Invalid(caskade_core::tr!("the link is too long for a QR code"))
        })?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .build())
}

/// Stops share `id`: its link fails at once.
pub async fn share_stop(shares: &Shares, id: &str) -> Result<(), CommandError> {
    shares.stop(id).await.map_err(report)
}

/// Forgets a stopped or expired share.
pub async fn share_forget(shares: &Shares, id: &str) -> Result<(), CommandError> {
    shares.forget(id).await.map_err(report)
}

/// What a share link shares, asked of its sharer; nothing is fetched.
pub async fn share_inspect(shares: &Shares, link: &str) -> Result<ShareInfo, CommandError> {
    shares.inspect(link).await.map_err(report)
}

/// Receives the share `link` links to into storage root `root_id` (the
/// default when absent), as a copy job.
pub async fn share_receive(
    shares: &Shares,
    link: &str,
    root_id: Option<String>,
) -> Result<JobSnapshot, CommandError> {
    shares.receive(link, root_id).await.map_err(report)
}

/// Shares received here that are arriving or helping others receive them.
pub fn shares_received(shares: &Shares) -> Vec<ReceivedShare> {
    shares.received()
}

/// Whether the share received as job `job` goes on helping the share's
/// other recipients once it's here (#249).
pub fn share_keep_helping(shares: &Shares, job: &str, on: bool) -> Result<(), CommandError> {
    shares.keep_helping(job, on).map_err(report)
}

/// Runs the shares that were running, and tells the UI of changes.
pub fn setup(events: Events, shares: &Shares) {
    let mut changes = shares.subscribe();
    crate::runtime::spawn(async move {
        while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            changes.recv().await
        {
            let _ = events.emit(Event::SharesChanged(SharesChanged));
        }
    });
    let resumed = shares.clone();
    crate::runtime::spawn(async move {
        if let Err(error) = resumed.resume().await {
            tracing::warn!(%error, "shares didn't start again");
        }
    });
}

/// Queues `link` (a share link) as an install request with what its sharer
/// says it shares; the reason when the sharer refuses it or it expired.
pub async fn queue_share_link(
    events: &Events,
    shares: &Shares,
    requests: &super::install_requests::InstallRequests,
    link: String,
) -> Result<super::install_requests::InstallRequest, String> {
    let info = shares.inspect(&link).await.map_err(|e| e.to_string())?;
    requests.queue_share(events, link, info)
}

/// Whether `link` is a share link (`caskade://receive?t=…`).
pub fn is_share_link(link: &url::Url) -> bool {
    caskade_share::is_link(link.as_str())
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
