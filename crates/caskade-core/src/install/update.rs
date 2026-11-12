//! Updating a managed install to a newer release (`crate::updates`).
//!
//! The update reinstalls the game in its own place. The release is
//! downloaded first; then, with the root locked, the old folder is set
//! aside in the root's `.caskade/updates/<job>`, the release installs under
//! the old folder's name and the install's identity (so the alias, library
//! entry, playtime and artwork stay), and the user's files come across
//! from the old folder (`carry`) before it is deleted. If the install
//! fails, is cancelled or needs an answer, the old folder goes back.
//!
//! A folder still set aside at startup (the app stopped mid-update) is
//! finished or put back by [`InstallService::recover_updates`]. A new
//! version that is missing its launch target is removed by the pipeline,
//! so the old folder goes back as for any failure (#158).

use super::{carry, *};
use crate::{
    launch::LaunchService,
    model::{GameEntry, InstallLocation},
};
use caskade_discovery::InstallState;
use std::path::{Path, PathBuf};

/// What an update job updates, kept for its restarts.
#[derive(Clone)]
pub(super) struct UpdateTarget {
    pub(super) entry_id: String,
    /// The release being installed.
    pub(super) version: String,
    pub(super) launch: LaunchService,
}

/// Where updates set the old version aside, inside the root.
const SET_ASIDE: &str = ".caskade/updates";

impl InstallService {
    /// Follows a finished install for updates; a failure only costs the
    /// install its update checks.
    pub(super) async fn track(
        &self,
        entry_id: &str,
        manifest: &InstallManifest,
        download: Option<&url::Url>,
    ) {
        let tracker = lock(&self.inner.tracker).clone();
        if let Some(tracker) = tracker
            && let Err(error) = tracker.track(entry_id, manifest, download).await
        {
            tracing::warn!(%error, entry_id, "install not followed for updates");
        }
    }

    /// Queues an update of managed entry `entry_id` to release `version`,
    /// installed from `input`. The game must not be running, and is kept
    /// from starting until the update is done.
    pub async fn update_game(
        &self,
        entry_id: String,
        input: String,
        version: String,
        launch: LaunchService,
    ) -> Result<JobSnapshot> {
        let (entry, root, _) = self.managed_entry(&entry_id).await?;
        // Fails now if the game is running; the run takes the lease again.
        drop(
            launch
                .try_game_lease(&entry.game_id)
                .map_err(|e| Error::Invalid(e.to_string()))?,
        );
        let title = self
            .inner
            .store
            .game(&entry.game_id)
            .await?
            .map_or_else(|| title_of(&input), |game| game.title);
        let snapshot = JobSnapshot {
            game_id: Some(entry.game_id.clone()),
            ..JobSnapshot::queued(
                JobKind::Update,
                format!("{title} {version}"),
                input.clone(),
                Some(root.id.clone()),
            )
        };
        let id = snapshot.id.clone();
        let target = UpdateTarget {
            entry_id,
            version,
            launch,
        };
        let cancel = self.add_job(snapshot.clone(), Some(target), DiscoveryHints::default());
        self.spawn_update(id, input, root, cancel);
        Ok(snapshot)
    }

    pub(super) fn spawn_update(
        &self,
        id: String,
        input: String,
        root: StorageRoot,
        cancel: CancellationToken,
    ) {
        let service = self.clone();
        tokio::spawn(async move {
            let _activity = caskade_jobs::global().activity(crate::tr!("Updating game"));
            let result = service.run_update(&id, input, root, cancel.clone()).await;
            service.end_operation(&id, result, cancel.is_cancelled());
        });
    }

