//! Restores at startup (#192): a restore staged in Settings, or with
//! `caskade --restore <backup>`, is applied before the library opens, and
//! undone if the restored library won't open. When the library can't open
//! at all, [`library_failed`]'s dialog offers Restore from backup…, which
//! [`stage`]s the chosen backup, or an update when a newer Caskade wrote
//! the library.

use caskade_backup::restore;
use caskade_core::error::{ErrorReport, RecoveryHint};
use caskade_core::i18n::Message;
use caskade_core::{AppPaths, db::LibraryStore, tr};
use std::path::{Path, PathBuf};

/// The data folders, made if missing, with `caskade --restore <backup>`
/// staged to apply as the library opens. Ends the process when there is
/// nowhere to keep the library.
pub fn prepare_paths() -> AppPaths {
    let paths = AppPaths::standard()
        .unwrap_or_else(|error| startup_failed("Caskade couldn't find its data folders", &error));
    if let Err(error) = paths.create() {
        startup_failed("Caskade couldn't create its data folders", &error);
    }
    if let Some(archive) = restore_arg(
        &std::env::args().collect::<Vec<_>>(),
        &std::env::current_dir().unwrap_or_default(),
    ) && let Err(error) = crate::runtime::block_on(stage(&paths, &archive))
    {
        startup_failed("Caskade couldn't restore the backup", &error);
    }
    paths
}

fn startup_failed(what: &str, error: &dyn std::fmt::Display) -> ! {
    tracing::error!(%error, "{what}");
    std::process::exit(1)
}

/// `--restore <backup>` from the command line.
fn restore_arg(args: &[String], cwd: &Path) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == "--restore")?;
    args.get(at + 1).map(|path| cwd.join(path))
}

