//! Caskade's own updates: the status a check and an install move through,
//! what a copy that can't update says, and the deb install.
use super::*;
use crate::error::{CommandError, ErrorCode, refused};

/// Asserts that `error` is a report of `code` whose summary names none of
/// `internal` (ids, namespaces, type names).
fn assert_report(error: &CommandError, code: ErrorCode, internal: &[&str]) {
    assert_eq!(error.code, code, "{error:?}");
    for name in internal {
        assert!(
            !error.summary.contains(name),
            "{} names {name}",
            error.summary
        );
    }
}

fn release(version: &str) -> AppRelease {
    AppRelease {
        version: version.into(),
        notes: None,
        date: None,
    }
}

fn updater() -> AppUpdater<&'static str> {
    AppUpdater::new("0.1.0".into(), None, ENDPOINT)
}

/// Why a development build can't update, as the shells say it.
fn development() -> Message {
    caskade_core::msg!("Development builds don't update themselves.")
}

#[test]
fn why_a_copy_cant_update_follows_the_language() {
    let updater = AppUpdater::<()>::new("0.1.0".into(), Some(development()), ENDPOINT);
    assert_eq!(
        updater.status().unavailable.as_deref(),
        Some("Development builds don't update themselves.")
    );
    let _sv = caskade_core::i18n::scope_catalog(
        "sv",
        r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Development builds don't update themselves."
msgstr "Utvecklingsversioner uppdaterar sig inte själva."
"#,
    );
    assert_eq!(
        updater.status().unavailable.as_deref(),
        Some("Utvecklingsversioner uppdaterar sig inte själva.")
    );
}

#[test]
fn a_copy_that_cant_update_refuses_to_check_or_install() {
    let updater = AppUpdater::<()>::new("0.1.0".into(), Some(development()), ENDPOINT);
    assert!(updater.begin_check().unwrap_err().contains("Development"));
    assert!(updater.begin_install().is_err());
    assert_eq!(updater.status().phase, AppUpdatePhase::Idle);
}

#[test]
fn a_check_forgets_the_last_release_and_refuses_while_busy() {
    let updater = updater();
    updater.begin_check().unwrap();
    let found = updater.checked(Ok(Some((release("0.2.0"), "0.2.0"))));
    assert_eq!(found.phase, AppUpdatePhase::Available);
    assert_eq!(found.release, Some(release("0.2.0")));
    assert!(found.checked_at.is_some());

    let checking = updater.begin_check().unwrap();
    assert_eq!(checking.phase, AppUpdatePhase::Checking);
    assert_eq!(checking.release, None);
    assert!(updater.begin_check().is_err(), "a second check at once");
    // A failed check leaves nothing to install.
    let failed = updater.checked(Err("offline".into()));
    assert_eq!(failed.phase, AppUpdatePhase::Failed);
    assert_eq!(failed.error.as_deref(), Some("offline"));
    assert!(
        updater
            .begin_install()
            .unwrap_err()
            .contains("Check for updates")
    );

    updater.begin_check().unwrap();
    assert_eq!(
        updater.checked(Ok(None)).phase,
        AppUpdatePhase::UpToDate,
        "the same version"
    );
    assert_eq!(updater.status().error, None);
}

/// A check that couldn't reach the feed fails with a reason, and the
/// install a shell asks for next is refused as the user's mistake.
#[test]
fn a_failed_check_refuses_an_install() {
    let updater = updater();
    updater.begin_check().unwrap();
    let status = updater.checked(Err(caskade_core::tr!(
        "Couldn't check for updates: {error}",
        error = "500 Internal Server Error"
    )));
    assert_eq!(status.phase, AppUpdatePhase::Failed);
    assert_eq!(status.release, None);
    assert!(
        status
            .error
            .unwrap()
            .starts_with("Couldn't check for updates"),
        "the error says what failed"
    );
    let refused = updater.begin_install().map_err(refused).unwrap_err();
    assert_report(&refused, ErrorCode::Invalid, &[]);
    assert!(
        refused.summary.contains("Check for updates"),
        "{}",
        refused.summary
    );
}

