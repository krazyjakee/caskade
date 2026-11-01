//! Removing a game Caskade added from the library for good. Hiding is for
//! games a scan found, since the next scan would bring them back; nothing
//! brings back an install or a hand-off, so those can go.

use super::{HANDOFF_SOURCE, InstallService, JobKind, JobPhase, JobSnapshot, JobStatus};
use crate::{
    Error, Result,
    bus::Event,
    launch::LaunchService,
    model::{InstallLocation, MANUAL_SOURCE},
};

impl InstallService {
    /// Removes `game_id` from the library when every entry it has came from
    /// Caskade: installs into a storage root, whose files are uninstalled
    /// first, and hand-offs and games added by hand (#174), whose files stay
    /// where they are. A game a scan
    /// found is refused, and so is a web game (`WebGames::remove`). Returns
    /// the uninstall job when files have to go, which removes the game once
    /// it completes; otherwise the game is gone on return. A failed
    /// uninstall leaves the game as it was.
    pub async fn remove_game(
        &self,
        game_id: &str,
        launch: LaunchService,
    ) -> Result<Option<JobSnapshot>> {
        let store = &self.inner.store;
        if store.game(game_id).await?.is_none() {
            return Err(crate::db::missing("game", game_id));
        }
        let entries = store.entries_for_game(game_id).await?;
        if entries
            .iter()
            .any(|e| e.source_id == crate::web::WEB_SOURCE)
        {
            return Err(Error::Invalid(crate::tr!("remove its web version first")));
        }
        if entries.iter().any(|e| {
            e.source_id != super::STORAGE_SOURCE
                && e.source_id != HANDOFF_SOURCE
                && e.source_id.as_str() != MANUAL_SOURCE
        }) {
            return Err(Error::Invalid(crate::tr!(
                "a scan found this game and would bring it back; hide it instead"
            )));
        }
        let lease = launch
            .try_game_lease(game_id)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if store
            .in_flight_sessions()
            .await?
            .iter()
            .any(|s| s.game_id == game_id)
        {
            return Err(Error::Invalid(crate::tr!("game has an open play session")));
        }
        let mut installed = vec![];
        for entry in &entries {
            if entry.source_id != super::STORAGE_SOURCE
                || !matches!(entry.location, Some(InstallLocation::Managed { .. }))
            {
                continue;
            }
            match self.managed_entry(&entry.id).await {
                Ok(found) => installed.push(found),
                // Its folder was deleted outside Caskade: only the entry is left.
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        let Some((first, root, _)) = installed.first() else {
            self.forget_game(game_id).await?;
            drop(lease);
            return Ok(None);
        };
        let game_id = game_id.to_owned();
        Ok(Some(self.queue_operation(
            JobKind::Uninstall,
            first.external_id.clone(),
            first.id.clone(),
            Some(root.id.clone()),
            JobPhase::Install,
            move |service, id, cancel| async move {
                let _lease = lease;
                for (entry, root, dir) in installed {
                    service
                        .uninstall_files(&id, &cancel, &entry, &root, dir)
                        .await?;
                }
                service.forget_game(&game_id).await?;
                service.update(&id, |job| {
                    job.status = JobStatus::Completed;
                    job.game_id = Some(game_id);
                });
                Ok(())
            },
        )))
    }

    /// Deletes the game, and with it the entries it has left.
    async fn forget_game(&self, game_id: &str) -> Result<()> {
        self.inner.store.delete_game(game_id).await?;
        self.inner.bus.emit(Event::LibraryGameRemoved {
            game_id: game_id.to_owned(),
        });
        tracing::info!(game = game_id, "game removed from the library");
        Ok(())
    }
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod tests;
