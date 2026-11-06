//! Caskade's own updates: which copies can update, the status a check and
//! an install move through, and checks against a release feed served by
//! wiremock on the mock runtime.
use super::*;
use crate::commands::test_ipc::{assert_report, invoke};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

#[test]
fn only_an_rpm_or_a_development_build_cant_update() {
    for bundle in [
        BundleType::AppImage,
        BundleType::Nsis,
        BundleType::Msi,
        BundleType::App,
    ] {
        assert_eq!(unavailable_reason(Some(bundle.clone())), None, "{bundle:?}");
    }
    #[cfg(target_os = "linux")]
    assert_eq!(unavailable_reason(Some(BundleType::Deb)), None);
    let rpm = unavailable_reason(Some(BundleType::Rpm)).unwrap().render();
    assert!(rpm.contains("package manager"), "{rpm}");
    assert!(unavailable_reason(Some(BundleType::Dmg)).is_some());
    let dev = unavailable_reason(None).unwrap().render();
    assert!(dev.contains("Development"), "{dev}");
}

#[test]
fn why_a_copy_cant_update_follows_the_language() {
    let updater = AppUpdater::<()>::new("0.1.0".into(), unavailable_reason(None), ENDPOINT);
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
    let updater = AppUpdater::<()>::new("0.1.0".into(), unavailable_reason(None), ENDPOINT);
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

/// A release feed's `latest.json`, with an entry for every platform the
/// tests may run on.
fn feed(version: &str) -> serde_json::Value {
    let platform = serde_json::json!({
        "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IG5vdCBhIHNpZ25hdHVyZQo=",
        "url": "https://example.invalid/caskade.bundle",
    });
    serde_json::json!({
        "version": version,
        "notes": "Faster library scans.",
        "pub_date": "2026-10-28T12:00:00Z",
        "platforms": {
            "linux-x86_64": platform,
            "linux-aarch64": platform,
            "windows-x86_64": platform,
            "darwin-x86_64": platform,
            "darwin-aarch64": platform,
        },
    })
}

fn app(endpoint: &str) -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
    let specta = tauri_specta::Builder::<MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            app_update_status,
            app_update_check::<MockRuntime>,
            app_update_install::<MockRuntime>,
            app_update_restart::<MockRuntime>,
        ])
        .events(tauri_specta::collect_events![AppUpdateChanged]);
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({
            "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDU4MDI1QjVGRDlERjdBNTgKUldSWWV0L1pYMXNDV05xNkwrSHhpaGN1NTdwdHV6MjQxQ0FhY1JJSU1uc3orRzN3NEdoVHhKVEEK",
        }),
    );
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppUpdater::<Update>::new("0.1.0".into(), None, endpoint))
        .invoke_handler(specta.invoke_handler())
        .build(context)
        .unwrap();
    specta.mount_events(&app);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    (app, window)
}

#[test]
fn a_check_offers_a_newer_release_from_the_feed() {
    let server = tauri::async_runtime::block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(feed("0.2.0")))
            .mount(&server)
            .await;
        server
    });
    let (_app, window) = app(&format!("{}/latest.json", server.uri()));

    let status: AppUpdateStatus =
        serde_json::from_value(invoke(&window, "app_update_check", serde_json::json!({})).unwrap())
            .unwrap();
    assert_eq!(status.phase, AppUpdatePhase::Available);
    assert_eq!(
        status.release,
        Some(AppRelease {
            version: "0.2.0".into(),
            notes: Some("Faster library scans.".into()),
            date: Some("2026-10-28T12:00:00Z".into()),
        })
    );
    let read: AppUpdateStatus = serde_json::from_value(
        invoke(&window, "app_update_status", serde_json::json!({})).unwrap(),
    )
    .unwrap();
    assert_eq!(read, status);
    // Not installed yet: no restart.
    assert!(invoke(&window, "app_update_restart", serde_json::json!({})).is_err());
}

#[test]
fn the_same_version_is_up_to_date_and_a_broken_feed_fails() {
    let server = tauri::async_runtime::block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(feed("0.1.0")))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/broken.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        server
    });

    let (_app, window) = app(&format!("{}/latest.json", server.uri()));
    let status: AppUpdateStatus =
        serde_json::from_value(invoke(&window, "app_update_check", serde_json::json!({})).unwrap())
            .unwrap();
    assert_eq!(status.phase, AppUpdatePhase::UpToDate);
    assert_eq!(status.release, None);

    let (_app, window) = app(&format!("{}/broken.json", server.uri()));
    let status: AppUpdateStatus =
        serde_json::from_value(invoke(&window, "app_update_check", serde_json::json!({})).unwrap())
            .unwrap();
    assert_eq!(status.phase, AppUpdatePhase::Failed);
    assert!(
        status
            .error
            .unwrap()
            .starts_with("Couldn't check for updates"),
        "the error says what failed"
    );
    let refused = invoke(&window, "app_update_install", serde_json::json!({})).unwrap_err();
    assert_report(&refused, "invalid", &[]);
    assert!(
        refused.to_string().contains("Check for updates"),
        "{refused}"
    );
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
