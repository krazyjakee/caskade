//! The `roms` library source: ROM scanners take part in
//! library updates. Scanning and reconciliation live in the core's
//! [`ScannerService`], which the installer's ROM hand-off uses too.
//!
//! On the hook bus it is a [`LibrarySource`] that
//! [imports itself](LibrarySource::imports_itself) (#135): library updates
//! find it in the provider registry and run it after the other sources.
use async_trait::async_trait;
use caskade_core::{
    Result,
    bus::{Bus, Event, LibrarySource},
    emulation::{ScanSummary, ScannerService},
};
use std::sync::Arc;

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

    /// Registers the source on the bus and runs the registry's
    /// self-importing sources after every library scan.
    pub fn watch(&self, bus: &Bus) -> tokio::task::JoinHandle<()> {
        bus.register_library_source(Arc::new(self.clone()));
        update_self_importing(bus)
    }
}

#[async_trait]
impl LibrarySource for RomsSource {
    fn id(&self) -> &str {
        RomsSource::id(self)
    }

    /// Every scanner with `in_library_update` imports; a failing scanner is
    /// logged and the others still run.
    async fn scan(&self) -> std::result::Result<Vec<caskade_discovery::DetectedGame>, String> {
        let summaries = self.update().await.map_err(|error| error.to_string())?;
        for summary in summaries.iter().filter(|s| s.error.is_some()) {
            tracing::warn!(
                scanner = %summary.scanner_id,
                error = summary.error.as_deref().unwrap_or_default(),
                "ROM scanner failed during a library update"
            );
        }
        Ok(Vec::new())
    }

    fn imports_itself(&self) -> bool {
        true
    }
}

/// Runs the self-importing library sources in the bus's registry after
/// every library scan, until the bus is gone.
fn update_self_importing(bus: &Bus) -> tokio::task::JoinHandle<()> {
    let registry = bus.clone();
    let mut events = bus.stream();
    tokio::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match events.recv().await {
                Ok(Event::LibraryScanCompleted { .. }) => {
                    for source in registry.library_sources() {
                        if !source.imports_itself() {
                            continue;
                        }
                        if let Err(error) = source.scan().await {
                            tracing::warn!(source = source.id(), %error, "library source failed to run");
                        }
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    })
}
