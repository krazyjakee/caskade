use super::*;
use std::io;

#[test]
fn os_errors_get_a_plain_summary_and_keep_the_cause_as_detail() {
    let error = Error::Io(io::Error::from(io::ErrorKind::NotFound));
    let report = ErrorReport::from(&error);
    assert_eq!(report.code, ErrorCode::MissingFile);
    assert_eq!(report.summary, plain(ErrorCode::MissingFile));
    assert!(report.detail.is_some(), "{report:?}");
    assert!(!report.transient);
}

#[test]
fn text_written_for_people_stays_the_summary() {
    let report = ErrorReport::from(Error::Invalid("a PIN is 4 to 8 digits".into()));
    assert_eq!(report.code, ErrorCode::Invalid);
    assert_eq!(report.summary, "A PIN is 4 to 8 digits");
    assert_eq!(report.detail, None);
    assert!(report.hints.is_empty());

    let forbidden = ErrorReport::from(Error::Forbidden("Sign in to change settings.".into()));
    assert_eq!(forbidden.code, ErrorCode::Forbidden);
    assert_eq!(forbidden.hints, [RecoveryHint::SignIn]);
}

#[test]
fn network_failures_are_transient_and_keep_the_host() {
    let error = Error::Io(io::Error::new(
        io::ErrorKind::TimedOut,
        "metadata.example timed out",
    ));
    let report = ErrorReport::from(&error);
    assert_eq!(report.code, ErrorCode::Offline);
    assert!(report.transient);
    assert_eq!(report.summary, "Metadata.example timed out");
    assert_eq!(
        report.hints,
        [RecoveryHint::CheckConnection, RecoveryHint::Retry]
    );
}

#[test]
fn library_connections_all_busy_is_caskades_own_load_not_a_file_in_use() {
    // A pool timeout: every connection was Caskade's own (#77).
    let settings = crate::settings::SettingsError::Store(Error::Db(sqlx::Error::PoolTimedOut));
    let report = ErrorReport::from_error(&settings);
    assert_eq!(report.code, ErrorCode::Overloaded);
    assert_eq!(report.summary, plain(ErrorCode::Overloaded));
    assert!(report.transient);
    assert_eq!(report.hints, [RecoveryHint::Retry]);
    assert_ne!(report.summary, plain(ErrorCode::Busy));
    // SQLite's own "database is locked" stays a file in use.
    let locked = ErrorReport::from(Error::Io(io::Error::from(io::ErrorKind::ResourceBusy)));
    assert_eq!(locked.code, ErrorCode::Busy);
}

#[test]
fn causes_are_walked_through_wrapping_errors() {
    use crate::settings::SettingsError;
    let error = SettingsError::Store(Error::Io(io::Error::from(io::ErrorKind::PermissionDenied)));
    let report = ErrorReport::from_error(&error);
    assert_eq!(report.code, ErrorCode::AccessDenied);
    assert!(report.hints.contains(&RecoveryHint::CheckPermissions));

    let newer = ErrorReport::from_error(&SettingsError::Version);
    assert_eq!(newer.code, ErrorCode::Unavailable);
    assert!(newer.summary.contains("newer version"), "{newer:?}");
    assert_eq!(newer.hints, [RecoveryHint::UpdateCaskade]);
}

#[test]
fn settings_errors_keep_namespaces_and_fields_out_of_the_summary() {
    use crate::settings::SettingsError as S;
    let unknown = ErrorReport::from_error(&S::Unknown("core.saves".into()));
    assert_eq!(unknown.code, ErrorCode::Unavailable);
    assert!(!unknown.summary.contains("core.saves"), "{unknown:?}");
    assert!(unknown.detail.unwrap().contains("core.saves"));

    let invalid = ErrorReport::from_error(&S::Validation {
        namespace: "core.romm".into(),
        paths: vec!["/cache/dir".into()],
    });
    assert_eq!(invalid.code, ErrorCode::Invalid);
    assert!(
        !invalid.summary.contains("core.romm") && !invalid.summary.contains("/cache"),
        "{invalid:?}"
    );
    assert!(invalid.detail.unwrap().contains("/cache/dir"));

    assert_eq!(ErrorReport::from_error(&S::Limit).code, ErrorCode::Invalid);
    let stored = ErrorReport::from_error(&S::Stored);
    assert_eq!(stored.code, ErrorCode::Corrupt);
    assert!(!stored.summary.contains("envelope"), "{stored:?}");
    let internal = ErrorReport::from_error(&S::Registration("duplicate namespace"));
    assert_eq!(internal.code, ErrorCode::Internal);
    assert!(!internal.summary.contains("namespace"), "{internal:?}");
}

