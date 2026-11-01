//! Restores at startup (#192): a restore staged in Settings, or with
//! `caskade --restore <backup>`, is applied before the library opens, and
//! undone if the restored library won't open. When the library can't open
//! at all, its error dialog offers Restore from backup….

use caskade_core::{AppPaths, backup::restore, db::LibraryStore};
use std::path::{Path, PathBuf};

/// `--restore <backup>` from the command line.
pub fn restore_arg(args: &[String], cwd: &Path) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == "--restore")?;
    args.get(at + 1).map(|path| cwd.join(path))
}

/// Stages `archive` for [`open_library`] to apply; the profile is untouched
/// until then.
pub async fn stage(paths: &AppPaths, archive: &Path) -> Result<(), String> {
    let options = caskade_core::backup::RestoreOptions {
        archive: archive.to_owned(),
        // The library, and whatever else the backup holds.
        components: caskade_core::backup::Component::ALL.into_iter().collect(),
    };
    restore::stage(
        paths,
        &options,
        &caskade_core::backup::CancellationToken::new(),
        &(std::sync::Arc::new(|_| {}) as caskade_core::backup::ProgressFn),
    )
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// Opens the library, applying a staged restore first. A restored library
/// that won't open is rolled back, and the one from before is opened.
pub async fn open_library(paths: &AppPaths) -> caskade_core::Result<LibraryStore> {
    let applied = match restore::apply_pending(paths) {
        Ok(applied) => applied.is_some(),
        Err(error) => {
            tracing::error!(%error, "the restore couldn't be applied");
            undo(paths, &error.to_string());
            false
        }
    };
    match LibraryStore::open_default(paths).await {
        Ok(store) => {
            if applied && let Err(error) = restore::finish(paths) {
                tracing::warn!(%error, "the restore's previous profile wasn't removed");
            }
            Ok(store)
        }
        Err(error) if applied => {
            tracing::error!(%error, "the restored library doesn't open");
            undo(paths, &error.to_string());
            LibraryStore::open_default(paths).await
        }
        Err(error) => Err(error),
    }
}

fn undo(paths: &AppPaths, why: &str) {
    if let Err(error) = restore::roll_back(paths, why) {
        tracing::error!(%error, "the restore couldn't be undone");
    }
}

/// From the library error dialog: asks for a backup, stages it and
/// restarts into it. Quits if none is chosen or it can't be staged.
pub fn restore_from_dialog<R: tauri::Runtime>(app: &tauri::AppHandle<R>, paths: AppPaths) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let handle = app.clone();
    app.dialog()
        .file()
        .set_title(caskade_core::tr!("Choose a Caskade backup"))
        .add_filter(caskade_core::tr!("Caskade backups"), &["zip"])
        .pick_file(move |file| {
            let Some(archive) = file.and_then(|file| file.into_path().ok()) else {
                handle.exit(1);
                return;
            };
            tauri::async_runtime::spawn(async move {
                match stage(&paths, &archive).await {
                    Ok(()) => handle.restart(),
                    Err(error) => {
                        let quit = handle.clone();
                        handle
                            .dialog()
                            .message(caskade_core::tr!(
                                "This backup can't be restored.\n\n{error}",
                                error = error
                            ))
                            .title("Caskade")
                            .kind(MessageDialogKind::Error)
                            .show(move |_| quit.exit(1));
                    }
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_restore_argument_is_resolved_against_the_working_folder() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let cwd = Path::new("/home/player");
        assert_eq!(
            restore_arg(&args(&["caskade", "--restore", "backup.zip"]), cwd),
            Some(PathBuf::from("/home/player/backup.zip"))
        );
        assert_eq!(
            restore_arg(&args(&["caskade", "--restore", "/mnt/usb/b.zip"]), cwd),
            Some(PathBuf::from("/mnt/usb/b.zip"))
        );
        assert_eq!(restore_arg(&args(&["caskade", "--restore"]), cwd), None);
        assert_eq!(restore_arg(&args(&["caskade"]), cwd), None);
    }

    #[tokio::test]
    async fn a_staged_restore_is_applied_when_the_library_opens() {
        let temp = tempfile::tempdir().unwrap();
        let source = AppPaths::under(&temp.path().join("source"));
        source.create().unwrap();
        let store = LibraryStore::open_default(&source).await.unwrap();
        store
            .upsert_game(&caskade_core::model::Game::new("Backed up"))
            .await
            .unwrap();
        let backups = caskade_core::backup::Backups::new(store.clone(), source.clone());
        let exported = backups
            .export(
                caskade_core::backup::ExportOptions {
                    destination: backups.default_destination(),
                    components: Default::default(),
                    set_id: None,
                },
                std::sync::Arc::new(|_| {}),
                caskade_core::backup::CancellationToken::new(),
            )
            .await
            .unwrap();

        let target = AppPaths::under(&temp.path().join("target"));
        target.create().unwrap();
        // The library here doesn't open: restoring is the way back.
        std::fs::write(target.database(), b"damaged").unwrap();
        assert!(open_library(&target).await.is_err());
        stage(&target, &exported.path).await.unwrap();
        let restored = open_library(&target).await.unwrap();
        let games = restored
            .list_games(&caskade_core::model::ListGames::default())
            .await
            .unwrap();
        assert_eq!(games.items[0].title, "Backed up");
        assert!(restore::pending(&target).unwrap().is_none());
        assert!(restore::take_outcome(&target).unwrap().error.is_none());
    }
}
