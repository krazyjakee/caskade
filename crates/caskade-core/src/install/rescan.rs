//! The first-exit reference rescan: many games
//! write configs holding their own path on first launch, so a managed
//! game's relocation evidence is refreshed after its first exit. Moves scan
//! again; this keeps the verdict shown before a move honest.

use super::{InstallService, STORAGE_SOURCE};
use crate::{
    Error, Result,
    bus::{Event, GameSession},
    model::InstallLocation,
};
use caskade_installer::{
    events::{EventSink, JobEvent},
    host::HostCaps,
    manifest::InstallManifest,
    pipeline,
    runtime::default_watch_roots,
};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

/// The rescan is not a job; its progress goes nowhere.
struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _: JobEvent) {}
}

impl InstallService {
    /// Rescans managed installs after their first exit, until the bus closes.
    pub fn watch_first_exits(&self) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        let mut events = self.inner.bus.stream();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(Event::GameExited(session)) => {
                        if let Err(error) = service.rescan_after_exit(&session).await {
                            tracing::warn!(
                                %error,
                                entry_id = %session.entry_id,
                                "first-exit reference rescan failed"
                            );
                        }
                    }
                    // A missed rescan is safe: the move job scans again.
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        })
    }

    /// Rescans `session`'s install if this was its first exit and it is a
    /// managed install. Returns whether the manifest was rescanned.
    pub async fn rescan_after_exit(&self, session: &GameSession) -> Result<bool> {
        let store = &self.inner.store;
        let Some(entry) = store.get_entry(&session.entry_id).await? else {
            return Ok(false);
        };
        let Some(InstallLocation::Managed { root_id, rel_dir }) = &entry.location else {
            return Ok(false);
        };
        if entry.source_id != STORAGE_SOURCE {
            return Ok(false);
        }
        let finished = store
            .sessions_for_game(&entry.game_id, u32::MAX)
            .await?
            .into_iter()
            .filter(|played| played.entry_id == entry.id && played.ended_at_ms.is_some())
            .count();
        if finished != 1 {
            return Ok(false);
        }
        let Some(root) = store.get_root(root_id).await? else {
            return Ok(false);
        };

        // Installs and moves into this root wait while the manifest changes.
        let root_lock = self.root_lock(&root.id);
        let _guard = root_lock.lock().await;
        let game_dir = root.path.join(rel_dir);
        let aliases = self.inner.options.aliases.clone();
        caskade_jobs::blocking(crate::tr!("Updating game references"), move || {
            let failed = |error: &dyn std::fmt::Display| Error::Invalid(error.to_string());
            let manifest = InstallManifest::read(&game_dir).map_err(|e| failed(&e))?;
            let old = manifest
                .record()
                .relocation
                .last_path
                .clone()
                .unwrap_or_else(|| game_dir.to_string_lossy().into_owned());
            let external = default_watch_roots()
                .into_values()
                .filter(|path| path.is_dir())
                .collect();
            let reconciler =
                pipeline::relocate::reconciler(HostCaps::detect(), Arc::new(Quiet), aliases)
                    .map_err(|e| failed(&e))?
                    .with_external_roots(external);
            let found = reconciler
                .prepare_references(&game_dir, &old, &manifest)
                .map_err(|e| failed(&e))?;
            let mut record = manifest.into_record();
            record.relocation.verdict = found.verdict;
            record.relocation.references = found.references;
            InstallManifest::new(record)
                .and_then(|manifest| manifest.write(&game_dir))
                .map_err(|e| failed(&e))
        })
        .await
        .map_err(|error| Error::Invalid(error.to_string()))??;
        Ok(true)
    }
}
