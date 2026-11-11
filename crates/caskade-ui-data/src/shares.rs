//! One-off shares for flows (#247, #108): making a link to a game,
//! stopping it, what a link shares, and receiving it, through the app
//! layer's commands as the webview's `api/shares.ts` does.

use caskade_app::commands::share_items;
use caskade_app::commands::shares::{self, ShareCreated};
use caskade_core::{
    accounts::AccountService, error::ErrorReport, install::JobSnapshot, model::Collection,
};
use caskade_share::Shares as Service;

/// The share types the flows show, so the UI needn't depend on the share
/// crate itself.
pub use caskade_share::{
    GameHere, ReceivedShare, ShareContents, ShareExpiry, ShareInfo, ShareItem, ShareReach,
    ShareRecipient, ShareRecord, ShareState, SharedGame,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What share flows read and change, over one app's share service.
#[derive(Clone)]
pub struct Shares {
    shares: Service,
    accounts: AccountService,
}

impl Shares {
    pub fn new(shares: Service, accounts: AccountService) -> Self {
        Self { shares, accounts }
    }

    /// The running app's shares; `None` where it has none.
    pub fn of(services: &caskade_app::start::Services) -> Option<Self> {
        Some(Self::new(
            services.shares.clone()?,
            services.accounts.clone(),
        ))
    }

    /// Shares something smaller than a game (#248): a save version, a web
    /// game's offline copy, a ROM, or one of the account's collections.
    pub async fn share_item(&self, item: ShareItem, expiry: ShareExpiry) -> Result<ShareCreated> {
        share_items::share_item(&self.shares, &self.accounts, item, expiry).await
    }

    /// Adds the collection `link` shares to the account's collections:
    /// nothing downloads.
    pub async fn receive_collection(&self, link: &str) -> Result<Collection> {
        share_items::share_receive_collection(&self.shares, &self.accounts, link).await
    }

    /// Shares received here that arrive or help their other recipients.
    pub fn received(&self) -> Vec<ReceivedShare> {
        shares::shares_received(&self.shares)
    }

    /// Whether the share received as `job` goes on helping once it's here.
    pub fn keep_helping(&self, job: &str, on: bool) -> Result<()> {
        shares::share_keep_helping(&self.shares, job, on)
    }

    /// A running share's link again, to copy.
    pub async fn link(&self, id: &str) -> Result<String> {
        shares::share_link(&self.shares, id).await
    }

    /// Makes a link to installation `entry_id`, working until `expiry`.
    pub async fn share(&self, entry_id: &str, expiry: ShareExpiry) -> Result<ShareCreated> {
        shares::share_game(&self.shares, entry_id, expiry).await
    }

    /// Every share this device made, the newest first.
    pub async fn list(&self) -> Result<Vec<ShareRecord>> {
        shares::shares_list(&self.shares).await
    }

    /// A running share's link as a QR code (an SVG document).
    pub async fn qr(&self, id: &str) -> Result<String> {
        shares::share_qr(&self.shares, id).await
    }

    /// Stops share `id`: its link fails at once.
    pub async fn stop(&self, id: &str) -> Result<()> {
        shares::share_stop(&self.shares, id).await
    }

    /// Forgets a stopped or expired share.
    pub async fn forget(&self, id: &str) -> Result<()> {
        shares::share_forget(&self.shares, id).await
    }

    /// What `link` shares, asked of its sharer; nothing is fetched.
    pub async fn inspect(&self, link: &str) -> Result<ShareInfo> {
        shares::share_inspect(&self.shares, link).await
    }

    /// Receives `link` into location `root_id` (the default without one),
    /// as a job in Activity; with `keep_helping`, it goes on helping the
    /// share's other recipients once it's here.
    pub async fn receive(
        &self,
        link: &str,
        root_id: Option<String>,
        keep_helping: bool,
    ) -> Result<JobSnapshot> {
        let job = shares::share_receive(&self.shares, link, root_id).await?;
        if keep_helping {
            shares::share_keep_helping(&self.shares, &job.id, true)?;
        }
        Ok(job)
    }

    /// Hears every change to the shares or what's received.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.shares.subscribe()
    }
}

impl PartialEq for Shares {
    /// One share service per app: a flow's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