    async fn run_update(
        &self,
        id: &str,
        input: String,
        root: StorageRoot,
        cancel: CancellationToken,
    ) -> Result<()> {
        let target = lock(&self.inner.jobs)
            .get(id)
            .and_then(|job| job.update.clone())
            .ok_or_else(|| Error::Invalid(crate::tr!("unknown update job")))?;
        let (entry, _, game_dir) = self.managed_entry(&target.entry_id).await?;
        let _lease = target
            .launch
            .try_game_lease(&entry.game_id)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let old = read_manifest(&game_dir).await?;
        if old.record().ids.get(pipeline::INSTALL_ID) != Some(&entry.external_id) {
            return Err(Error::Invalid(crate::tr!(
                "installation identity changed before the update"
            )));
        }
        let rel_dir = match &entry.location {
            Some(InstallLocation::Managed { rel_dir, .. }) => rel_dir.clone(),
            _ => {
                return Err(Error::Invalid(crate::tr!(
                    "the game isn't a managed install"
                )));
            }
        };

        let mut ctx = self.context(id, cancel.clone());
        // The same folder and identity as the version it replaces.
        ctx.folder_template = Some(rel_dir.to_string_lossy().into_owned());
        ctx.install_id = Some(entry.external_id.clone());
        let Some(ready) = self
            .prepare_run(id, &input, &root, &mut ctx, &cancel)
            .await?
        else {
            return Ok(());
        };
        let download = ready.prepared.download_url.clone();
        let source = ready.prepared.path.clone();
        let remote = pipeline::input::downloads(&input);

        let root_lock = self.root_lock(&root.id);
        let _guard = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Invalid(crate::tr!("cancelled"))),
            guard = root_lock.lock() => guard,
        };
        if self
            .inner
            .store
            .in_flight_sessions()
            .await?
            .iter()
            .any(|s| s.game_id == entry.game_id)
        {
            return Err(Error::Invalid(crate::tr!("game has an open play session")));
        }
        self.update(id, |job| {
            job.status = JobStatus::Installing;
            job.phase = Some(JobPhase::Install);
            // The download's last report isn't the install's.
            job.progress = None;
        });
        let aside = set_aside_dir(&root.path, id)?;
        self.mark(&entry, InstallState::Updating).await?;
        if let Err(error) = move_dir(game_dir.clone(), aside.clone()).await {
            self.mark(&entry, InstallState::Installed).await?;
            return Err(error);
        }
        let outcome = pipeline::install_prepared(
            ready.prepared,
            root.path.clone(),
            ready.job_id,
            ready.host,
            ready.preferences.keep_downloads,
            false,
            &ctx,
        )
        .await;
        self.sync_plans(id);
        let installed = match outcome {
            Ok(InstallOutcome::Installed(installed)) => installed,
            other => {
                move_dir(aside, game_dir).await?;
                self.mark(&entry, InstallState::Installed).await?;
                return match other {
                    Ok(InstallOutcome::NeedsUser(question)) => {
                        self.update(id, |job| {
                            job.status = JobStatus::NeedsUser;
                            job.question = Some(question);
                        });
                        Ok(())
                    }
                    Ok(_) => Err(Error::Invalid(crate::tr!(
                        "the new release doesn't install like the version it replaces"
                    ))),
                    Err(error) => Err(self.failed(
                        id,
                        &*error,
                        caskade_installer::failure::Probe {
                            stage: self.running_stage(id),
                            source: Some(&source),
                            remote,
                            root: Some(&root.path),
                        },
                    )),
                };
            }
        };

        let new_dir = root.path.join(&installed.dir);
        let manifest = read_manifest(&new_dir).await?;
        let from = old_version(&self.inner.store, &entry.id).await;
        let mut warnings = installed.warnings.clone();
        warnings
            .extend(carry_and_discard(aside, old, new_dir.clone(), manifest.clone(), from).await);
        let (_, mut updated) = from_manifest(&root.id, &installed.dir, &manifest)?;
        updated.id = entry.id.clone();
        updated.game_id = entry.game_id.clone();
        updated.tracking = updated.tracking.or(entry.tracking.clone());
        self.inner.store.upsert_entry(&updated).await?;
        if let (Some(old_alias), Some(aliases)) = (&entry.alias_id, &self.inner.options.aliases)
            && updated.alias_id.as_ref() != Some(old_alias)
        {
            let _ = aliases.remove(old_alias);
        }
        self.track(&entry.id, &manifest, download.as_ref()).await;
        let asset = download
            .as_ref()
            .and_then(|url| url.path_segments()?.next_back().map(str::to_owned));
        if let Err(error) = self
            .inner
            .store
            .updated_to(&entry.id, &target.version, asset.as_deref())
            .await
        {
            tracing::warn!(%error, entry_id = %entry.id, "updated release not recorded");
        }
        self.inner.bus.emit(Event::LibraryGameUpdated {
            game_id: entry.game_id.clone(),
        });
        self.update(id, |job| {
            job.status = JobStatus::Completed;
            job.warnings = warnings;
            job.target_dir = Some(new_dir);
        });
        Ok(())
    }

    /// Saves `entry` in `state` and tells the library.
    async fn mark(&self, entry: &GameEntry, state: InstallState) -> Result<()> {
        let mut entry = entry.clone();
        entry.state = state;
        self.inner.store.upsert_entry(&entry).await?;
        self.inner.bus.emit(Event::LibraryGameUpdated {
            game_id: entry.game_id,
        });
        Ok(())
    }

    /// Finishes or undoes updates the app stopped in the middle of: a
    /// folder still set aside goes back if nothing replaced it, and has the
    /// user's files carried over and is deleted if the new version is
    /// installed. Roots that are offline wait for the next start.
    pub async fn recover_updates(&self) {
        // First what interrupted installs left, so a half-installed new
        // version is gone before its old folder comes back.
        self.recover_interrupted_installs().await;
        let roots = match self.inner.store.roots().await {
            Ok(roots) => roots,
            Err(error) => {
                tracing::warn!(%error, "interrupted updates not checked");
                return;
            }
        };
        for root in roots {
            let Ok(entries) = std::fs::read_dir(root.path.join(SET_ASIDE)) else {
                continue;
            };
            for aside in entries.flatten().map(|entry| entry.path()) {
                if let Err(error) = self.recover_update(&root, aside.clone()).await {
                    tracing::warn!(%error, path = %aside.display(), "interrupted update left as it is");
                }
            }
        }
    }

    async fn recover_update(&self, root: &StorageRoot, aside: PathBuf) -> Result<()> {
        let old = read_manifest(&aside).await?;
        let install_id = old
            .record()
            .ids
            .get(pipeline::INSTALL_ID)
            .ok_or_else(|| Error::Invalid(crate::tr!("set-aside install has no identity")))?;
        let entry = self
            .inner
            .store
            .entry_by_source(STORAGE_SOURCE.as_str(), install_id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("set-aside install has no library entry")))?;
        let Some(InstallLocation::Managed { root_id, rel_dir }) = &entry.location else {
            return Err(Error::Invalid(crate::tr!(
                "set-aside install isn't managed"
            )));
        };
        if *root_id != root.id {
            return Err(Error::Invalid(crate::tr!(
                "set-aside install moved to another root"
            )));
        }
        let game_dir = root.path.join(rel_dir);
        if !game_dir.exists() {
            move_dir(aside, game_dir).await?;
            tracing::info!(entry_id = %entry.id, "interrupted update undone");
        } else {
            let manifest = read_manifest(&game_dir).await?;
            if manifest.record().ids.get(pipeline::INSTALL_ID) != Some(install_id) {
                return Err(Error::Invalid(crate::tr!(
                    "another install took the game's folder"
                )));
            }
            let from = old_version(&self.inner.store, &entry.id).await;
            for warning in carry_and_discard(aside, old, game_dir, manifest, from).await {
                tracing::warn!(message = %warning.message, "interrupted update");
            }
            tracing::info!(entry_id = %entry.id, "interrupted update finished");
        }
        if entry.state == InstallState::Updating {
            self.mark(&entry, InstallState::Installed).await?;
        }
        Ok(())
    }
}

