//! The `roms` library source: ROM scanners take part in
//! library updates (#135). Scanning and reconciliation live in the core's
//! [`ScannerService`], which the installer's ROM hand-off uses too.
use caskade_core::{
    Result,
    bus::{Bus, Event},
    emulation::{ScanSummary, ScannerService},
};

#[derive(Clone)]
pub struct RomsSource {
    scanners: ScannerService,
}

impl RomsSource {
    pub fn new(scanners: ScannerService) -> Self {
        Self { scanners }
    }

    pub fn id(&self) -> &'static str {
        "roms"
    }

    /// A library update: every scanner with `in_library_update` imports.
    pub async fn update(&self) -> Result<Vec<ScanSummary>> {
        self.scanners.scan_all().await
    }

    /// Runs [`Self::update`] after every library scan, until the bus is
    /// gone. A failing scanner is logged and the others still run.
    pub fn watch(&self, bus: &Bus) -> tokio::task::JoinHandle<()> {
        let source = self.clone();
        let mut events = bus.stream();
        tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match events.recv().await {
                    Ok(Event::LibraryScanCompleted { .. }) => source.log_update().await,
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        })
    }

    async fn log_update(&self) {
        match self.update().await {
            Ok(summaries) => {
                for summary in summaries.iter().filter(|s| s.error.is_some()) {
                    tracing::warn!(
                        scanner = %summary.scanner_id,
                        error = summary.error.as_deref().unwrap_or_default(),
                        "ROM scanner failed during a library update"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(source = self.id(), %error, "library source failed to run");
            }
        }
    }
}
