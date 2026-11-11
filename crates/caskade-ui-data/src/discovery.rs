//! Library discovery for Settings → Launchers and Library Discovery (#104):
//! the controller's snapshot (launchers found, the scan's phase, results
//! waiting for review), detection, scans and reviews, through the app
//! layer's commands as the webview's `api/discovery.ts` does. Every change
//! arrives as `DiscoveryChanged` (`Key::Discovery`).

use caskade_app::commands::discovery as commands;
use caskade_app::discovery::{DiscoveryController, DiscoverySnapshot};
use caskade_core::error::ErrorReport;
pub use caskade_discovery::types::{LauncherStatus, SourceId};

type Result<T> = std::result::Result<T, ErrorReport>;

/// The later snapshot. The same revision takes the new one: a language
/// switch changes its text and not its revision.
pub fn newest(current: Option<&DiscoverySnapshot>, next: DiscoverySnapshot) -> DiscoverySnapshot {
    match current {
        Some(current) if current.revision > next.revision => current.clone(),
        _ => next,
    }
}

/// Whether a scan runs (or is being cancelled).
pub fn scanning(snapshot: Option<&DiscoverySnapshot>) -> bool {
    snapshot.is_some_and(|snapshot| {
        matches!(
            snapshot.phase.as_str(),
            "scanning" | "importing" | "cancelling"
        )
    })
}

/// What discovery views read and do, over one app's controller.
#[derive(Clone)]
pub struct Discovery {
    controller: DiscoveryController,
}

impl Discovery {
    pub fn new(controller: DiscoveryController) -> Self {
        Self { controller }
    }

    /// The running app's discovery.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(services.discovery_controller.clone())
    }

    pub async fn status(&self) -> Result<DiscoverySnapshot> {
        commands::discovery_status(&self.controller).await
    }

    /// Looks for the launchers again.
    pub async fn launchers(&self) -> Result<DiscoverySnapshot> {
        commands::discovery_launchers(&self.controller).await
    }

    pub async fn scan(&self) -> Result<DiscoverySnapshot> {
        commands::discovery_scan(&self.controller).await
    }

    pub async fn cancel(&self) -> Result<DiscoverySnapshot> {
        commands::discovery_cancel(&self.controller).await
    }

    /// Accepts or ignores results waiting for review, at the snapshot's
    /// `revision`.
    pub async fn review(
        &self,
        ids: Vec<String>,
        accept: bool,
        revision: u32,
    ) -> Result<DiscoverySnapshot> {
        commands::discovery_review(&self.controller, ids, accept, revision).await
    }
}