/// A new folder for the version job `id` sets aside in `root`.
fn set_aside_dir(root: &Path, id: &str) -> Result<PathBuf> {
    let parent = std::fs::canonicalize(root)?.join(SET_ASIDE);
    std::fs::create_dir_all(&parent)?;
    Ok(parent.join(id))
}

async fn move_dir(from: PathBuf, to: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || std::fs::rename(from, to))
        .await
        .map_err(|e| Error::Invalid(e.to_string()))??;
    Ok(())
}

async fn read_manifest(dir: &Path) -> Result<InstallManifest> {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || InstallManifest::read(&dir))
        .await
        .map_err(|e| Error::Invalid(e.to_string()))?
        .map_err(|e| Error::Invalid(e.to_string()))
}

/// The release an entry had before its update, to name the backup folder.
async fn old_version(store: &LibraryStore, entry_id: &str) -> String {
    store
        .game_update(entry_id)
        .await
        .ok()
        .flatten()
        .and_then(|row| row.installed)
        .unwrap_or_else(|| "previous".into())
}

/// Carries the user's files from the set-aside folder into the new one,
/// then deletes it. When that fails, the folder stays and a warning says
/// where.
async fn carry_and_discard(
    aside: PathBuf,
    old: InstallManifest,
    new_dir: PathBuf,
    new: InstallManifest,
    from: String,
) -> Vec<caskade_installer::Warning> {
    let kept = aside.clone();
    let result = tokio::task::spawn_blocking(move || {
        let carried = carry::carry_over(&aside, &old, &new_dir, &new, &from)?;
        std::fs::remove_dir_all(&aside)?;
        Ok::<_, std::io::Error>(carried)
    })
    .await
    .map_err(std::io::Error::other)
    .and_then(|result| result);
    match result {
        Ok(carried) if carried.backed_up.is_empty() => vec![],
        Ok(carried) => vec![caskade_installer::Warning {
            code: "update_backed_up".into(),
            message: crate::trn!(
                "The new version replaced {n} file you had changed; your copy is in {folder}.",
                "The new version replaced {n} files you had changed; your copies are in {folder}.",
                carried.backed_up.len(),
                folder = carry::BACKUP_DIR
            ),
            path: None,
        }],
        Err(error) => vec![caskade_installer::Warning {
            code: "update_files_kept".into(),
            message: crate::tr!(
                "Updated, but not every file of the previous version came across ({error}); it is kept in full.",
                error = error
            ),
            path: Some(kept),
        }],
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "update_tests.rs"]
mod tests;
