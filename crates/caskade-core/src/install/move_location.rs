//! Moving every game off a storage location, then removing it (#64): one
//! job in Activity with one progress bar and one cancel. The location goes
//! only once every game has moved. A cancel or a failed move keeps it, games
//! already moved stay moved, and the job says how far it got.

use super::*;
use crate::storage::{LocationMove, StorageError, StorageService};

/// One game's move, reported as part of the whole job.
struct Overall {
    job: Arc<dyn EventSink>,
    /// Bytes of the games before this one.
    before: u64,
    size: u64,
    total: u64,
    message: String,
}

impl EventSink for Overall {
    fn emit(&self, mut event: JobEvent) {
        let part = match event.total_bytes {
            Some(total) if total > 0 => {
                (u128::from(event.bytes.min(total)) * u128::from(self.size) / u128::from(total))
                    as u64
            }
            _ => 0,
        };
        event.bytes = self.before + part;
        event.total_bytes = Some(self.total);
        event.message = self.message.clone();
        self.job.emit(event);
    }
}

/// `root`'s name for people.
fn location_name(root: &StorageRoot) -> String {
    root.label
        .clone()
        .unwrap_or_else(|| root.path.display().to_string())
}

/// Why the move failed, without the storage error's prefix.
fn reason(error: StorageError) -> String {
    match error {
        StorageError::Invalid(reason) => reason,
        other => other.to_string(),
    }
}

fn moved_so_far(moved: usize, to: &str) -> String {
    crate::trn!(
        "One game moved to {to}.",
        "{n} games moved to {to}.",
        moved,
        to = to
    )
}

impl InstallService {
    /// Queues moving every game on `root_id` to `target_root_id`, then
    /// removing `root_id`. What can't work is refused here, before anything
    /// moves.
    pub async fn move_location(
        &self,
        root_id: String,
        target_root_id: String,
        storage: StorageService,
    ) -> Result<JobSnapshot> {
        let moving = lock(&self.inner.jobs).values().any(|job| {
            job.active
                && job.snapshot.kind == JobKind::MoveLocation
                && job.snapshot.input == root_id
        });
        if moving {
            return Err(Error::Invalid(crate::tr!(
                "these games are already moving; follow them in Activity"
            )));
        }
        let plan = storage
            .plan_location_move(&root_id, &target_root_id)
            .await
            .map_err(|error| Error::Invalid(reason(error)))?;
        for game in &plan.games {
            self.not_sending(&game.entry_id)?;
        }
        Ok(self.queue_operation(
            JobKind::MoveLocation,
            crate::tr!(
                "Games on {location}",
                location = location_name(&plan.source)
            ),
            root_id,
            Some(target_root_id),
            JobPhase::Move,
            move |service, id, cancel| async move {
                service.empty_location(&id, plan, storage, cancel).await
            },
        ))
    }

    /// Job `id`'s run: each game in turn, then the location.
    async fn empty_location(
        &self,
        id: &str,
        plan: LocationMove,
        storage: StorageService,
        cancel: CancellationToken,
    ) -> Result<()> {
        let LocationMove {
            source,
            target,
            games,
        } = plan;
        let (from, to) = (location_name(&source), location_name(&target));
        self.update(id, |job| {
            job.status = JobStatus::Installing;
            job.target_dir = Some(target.path.clone());
        });
        let sink = self.context(id, cancel.clone()).sink;
        let total: u64 = games.iter().map(|game| game.size_bytes).sum();
        let count = games.len();
        let mut before = 0;
        for (index, game) in games.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(self.cancelled(id, index, &from, &to));
            }
            let overall = Arc::new(Overall {
                job: sink.clone(),
                before,
                size: game.size_bytes,
                total,
                message: crate::tr!(
                    "Moving “{title}” ({number} of {count})",
                    title = game.title,
                    number = index + 1,
                    count = count
                ),
            });
            let moved = async {
                self.not_sending(&game.entry_id)?;
                let (_, _, folder) = self.managed_entry(&game.entry_id).await?;
                let moved = storage
                    .move_entry(
                        &game.entry_id,
                        &target.id,
                        &format!("{id}-{index}"),
                        overall,
                        cancel.clone(),
                    )
                    .await
                    .map_err(|error| Error::Invalid(reason(error)))?;
                Ok::<_, Error>((folder, moved))
            }
            .await;
            let (folder, moved) = match moved {
                Ok(moved) => moved,
                Err(_) if cancel.is_cancelled() => {
                    return Err(self.cancelled(id, index, &from, &to));
                }
                Err(error) => {
                    return Err(Error::Invalid(crate::tr!(
                        "{moved} “{title}” wasn't moved: {reason} {location} was kept.",
                        moved = moved_so_far(index, &to),
                        title = game.title,
                        reason = error,
                        location = from
                    )));
                }
            };
            self.inner.bus.emit(Event::GameMoved {
                job_id: id.to_owned(),
                game_id: Some(moved.game_id.clone()),
                dir: Some(moved.directory.clone()),
            });
            let warning = self.announce_move(id, folder, &moved).await;
            self.update(id, |job| job.warnings.extend(warning));
            before += game.size_bytes;
        }
        if cancel.is_cancelled() {
            return Err(self.cancelled(id, count, &from, &to));
        }
        storage.remove_emptied(&source.id).await.map_err(|error| {
            Error::Invalid(crate::tr!(
                "{moved} {location} wasn't removed: {reason}",
                moved = moved_so_far(count, &to),
                location = from,
                reason = reason(error)
            ))
        })?;
        self.update(id, |job| job.status = JobStatus::Completed);
        Ok(())
    }

    /// Records how far a cancelled job got, for its card and for whoever
    /// waits on it, and gives the error that ends its run.
    fn cancelled(&self, id: &str, moved: usize, from: &str, to: &str) -> Error {
        let error = crate::tr!(
            "Cancelled. {moved} {location} was kept.",
            moved = moved_so_far(moved, to),
            location = from
        );
        self.update(id, |job| job.error = Some(error.clone()));
        Error::Invalid(error)
    }
}

#[cfg(test)]
#[path = "move_location_tests.rs"]
mod tests;