#[test]
fn a_missing_record_is_named_for_people_without_its_id() {
    for entity in [
        "account",
        "collection",
        "entry",
        "game",
        "session",
        "storage root",
        "store account",
    ] {
        let report = ErrorReport::from(crate::db::missing(entity, "0192-abc"));
        assert_eq!(report.code, ErrorCode::Invalid);
        assert!(!report.summary.contains("0192-abc"), "{report:?}");
    }
}

#[test]
fn launch_errors_name_what_stopped_them() {
    use crate::launch::LaunchError;
    let refused = ErrorReport::from_error(&LaunchError::Refused(
        "The plugin “saves” stopped the launch: cloud sync is down".into(),
    ));
    assert_eq!(refused.code, ErrorCode::Refused);
    assert!(refused.summary.contains("cloud sync is down"));

    let invalid = ErrorReport::from_error(&LaunchError::Invalid("the ROM is missing".into()));
    assert_eq!(invalid.summary, "The ROM is missing");

    let running = ErrorReport::from_error(&LaunchError::AlreadyRunning);
    assert_eq!(running.code, ErrorCode::Conflict);
}

#[test]
fn an_unknown_error_keeps_its_own_text() {
    let error: Box<dyn std::error::Error + Send + Sync> = "the plugin answered nothing".into();
    let report = ErrorReport::from_error(&*error);
    assert_eq!(report.code, ErrorCode::Other);
    assert_eq!(report.summary, "The plugin answered nothing");
}

