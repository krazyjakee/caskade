//! Settings → Backup's commands, called as a shell calls them.
use super::*;
use caskade_backup::{Exclusion, RestoreState};
use caskade_core::{AppPaths, db::LibraryStore, model::Game, settings::SettingsService};

struct Fixture {
    _temp: tempfile::TempDir,
    paths: AppPaths,
    backups: Backups,
}

async fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    paths.create().unwrap();
    let store = LibraryStore::open_default(&paths).await.unwrap();
    store.upsert_game(&Game::new("Kept safe")).await.unwrap();
    let backups = Backups::new(store.clone(), paths.clone());
    backups.set_settings(SettingsService::new(store));
    Fixture {
        _temp: temp,
        paths,
        backups,
    }
}

/// The status once no job runs and a backup has an outcome.
async fn settled(backups: &Backups) -> BackupStatus {
    for _ in 0..500 {
        let status = backup_status(backups).await.unwrap();
        if status.running.is_none() && status.last.is_some() {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the backup didn't finish");
}

#[tokio::test(flavor = "multi_thread")]
async fn back_up_now_then_check_and_stage_a_restore() {
    let Fixture {
        paths,
        backups,
        _temp,
    } = fixture().await;
    let status = backup_status(&backups).await.unwrap();
    assert!(status.running.is_none() && status.last.is_none());
    assert_eq!(status.default_destination, paths.data.join("backups"));
    let plan = backup_plan(&backups, None, vec![Component::Themes])
        .await
        .unwrap();
    assert_eq!(plan.components[0].id, Component::Library);
    assert!(plan.excluded.contains(&Exclusion::Credentials));

    backup_start(&backups).await.unwrap();
    let last = settled(&backups).await.last.unwrap();
    assert!(last.error.is_none(), "{last:?}");
    let archive = last.path.unwrap();
    assert!(archive.is_file());
    let archive = archive.to_str().unwrap().to_owned();

    let inspection = backup_inspect(&backups, archive.clone()).await.unwrap();
    assert!(!inspection.manifest.tables.is_empty());
    assert!(!inspection.needs_migration);
    let journal = backup_restore(&backups, archive, vec![]).await.unwrap();
    assert_eq!(journal.state, RestoreState::Staged);
    let status = backup_status(&backups).await.unwrap();
    assert_eq!(
        status.staged_restore.map(|journal| journal.state),
        Some(RestoreState::Staged)
    );
    backup_restore_discard(&backups).await.unwrap();
    let status = backup_status(&backups).await.unwrap();
    assert!(status.staged_restore.is_none());
    // Nothing to apply: no restart.
    let refused = backup_restart_ready(&backups, None, None)
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Invalid);
}

#[tokio::test]
async fn only_backups_in_the_backup_folder_are_shown() {
    let Fixture {
        paths,
        backups,
        _temp,
    } = fixture().await;
    for path in [
        paths.data.join("library.sqlite"),
        paths.data.join("backups/notes.txt"),
        paths.config.join("caskade-backup-20260101-000000.zip"),
    ] {
        let refused = backup_reveal(&backups, path.to_str().unwrap().to_owned())
            .await
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Invalid, "{}", path.display());
    }
}

#[tokio::test]
async fn a_damaged_backup_is_refused_with_its_reason() {
    let Fixture {
        paths,
        backups,
        _temp,
    } = fixture().await;
    let damaged = paths.data.join("damaged.zip");
    std::fs::write(&damaged, b"not a backup").unwrap();
    let refused = backup_inspect(&backups, damaged.to_str().unwrap().to_owned())
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Corrupt);
}
