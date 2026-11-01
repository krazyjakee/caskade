//! First-run setup (`core.onboarding`). Until it is finished, the library's
//! first scan and the online lookups wait, so the choices made in it count.
//! A library that already has games counts as set up.

use caskade_core::{
    db::LibraryStore,
    model::ListGames,
    settings::{MetadataSettings, OnboardingSettings, SettingsService},
};
use tokio::{sync::broadcast::error::RecvError, task::JoinHandle};

const ONBOARDING: &str = "core.onboarding";
const METADATA: &str = "core.metadata";

/// Setup has been finished or skipped.
pub async fn completed(settings: &SettingsService) -> bool {
    settings
        .read_or_default::<OnboardingSettings>(ONBOARDING)
        .await
        .completed
}

/// Background lookups may go online: setup is done and allowed them.
async fn online(settings: &SettingsService) -> bool {
    completed(settings).await
        && settings
            .read_or_default::<MetadataSettings>(METADATA)
            .await
            .online
}

/// Marks setup done for a library from before it existed: one with games
/// whose setup section was never saved.
pub async fn settle_existing(
    settings: &SettingsService,
    store: &LibraryStore,
) -> Result<(), String> {
    let snapshot = settings
        .get(ONBOARDING)
        .await
        .map_err(|error| error.to_string())?;
    if snapshot.revision > 0 {
        return Ok(());
    }
    let games = store
        .list_games(&ListGames {
            limit: 1,
            ..ListGames::default()
        })
        .await
        .map_err(|error| error.to_string())?
        .total;
    if games == 0 {
        return Ok(());
    }
    let value = OnboardingSettings {
        completed: true,
        ..serde_json::from_value(snapshot.value).unwrap_or_default()
    };
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    settings
        .set(ONBOARDING, value, Some(snapshot.revision))
        .await
        .map_err(|error| error.to_string())?;
    tracing::info!(games, "existing library: first-run setup skipped");
    Ok(())
}

/// Keeps the tasks `start` spawns running while background lookups may go
/// online, and stops them when that is turned off. When one panics, all of
/// them start again, after a pause.
pub fn supervise_online(
    settings: SettingsService,
    start: impl Fn() -> Vec<JoinHandle<()>> + Send + 'static,
) {
    supervise_online_every(settings, start, crate::background::RESTART_DELAY);
}

/// [`supervise_online`], looking for panicked tasks every `check`.
fn supervise_online_every(
    settings: SettingsService,
    start: impl Fn() -> Vec<JoinHandle<()>> + Send + 'static,
    check: std::time::Duration,
) {
    tauri::async_runtime::spawn(async move {
        let mut changes = settings.subscribe();
        let mut running: Vec<JoinHandle<()>> = Vec::new();
        loop {
            let wanted = online(&settings).await;
            if wanted && running.is_empty() {
                running = start();
                tracing::info!("online lookups started");
            } else if !wanted && !running.is_empty() {
                running.drain(..).for_each(|task| task.abort());
                tracing::info!("online lookups stopped");
            }
            loop {
                tokio::select! {
                    change = changes.recv() => match change {
                        Ok(change)
                            if [ONBOARDING, METADATA].contains(&change.namespace.as_str()) =>
                        {
                            break;
                        }
                        Ok(_) => {}
                        Err(RecvError::Lagged(_)) => break,
                        Err(RecvError::Closed) => return,
                    },
                    () = tokio::time::sleep(check), if !running.is_empty() => {
                        if reap(&mut running).await {
                            break;
                        }
                    }
                }
            }
        }
    });
}

/// Takes finished tasks out of `running`. When one of them panicked, stops
/// the rest too and returns true, so that they all start again.
async fn reap(running: &mut Vec<JoinHandle<()>>) -> bool {
    let (finished, alive): (Vec<_>, Vec<_>) = running.drain(..).partition(JoinHandle::is_finished);
    *running = alive;
    let mut panicked = false;
    for task in finished {
        if let Err(error) = task.await
            && error.is_panic()
        {
            let panic = error.into_panic();
            tracing::error!(
                panic = crate::background::panic_message(&*panic),
                "an online lookup panicked; restarting them"
            );
            panicked = true;
        }
    }
    if panicked {
        running.drain(..).for_each(|task| task.abort());
    }
    panicked
}

/// Images a game page downloads when it opens may come from IGDB.
pub async fn page_images_online(settings: &SettingsService) -> bool {
    settings
        .read_or_default::<MetadataSettings>(METADATA)
        .await
        .online
}

#[cfg(test)]
#[path = "onboarding_tests.rs"]
mod tests;
