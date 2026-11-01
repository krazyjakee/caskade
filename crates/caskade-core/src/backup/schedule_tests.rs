//! Automatic backups on a fake clock, Back up now and retention (#193).

use super::{
    tests::{fill, listing, profile},
    *,
};
use crate::settings::SettingsService;
use std::path::Path;

const DAY: i64 = 24 * 60 * 60 * 1000;
// In the past: the status reads the real clock.
const START: i64 = 1_700_000_000_000;

async fn scheduled(destination: &Path, schedule: Schedule, keep: u32) -> super::tests::Profile {
    let profile = profile().await;
    fill(&profile).await;
    let settings = SettingsService::new(profile.store.clone());
    profile.backups.set_settings(settings.clone());
    settings
        .set(
            NAMESPACE,
            serde_json::to_value(BackupSettings {
                destination: destination.to_string_lossy().into_owned(),
                components: [Component::Plugins].into(),
                schedule,
                keep,
            })
            .unwrap(),
            None,
        )
        .await
        .unwrap();
    profile
}

fn backups_in(dir: &Path) -> Vec<String> {
    listing(dir)
        .into_iter()
        .filter(|name| name.starts_with(PREFIX))
        .collect()
}

#[tokio::test]
async fn automatic_backups_are_opt_in_and_made_once_per_interval() {
    let temp = tempfile::tempdir().unwrap();
    let profile = scheduled(temp.path(), Schedule::Off, 3).await;
    assert!(profile.backups.run_due(START).await.is_none());
    assert_eq!(profile.backups.status().await.next_automatic_ms, None);

    let profile = scheduled(temp.path(), Schedule::Daily, 3).await;
    // Enabled: the first one is due at once.
    let first = profile.backups.run_due(START).await.unwrap().unwrap();
    assert!(first.manifest.set_id.is_some());
    assert!(profile.backups.run_due(START + DAY / 2).await.is_none());
    let status = profile.backups.status().await;
    assert_eq!(status.next_automatic_ms, Some(START + DAY));
    let last = status.last.unwrap();
    assert!(last.automatic && last.error.is_none() && last.path == Some(first.path));
    // Ten days away: one backup makes them up, then a day's wait again.
    profile
        .backups
        .run_due(START + 10 * DAY)
        .await
        .unwrap()
        .unwrap();
    assert!(
        profile
            .backups
            .run_due(START + 10 * DAY + 60_000)
            .await
            .is_none()
    );
    assert_eq!(backups_in(temp.path()).len(), 2);
}

#[tokio::test]
async fn retention_keeps_the_newest_of_its_own_set_and_nothing_else_goes() {
    let temp = tempfile::tempdir().unwrap();
    let profile = scheduled(temp.path(), Schedule::Daily, 2).await;
    // A manual backup, another set's, and files that only look like ones.
    let manual = profile.backups.back_up_now().await.unwrap().path;
    let mut foreign = super::tests::options(&profile, &[]);
    foreign.destination = temp.path().to_owned();
    foreign.set_id = Some("another set".into());
    let foreign = profile
        .backups
        .export(foreign, super::tests::quiet(), CancellationToken::new())
        .await
        .unwrap()
        .path;
    std::fs::write(temp.path().join("caskade-backup-notes.zip"), b"not a zip").unwrap();
    std::fs::write(temp.path().join("holiday.zip"), b"mine").unwrap();

    let mut made = Vec::new();
    for day in 0..4 {
        made.push(
            profile
                .backups
                .run_due(START + day * DAY)
                .await
                .unwrap()
                .unwrap()
                .path,
        );
    }
    let left = listing(temp.path());
    for path in [&manual, &foreign, &made[2], &made[3]] {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(left.contains(&name), "{name} was removed: {left:?}");
    }
    // Made within a second, a newer one may take a removed one's name:
    // count them instead.
    assert_eq!(backups_in(temp.path()).len(), 5, "{left:?}");
    assert!(left.contains("caskade-backup-notes.zip") && left.contains("holiday.zip"));
}

#[tokio::test]
async fn a_failed_backup_retries_and_keeps_the_last_good_ones() {
    let temp = tempfile::tempdir().unwrap();
    let drive = temp.path().join("usb");
    std::fs::create_dir(&drive).unwrap();
    let profile = scheduled(&drive, Schedule::Daily, 1).await;
    let good = profile.backups.run_due(START).await.unwrap().unwrap().path;
    // The drive is unplugged.
    std::fs::rename(&drive, temp.path().join("elsewhere")).unwrap();
    let error = profile
        .backups
        .run_due(START + DAY)
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(error, BackupError::DestinationMissing(_)),
        "{error:?}"
    );
    let status = profile.backups.status().await;
    let last = status.last.unwrap();
    assert_eq!(
        last.error.unwrap().code,
        crate::error::ErrorCode::StorageOffline
    );
    // Tried again in half an hour, not before.
    assert_eq!(status.next_automatic_ms, Some(START + DAY + DAY / 48));
    assert!(
        profile
            .backups
            .run_due(START + DAY + 60_000)
            .await
            .is_none()
    );
    // Back again: the next try works, and only then does the old one go.
    std::fs::rename(temp.path().join("elsewhere"), &drive).unwrap();
    assert!(good.exists());
    let newer = profile
        .backups
        .run_due(START + DAY + DAY / 48)
        .await
        .unwrap()
        .unwrap()
        .path;
    assert!(newer.exists() && !good.exists());
}

#[tokio::test]
async fn a_manual_backup_and_a_due_one_never_overlap() {
    let temp = tempfile::tempdir().unwrap();
    let profile = scheduled(temp.path(), Schedule::Daily, 3).await;
    let held = profile.backups.running.try_lock().unwrap();
    // Due, but a backup is running: not counted as a try, looked at later.
    assert!(profile.backups.run_due(START).await.is_none());
    assert!(matches!(
        profile.backups.back_up_now().await,
        Err(BackupError::Busy)
    ));
    drop(held);
    let next = profile.backups.status().await.next_automatic_ms.unwrap();
    assert!(next <= crate::now_ms());
    assert!(profile.backups.run_due(START).await.unwrap().is_ok());
    let manual = profile.backups.back_up_now().await.unwrap();
    assert_eq!(manual.manifest.set_id, None);
    let last = profile.backups.status().await.last.unwrap();
    assert!(!last.automatic);
}

#[tokio::test]
async fn status_follows_a_job_and_cancel_stops_it() {
    let temp = tempfile::tempdir().unwrap();
    let profile = scheduled(temp.path(), Schedule::Off, 3).await;
    let mut changes = profile.backups.subscribe();
    let backups = profile.backups.clone();
    let watcher = tokio::spawn(async move {
        let mut seen = Vec::new();
        while changes.changed().await.is_ok() {
            let status = changes.borrow_and_update().clone().unwrap();
            if let Some(job) = &status.running {
                seen.push(job.kind);
                backups.cancel();
            }
            if status.last.is_some() {
                break;
            }
        }
        seen
    });
    let error = profile.backups.back_up_now().await.unwrap_err();
    assert!(matches!(error, BackupError::Cancelled), "{error:?}");
    let seen = watcher.await.unwrap();
    assert!(seen.contains(&BackupJobKind::Backup), "{seen:?}");
    let status = profile.backups.status().await;
    assert!(status.running.is_none());
    assert_eq!(
        status.last.unwrap().error.unwrap().code,
        crate::error::ErrorCode::Cancelled
    );
    assert!(backups_in(temp.path()).is_empty());
}