#[test]
fn reports_never_carry_secrets() {
    let error: Box<dyn std::error::Error + Send + Sync> =
        "GET https://user:pw@romm.lan/api/roms?token=abc failed: password=hunter2, Authorization: Bearer eyJ.abc"
            .into();
    let report = ErrorReport::from_error(&*error);
    for text in [Some(report.summary.as_str()), report.detail.as_deref()]
        .into_iter()
        .flatten()
    {
        assert!(!text.contains("abc"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
        assert!(!text.contains("pw@"), "{text}");
        assert!(!text.contains("eyJ"), "{text}");
    }
    assert!(report.summary.contains("romm.lan"), "{report:?}");
    // Prose about secrets keeps its words.
    assert_eq!(
        sanitize("the token expired; enter the password again"),
        "the token expired; enter the password again"
    );
    assert_eq!(
        sanitize(r#"{"token": "abc", "user": "me"}"#),
        r#"{"token": "<redacted>", "user": "me"}"#
    );
}

#[test]
fn every_report_has_a_reference_for_the_logs() {
    let one = ErrorReport::from("one");
    let two = ErrorReport::from("two");
    assert_eq!(one.reference.len(), 8);
    assert_ne!(one.reference, two.reference);
}

#[test]
fn reports_cross_the_boundary_in_camel_case() {
    let report = ErrorReport::new(ErrorCode::StorageOffline, "the drive is gone");
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["code"], "storageOffline");
    assert_eq!(json["hints"][0], "reconnectStorage");
    assert_eq!(json["summary"], "The drive is gone");
}

#[test]
fn docs_list_every_code_and_hint() {
    // A new variant fails to compile here until it is added to the lists.
    fn _exhaustive(code: ErrorCode, hint: RecoveryHint) {
        use ErrorCode::*;
        use RecoveryHint as H;
        match code {
            Invalid | Forbidden | Conflict | Cancelled | Refused | MissingFile | AccessDenied
            | DiskFull | StorageOffline | Busy | Overloaded | Offline | Server | Auth
            | Integrity | Corrupt | Database | Unavailable | Internal | Other => {}
        }
        match hint {
            H::Retry
            | H::RetryLater
            | H::CheckConnection
            | H::ReconnectStorage
            | H::FreeSpace
            | H::CheckPermissions
            | H::CheckCredentials
            | H::SignIn
            | H::Reload
            | H::Restart
            | H::UpdateCaskade => {}
        }
    }
    use ErrorCode::*;
    let codes = [
        Invalid,
        Forbidden,
        Conflict,
        Cancelled,
        Refused,
        MissingFile,
        AccessDenied,
        DiskFull,
        StorageOffline,
        Busy,
        Overloaded,
        Offline,
        Server,
        Auth,
        Integrity,
        Corrupt,
        Database,
        Unavailable,
        Internal,
        Other,
    ];
    use RecoveryHint as H;
    let hints = [
        H::Retry,
        H::RetryLater,
        H::CheckConnection,
        H::ReconnectStorage,
        H::FreeSpace,
        H::CheckPermissions,
        H::CheckCredentials,
        H::SignIn,
        H::Reload,
        H::Restart,
        H::UpdateCaskade,
    ];
    let docs = include_str!("../../../docs/TECH-SPEC.md");
    let names = codes
        .iter()
        .map(|code| serde_json::to_value(code).unwrap())
        .chain(hints.iter().map(|hint| serde_json::to_value(hint).unwrap()));
    for name in names {
        let name = name.as_str().unwrap().to_owned();
        assert!(
            docs.contains(&format!("`{name}`")),
            "docs/TECH-SPEC.md lacks `{name}`"
        );
        // Every code and hint has a plain sentence or label in the webview.
        if hints
            .iter()
            .any(|hint| serde_json::to_value(hint).unwrap() == name.as_str())
        {
            let labels = include_str!("../../../apps/desktop/src/api/reply.ts");
            assert!(
                labels.contains(&format!("  {name}: ")),
                "reply.ts lacks a label for {name}"
            );
        }
    }
}

/// A test catalog, so the test doesn't depend on the bundled translations.
const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "A file or folder Caskade needs is missing."
msgstr "En fil eller mapp som Caskade behöver saknas."
"#;

#[test]
fn summaries_follow_the_language_shown() {
    let missing = || ErrorReport::from_error(&std::io::Error::from(std::io::ErrorKind::NotFound));
    {
        let _sv = crate::i18n::scope_catalog("sv", SV);
        assert_eq!(
            missing().summary,
            "En fil eller mapp som Caskade behöver saknas."
        );
    }
    assert_eq!(
        missing().summary,
        "A file or folder Caskade needs is missing."
    );
}

/// Swedish for an unreachable server.
#[cfg(feature = "net")]
const SV_NET: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "can't reach {host}: {why}"
msgstr "kan inte nå {host}: {why}"

msgid "can't reach {host}"
msgstr "kan inte nå {host}"
"#;

#[cfg(feature = "net")]
#[tokio::test]
async fn an_unreachable_server_reads_in_the_language_shown_also_once_stored() {
    // A port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let error = net(reqwest::get(format!("http://127.0.0.1:{port}/"))
        .await
        .unwrap_err());
    assert!(unreachable(&error), "{error}");
    assert!(
        error.to_string().starts_with("can't reach 127.0.0.1"),
        "{error}"
    );
    // Kept as its message, as an update check's failure is.
    let stored = crate::i18n::Text::of_error(&error).store();

    let _sv = crate::i18n::scope_catalog("sv", SV_NET);
    assert!(
        error.to_string().starts_with("kan inte nå 127.0.0.1"),
        "{error}"
    );
    let report = ErrorReport::from(&error);
    assert_eq!(report.code, ErrorCode::Offline);
    assert!(
        report.summary.starts_with("Kan inte nå 127.0.0.1"),
        "{report:?}"
    );
    assert!(
        crate::i18n::Text::load(&stored)
            .render()
            .starts_with("kan inte nå 127.0.0.1")
    );
}