/// Stages `archive` for [`open_library`] to apply; the profile is untouched
/// until then.
pub async fn stage(paths: &AppPaths, archive: &Path) -> Result<(), String> {
    let options = caskade_backup::RestoreOptions {
        archive: archive.to_owned(),
        // The library, and whatever else the backup holds.
        components: caskade_backup::Component::ALL.into_iter().collect(),
    };
    restore::stage(
        paths,
        &options,
        &caskade_backup::CancellationToken::new(),
        &(std::sync::Arc::new(|_| {}) as caskade_backup::ProgressFn),
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

/// What [`library_failed`] needs from a shell: error dialogs that wait for
/// an answer, and its own update, restart and quit.
pub trait StartupDialogs {
    /// Shows `text` with `buttons`, the first the default: the index of
    /// the one pressed, `None` when the dialog was closed.
    fn ask(&self, text: &str, buttons: &[String]) -> Option<usize>;
    /// Picks one file of `filter` (a name and its extensions).
    fn pick_file(&self, title: &str, filter: (&str, &[&str])) -> Option<PathBuf>;
    /// Why this copy can't replace itself; `None` when it can.
    fn cant_update(&self) -> Option<Message>;
    /// Finds, downloads and installs the newest release: its version, or
    /// `None` when the feed has nothing newer.
    fn update(&self) -> Result<Option<String>, String>;
    fn restart(&self);
    fn quit(&self);
}

/// The library store couldn't open: a dialog says why and offers to restore
/// a backup, or to update when a newer Caskade wrote the library; otherwise
/// Caskade quits. Returns once `shell` restarts or quits.
pub fn library_failed(
    error: &(dyn std::error::Error + 'static),
    paths: &AppPaths,
    shell: &impl StartupDialogs,
) {
    tracing::error!(%error, "library unavailable");
    let report = ErrorReport::from_error(error);
    if report.hints.contains(&RecoveryHint::UpdateCaskade) {
        // A backup is older than the library: restoring one would lose
        // what the newer Caskade added.
        return update_for_library(&report.summary, shell);
    }
    let text = tr!(
        "Caskade couldn't open its library.\n\n{error}\n\nYou can restore it from a backup, or start Caskade with --restore <backup file>.",
        error = error
    );
    if shell.ask(&text, &[tr!("Restore from backup…"), tr!("Quit")]) != Some(0) {
        return shell.quit();
    }
    let Some(archive) = shell.pick_file(
        &tr!("Choose a Caskade backup"),
        (&tr!("Caskade backups"), &["zip"]),
    ) else {
        return shell.quit();
    };
    match crate::runtime::block_on(stage(paths, &archive)) {
        Ok(()) => shell.restart(),
        Err(error) => {
            shell.ask(
                &tr!("This backup can't be restored.\n\n{error}", error = error),
                &[tr!("Quit")],
            );
            shell.quit();
        }
    }
}

/// A newer Caskade changed the library in a way this one can't read: a copy
/// that updates itself offers to, installs the newest release and restarts
/// into it; a failed update says why and offers to try again.
fn update_for_library(summary: &str, shell: &impl StartupDialogs) {
    let quit = tr!("Quit");
    if let Some(reason) = shell.cant_update() {
        shell.ask(&format!("{summary}\n\n{}", reason.render()), &[quit]);
        return shell.quit();
    }
    let text = tr!("{summary}\n\nUpdate Caskade to open it.", summary = summary);
    if shell.ask(&text, &[tr!("Update Caskade"), quit.clone()]) != Some(0) {
        return shell.quit();
    }
    loop {
        match shell.update() {
            Ok(Some(_)) => return shell.restart(),
            Ok(None) => {
                shell.ask(
                    &tr!(
                        "No released version of Caskade can open this library yet: a development build changed it."
                    ),
                    &[quit],
                );
                return shell.quit();
            }
            Err(error) => {
                tracing::warn!(%error, "update from the startup dialog failed");
                if shell.ask(&error, &[tr!("Try again"), quit.clone()]) != Some(0) {
                    return shell.quit();
                }
            }
        }
    }
}

fn undo(paths: &AppPaths, why: &str) {
    if let Err(error) = restore::roll_back(paths, why) {
        tracing::error!(%error, "the restore couldn't be undone");
    }
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

    /// Answers dialogs in order and notes what the flow did.
    #[derive(Default)]
    struct Shell {
        answers: std::cell::RefCell<Vec<Option<usize>>>,
        updates: std::cell::RefCell<Vec<Result<Option<String>, String>>>,
        cant_update: Option<Message>,
        did: std::cell::RefCell<Vec<String>>,
    }

    impl StartupDialogs for Shell {
        fn ask(&self, text: &str, _: &[String]) -> Option<usize> {
            self.did.borrow_mut().push(format!("ask {text}"));
            self.answers.borrow_mut().remove(0)
        }
        fn pick_file(&self, _: &str, _: (&str, &[&str])) -> Option<PathBuf> {
            self.did.borrow_mut().push("pick".into());
            None
        }
        fn cant_update(&self) -> Option<Message> {
            self.cant_update.clone()
        }
        fn update(&self) -> Result<Option<String>, String> {
            self.did.borrow_mut().push("update".into());
            self.updates.borrow_mut().remove(0)
        }
        fn restart(&self) {
            self.did.borrow_mut().push("restart".into());
        }
        fn quit(&self) {
            self.did.borrow_mut().push("quit".into());
        }
    }

    fn newer_library() -> caskade_core::settings::SettingsError {
        caskade_core::settings::SettingsError::Version
    }

    #[test]
    fn a_library_from_a_newer_caskade_updates_again_until_it_works() {
        let shell = Shell {
            answers: vec![Some(0), Some(0)].into(),
            updates: vec![Err("offline".into()), Ok(Some("9.0.0".into()))].into(),
            ..Shell::default()
        };
        library_failed(&newer_library(), &AppPaths::under(Path::new("x")), &shell);
        let did = shell.did.take();
        assert!(did[0].ends_with("Update Caskade to open it."), "{did:?}");
        assert_eq!(did[1..], ["update", "ask offline", "update", "restart"]);
    }

    #[test]
    fn a_copy_that_cannot_update_says_why_and_quits() {
        let shell = Shell {
            answers: vec![Some(0)].into(),
            cant_update: Some(caskade_core::msg!(
                "Development builds don't update themselves."
            )),
            ..Shell::default()
        };
        library_failed(&newer_library(), &AppPaths::under(Path::new("x")), &shell);
        let did = shell.did.take();
        assert!(did[0].ends_with("Development builds don't update themselves."));
        assert_eq!(did[1..], ["quit"]);
    }

    #[test]
    fn a_broken_library_offers_a_backup_and_quits_without_one() {
        let shell = Shell {
            answers: vec![Some(0)].into(),
            ..Shell::default()
        };
        let broken = std::io::Error::other("disk gone");
        library_failed(&broken, &AppPaths::under(Path::new("x")), &shell);
        let did = shell.did.take();
        assert!(did[0].contains("restore it from a backup"), "{did:?}");
        assert_eq!(did[1..], ["pick", "quit"]);
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
        let backups = caskade_backup::Backups::new(store.clone(), source.clone());
        let exported = backups
            .export(
                caskade_backup::ExportOptions {
                    destination: backups.default_destination(),
                    components: Default::default(),
                    set_id: None,
                },
                std::sync::Arc::new(|_| {}),
                caskade_backup::CancellationToken::new(),
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