#[test]
fn an_install_downloads_then_waits_for_a_restart() {
    let updater = updater();
    assert!(updater.ensure_ready().is_err());
    updater.begin_check().unwrap();
    updater.checked(Ok(Some((release("0.2.0"), "0.2.0"))));

    let (pending, status) = updater.begin_install().unwrap();
    assert_eq!(pending, "0.2.0");
    assert_eq!(status.phase, AppUpdatePhase::Downloading);
    assert!(updater.begin_check().is_err(), "no check mid-download");
    updater.progress(400, Some(1000));
    let status = updater.progress(600, Some(1000));
    assert_eq!((status.downloaded, status.total), (1000, Some(1000)));
    assert_eq!(updater.installing().phase, AppUpdatePhase::Installing);
    assert_eq!(updater.installed().phase, AppUpdatePhase::Ready);
    updater.ensure_ready().unwrap();
    // Nothing more until the restart.
    assert!(updater.begin_check().unwrap_err().contains("restart"));
    assert!(updater.begin_install().is_err());
}

#[test]
fn a_failed_download_can_be_tried_again_without_another_check() {
    let updater = updater();
    updater.begin_check().unwrap();
    updater.checked(Ok(Some((release("0.2.0"), "0.2.0"))));
    let (pending, _) = updater.begin_install().unwrap();
    updater.progress(10, None);

    let failed = updater.install_failed(Some(pending), "connection reset".into());
    assert_eq!(failed.phase, AppUpdatePhase::Failed);
    assert_eq!(failed.release, Some(release("0.2.0")), "still on offer");
    let (pending, again) = updater.begin_install().unwrap();
    assert_eq!(pending, "0.2.0");
    assert_eq!((again.downloaded, again.error), (0, None));
}

/// A `.deb` goes to the package manager through the elevation command
/// (here a script standing in for pkexec), and pkexec's answers become
/// reasons.
#[cfg(target_os = "linux")]
mod deb {
    use std::ffi::OsString;
    use std::path::Path;

    use super::super::deb::install_with;

    const PACKAGE: &[u8] =
        b"!<arch>\ndebian-binary   0           0     0     100644  4         `\n2.0\n";

    /// Runs the stand-in, which records its arguments and the package it
    /// was given, prints `stderr` and exits with `code`.
    fn elevate(dir: &Path, code: i32, stderr: &str) -> Vec<OsString> {
        let script = dir.join("pkexec.sh");
        std::fs::write(
            &script,
            format!(
                "printf '%s\\n' \"$@\" > '{args}'\nfor last; do :; done\ncp \"$last\" '{copy}'\nprintf '%s\\n' '{stderr}' >&2\nexit {code}\n",
                args = dir.join("args").display(),
                copy = dir.join("package").display(),
            ),
        )
        .unwrap();
        vec!["sh".into(), script.into()]
    }

    #[test]
    fn a_deb_is_installed_by_the_package_manager_as_root() {
        let dir = tempfile::tempdir().unwrap();
        install_with(&elevate(dir.path(), 0, ""), PACKAGE, "0.2.0").unwrap();
        let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
        let args: Vec<&str> = args.lines().collect();
        assert!(
            args[0] == "/usr/bin/apt-get" || args[0] == "/usr/bin/dpkg",
            "{args:?}"
        );
        assert!(
            args.last().unwrap().ends_with("/caskade_0.2.0_amd64.deb"),
            "{args:?}"
        );
        assert_eq!(std::fs::read(dir.path().join("package")).unwrap(), PACKAGE);
        assert!(
            !Path::new(args.last().unwrap()).exists(),
            "the package is removed afterwards"
        );
    }

    #[test]
    fn pkexecs_refusals_and_the_package_managers_errors_are_reasons() {
        let dir = tempfile::tempdir().unwrap();
        let dismissed = install_with(&elevate(dir.path(), 126, ""), PACKAGE, "0.2.0");
        assert!(dismissed.unwrap_err().contains("dismissed"));
        let refused = install_with(&elevate(dir.path(), 127, ""), PACKAGE, "0.2.0");
        assert!(refused.unwrap_err().contains("may not install packages"));
        let failed = install_with(
            &elevate(dir.path(), 100, "E: Could not get lock"),
            PACKAGE,
            "0.2.0",
        );
        let failed = failed.unwrap_err();
        assert!(failed.contains("E: Could not get lock"), "{failed}");
    }

    #[test]
    fn only_a_debian_package_reaches_the_package_manager() {
        let dir = tempfile::tempdir().unwrap();
        let error = install_with(&elevate(dir.path(), 0, ""), b"ELF", "0.2.0").unwrap_err();
        assert!(error.contains("isn't a Debian package"), "{error}");
        assert!(!dir.path().join("args").exists());
        let missing = install_with(&[OsString::from("/nonexistent/pkexec")], PACKAGE, "0.2.0");
        assert!(missing.unwrap_err().contains("pkexec isn't installed"));
    }
}
