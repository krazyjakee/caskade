use caskade_core::{
    AppPaths,
    bus::Bus,
    db::LibraryStore,
    install::{InstallOptions, InstallService, JobSnapshot, JobStatus, runner_paths},
    model::StorageRoot,
    settings::{InstallerSettings, SettingsService, SourceHandling},
};
use caskade_installer::{
    Question, Step, Strategy,
    host::{RunnerInfo, RunnerKind},
    manifest::RootManifest,
    pipeline::{CompatibilityRunner, input},
    plan::{ExecutionPolicy, WindowsMethod},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../caskade-installer/tests/fixtures/bin")
        .join(relative)
        .canonicalize()
        .unwrap()
}
async fn setup(temp: &Path) -> (LibraryStore, SettingsService, InstallService) {
    let store = LibraryStore::in_memory().await.unwrap();
    let path = temp.join("Games");
    std::fs::create_dir(&path).unwrap();
    let identity = RootManifest::new("settings-root").unwrap();
    identity.write(&path).unwrap();
    let mut root = StorageRoot::from_manifest(path.canonicalize().unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(&temp.join("app")))
        .unwrap();
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 2,
            aliases: None,
        },
    )
    .with_settings(settings.clone())
    .with_host(input::host(Some("windows")).unwrap());
    (store, settings, service)
}
async fn save(settings: &SettingsService, value: &InstallerSettings) {
    let previous = settings.get("core.installer").await.unwrap();
    settings
        .set(
            "core.installer",
            serde_json::to_value(value).unwrap(),
            Some(previous.revision),
        )
        .await
        .unwrap();
}
async fn stopped(service: &InstallService, id: &str) -> JobSnapshot {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let job = service.job(id).unwrap();
            if job.status.is_finished() || job.status == JobStatus::NeedsUser {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture job finishes")
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_preferences_change_the_next_real_queued_jobs_plan_and_policy() {
    let temp = tempfile::tempdir().unwrap();
    let (_store, settings, service) = setup(temp.path()).await;
    let mut preferences = InstallerSettings {
        execution: ExecutionPolicy::Ask,
        language: "french".into(),
        ..Default::default()
    };
    save(&settings, &preferences).await;
    let source = fixture("windows/gog-inno-unity/setup.exe")
        .to_string_lossy()
        .into_owned();
    let first = service.install(source.clone(), None).await.unwrap();
    let first = stopped(&service, &first.id).await;
    assert_eq!(first.status, JobStatus::NeedsUser, "{first:?}");
    assert!(
        matches!(first.question, Some(Question::ConfirmExecution { .. })),
        "{first:?}"
    );
    assert_eq!(first.candidates[0].strategy, Strategy::RunSilent);
    let args = first.candidates[0]
        .steps
        .iter()
        .find_map(|s| match s {
            Step::Run { args, .. } => Some(args),
            _ => None,
        })
        .unwrap();
    assert!(args.iter().any(|a| a == "/LANG=french"), "{args:?}");
    assert!(args.iter().any(|a| a == "/NOICONS"), "{args:?}");
    preferences.windows_method = WindowsMethod::Extract;
    preferences.execution = ExecutionPolicy::Never;
    preferences.let_installers_create_shortcuts = true;
    preferences.concurrent_downloads = 1;
    save(&settings, &preferences).await;
    let second = service.install(source, None).await.unwrap();
    let second = stopped(&service, &second.id).await;
    assert_eq!(second.status, JobStatus::Completed, "{second:?}");
    assert_eq!(second.candidates[0].strategy, Strategy::Extract);
    assert!(
        second
            .candidates
            .iter()
            .all(|c| !c.steps.iter().any(|s| matches!(
                s,
                Step::Run { .. } | Step::RunInteractive { .. } | Step::Delegate { .. }
            )))
    );
    assert_eq!(
        service.job(&first.id).unwrap().question,
        first.question,
        "an existing question retains its original policy"
    );
    preferences.windows_method = WindowsMethod::RunInstaller;
    preferences.execution = ExecutionPolicy::Ask;
    preferences.language = "german".into();
    save(&settings, &preferences).await;
    let third = service
        .install(
            fixture("windows/gog-inno-unity/setup.exe")
                .to_string_lossy()
                .into_owned(),
            None,
        )
        .await
        .unwrap();
    let third = stopped(&service, &third.id).await;
    assert!(
        matches!(third.question, Some(Question::ConfirmExecution { .. })),
        "{third:?}"
    );
    let args = third.candidates[0]
        .steps
        .iter()
        .find_map(|s| match s {
            Step::Run { args, .. } => Some(args),
            _ => None,
        })
        .unwrap();
    assert!(args.iter().any(|a| a == "/LANG=german"));
    assert!(!args.iter().any(|a| a == "/NOICONS"));
}

#[tokio::test(flavor = "multi_thread")]
async fn confidence_threshold_changes_the_question_before_any_installer_runs() {
    let temp = tempfile::tempdir().unwrap();
    let (_, settings, service) = setup(temp.path()).await;
    let source = fixture("burn/burn-bundle.exe")
        .to_string_lossy()
        .into_owned();
    let mut preference = InstallerSettings {
        execution: ExecutionPolicy::Ask,
        ask_below_confidence: 1.0,
        ..Default::default()
    };
    save(&settings, &preference).await;
    let job = service.install(source.clone(), None).await.unwrap();
    let first = stopped(&service, &job.id).await;
    assert!(
        matches!(first.question, Some(Question::LowConfidence { .. })),
        "{first:?}"
    );
    preference.ask_below_confidence = 0.0;
    save(&settings, &preference).await;
    let job = service.install(source, None).await.unwrap();
    let second = stopped(&service, &job.id).await;
    assert!(
        matches!(second.question, Some(Question::ConfirmExecution { .. })),
        "{second:?}"
    );
    assert!(first.game_id.is_none() && second.game_id.is_none());
}

#[tokio::test]
async fn empty_v1_settings_migrate_and_schema_rejects_invalid_values() {
    let temp = tempfile::tempdir().unwrap();
    let (store, settings, _) = setup(temp.path()).await;
    store
        .set_setting(
            "settings.section.core.installer",
            &serde_json::json!({"schemaVersion":1,"revision":3,"value":{}}),
        )
        .await
        .unwrap();
    let snapshot = settings.get("core.installer").await.unwrap();
    assert_eq!((snapshot.version, snapshot.revision), (3, 4));
    let defaults = InstallerSettings::default();
    assert_eq!(snapshot.value, serde_json::to_value(&defaults).unwrap());
    assert!(!defaults.let_installers_create_shortcuts);
    assert!(!defaults.keep_downloads);
    assert_eq!(defaults.concurrent_downloads, 2);
    for (key, bad) in [
        ("concurrentDownloads", serde_json::json!(0)),
        ("askBelowConfidence", serde_json::json!(1.1)),
        ("language", serde_json::json!("en\n/unsafe")),
        ("compatibilityRunner", serde_json::json!("unknown")),
    ] {
        let mut value = snapshot.value.clone();
        value[key] = bad;
        assert!(
            settings.set("core.installer", value, None).await.is_err(),
            "{key}"
        );
    }
    assert_eq!(settings.get("core.installer").await.unwrap(), snapshot);
}

#[test]
fn runner_preferences_use_detected_builds_without_changing_the_host_snapshot() {
    let mut host = input::host(Some("linux")).unwrap();
    host.runners = vec![
        RunnerInfo {
            kind: RunnerKind::Proton,
            program: "/fixture/umu-run".into(),
        },
        RunnerInfo {
            kind: RunnerKind::Wine,
            program: "/fixture/wine".into(),
        },
    ];
    host.proton_builds = vec![
        "/fixture/compatibilitytools.d/GE-Proton9-27".into(),
        "/fixture/steamapps/common/Proton 9.0".into(),
    ];
    let original = host.clone();
    for (preference, expected) in [
        (CompatibilityRunner::Proton, "UMU-Proton"),
        (
            CompatibilityRunner::GeProton,
            "/fixture/compatibilitytools.d/GE-Proton9-27",
        ),
        (
            CompatibilityRunner::SteamProton,
            "/fixture/steamapps/common/Proton 9.0",
        ),
    ] {
        let value = InstallerSettings {
            compatibility_runner: preference,
            ..Default::default()
        };
        let paths = runner_paths(&value, &host).unwrap();
        assert_eq!(paths.proton_path, expected);
        assert_eq!(paths.umu_run, Path::new("/fixture/umu-run"));
    }
    let mut missing = host.clone();
    missing.proton_builds.clear();
    assert!(
        runner_paths(
            &InstallerSettings {
                compatibility_runner: CompatibilityRunner::GeProton,
                ..Default::default()
            },
            &missing
        )
        .is_err()
    );
    assert_eq!(host, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_compatibility_family_is_recorded_by_the_next_install() {
    let temp = tempfile::tempdir().unwrap();
    let (store, settings, service) = setup(temp.path()).await;
    let mut host = input::host(Some("linux")).unwrap();
    host.runners.push(RunnerInfo {
        kind: RunnerKind::Wine,
        program: "/fixture/unused-wine".into(),
    });
    let service = service.with_host(host);
    let mut preferences = InstallerSettings {
        compatibility_runner: CompatibilityRunner::SystemWine,
        execution: ExecutionPolicy::Never,
        ..Default::default()
    };
    save(&settings, &preferences).await;
    let first = service
        .install(
            fixture("windows/gog-inno-unity/setup.exe")
                .to_string_lossy()
                .into_owned(),
            None,
        )
        .await
        .unwrap();
    let first = stopped(&service, &first.id).await;
    assert_eq!(first.status, JobStatus::Completed, "{first:?}");
    preferences.compatibility_runner = CompatibilityRunner::Proton;
    save(&settings, &preferences).await;
    let second_source = temp.path().join("Second setup.exe");
    std::fs::copy(fixture("windows/gog-inno-unity/setup.exe"), &second_source).unwrap();
    let second = service
        .install(second_source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let second = stopped(&service, &second.id).await;
    assert_eq!(second.status, JobStatus::Completed, "{second:?}");
    for (job, kind) in [
        (&first, caskade_discovery::RunnerKind::Wine),
        (&second, caskade_discovery::RunnerKind::Umu),
    ] {
        let entry = store
            .entries_for_game(job.game_id.as_deref().unwrap())
            .await
            .unwrap()
            .remove(0);
        let caskade_discovery::GameRuntime::Windows {
            runner: Some(runner),
            ..
        } = entry.runtime
        else {
            panic!("missing runner family");
        };
        assert_eq!(runner.kind, kind);
        assert!(runner.path.is_none());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn wine_launch_choices_keep_the_same_runtime_when_answered() {
    let temp = tempfile::tempdir().unwrap();
    let (store, settings, service) = setup(temp.path()).await;
    let mut host = input::host(Some("linux")).unwrap();
    host.runners.push(RunnerInfo {
        kind: RunnerKind::Wine,
        program: "/fixture/unused-wine".into(),
    });
    let service = service.with_host(host);
    save(
        &settings,
        &InstallerSettings {
            execution: ExecutionPolicy::Never,
            compatibility_runner: CompatibilityRunner::SystemWine,
            ..Default::default()
        },
    )
    .await;
    let folder = temp.path().join("Two games");
    std::fs::create_dir(&folder).unwrap();
    for name in ["Alpha.exe", "Beta.exe"] {
        std::fs::copy(
            fixture("fixture-game-windows-x86_64.exe"),
            folder.join(name),
        )
        .unwrap();
    }
    let job = service
        .install(folder.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let first = stopped(&service, &job.id).await;
    let Some(Question::ChooseLaunchTarget { targets }) = &first.question else {
        panic!("{first:?}");
    };
    assert!(targets.iter().all(|target| matches!(
        target.runtime,
        caskade_installer::RuntimeSpec::WinePrefix { .. }
    )));
    service
        .answer(
            &first.id,
            first.revision,
            caskade_core::install::JobAnswer::Select { index: 0 },
        )
        .await
        .unwrap();
    let finished = stopped(&service, &job.id).await;
    assert_eq!(finished.status, JobStatus::Completed, "{finished:?}");
    let entry = store
        .entries_for_game(finished.game_id.as_deref().unwrap())
        .await
        .unwrap()
        .remove(0);
    assert!(
        matches!(entry.runtime,caskade_discovery::GameRuntime::Windows{runner:Some(runner),..} if runner.kind==caskade_discovery::RunnerKind::Wine)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_windows_payload_does_not_commit_to_a_root_without_prefix_capabilities() {
    let temp = tempfile::tempdir().unwrap();
    let (store, _settings, service) = setup(temp.path()).await;
    let service = service.with_host(input::host(Some("linux-exfat-root")).unwrap());
    let job = service
        .install(fixture("unity.zip").to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let job = stopped(&service, &job.id).await;
    assert_eq!(job.status, JobStatus::Failed, "{job:?}");
    assert!(
        job.error
            .as_deref()
            .unwrap()
            .contains("cannot host an executable game-local compatibility prefix"),
        "{job:?}"
    );
    assert!(
        store
            .entries_for_root("settings-root")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn local_source_delete_happens_only_after_the_managed_install_is_saved() {
    let temp = tempfile::tempdir().unwrap();
    let (store, settings, service) = setup(temp.path()).await;
    let source = temp.path().join("Unity.zip");
    std::fs::copy(fixture("unity.zip"), &source).unwrap();
    save(
        &settings,
        &InstallerSettings {
            source_handling: SourceHandling::Delete,
            ..Default::default()
        },
    )
    .await;
    let job = service
        .install(source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let job = stopped(&service, &job.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    assert!(!source.exists());
    assert_eq!(
        store
            .entries_for_game(job.game_id.as_deref().unwrap())
            .await
            .unwrap()
            .len(),
        1
    );
    let bad = temp.path().join("invalid.bin");
    std::fs::write(&bad, b"not an installer").unwrap();
    let job = service
        .install(bad.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let job = stopped(&service, &job.id).await;
    assert_eq!(job.status, JobStatus::Failed);
    assert!(bad.is_file());
}

#[cfg(feature = "net")]
#[tokio::test(flavor = "multi_thread")]
async fn keep_downloads_is_read_again_for_the_next_url_install() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let temp = tempfile::tempdir().unwrap();
    let (store, settings, service) = setup(temp.path()).await;
    let server = MockServer::start().await;
    let bytes = std::fs::read(fixture("unity.zip")).unwrap();
    for route in ["/keep.zip", "/discard.zip"] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/zip")
                    .set_body_bytes(bytes.clone()),
            )
            .mount(&server)
            .await;
    }
    let mut preferences = InstallerSettings {
        keep_downloads: true,
        source_handling: SourceHandling::Delete,
        ..Default::default()
    };
    save(&settings, &preferences).await;
    let first = service
        .install(format!("{}/keep.zip", server.uri()), None)
        .await
        .unwrap();
    let first = stopped(&service, &first.id).await;
    assert_eq!(first.status, JobStatus::Completed, "{first:?}");
    let root = store.get_root("settings-root").await.unwrap().unwrap();
    assert!(
        root.path
            .join(".caskade/downloads")
            .join(&first.id)
            .is_dir()
    );
    let second_path = temp.path().join("Second");
    std::fs::create_dir(&second_path).unwrap();
    let identity = RootManifest::new("second-root").unwrap();
    identity.write(&second_path).unwrap();
    store
        .upsert_root(&StorageRoot::from_manifest(
            second_path.canonicalize().unwrap(),
            &identity,
        ))
        .await
        .unwrap();
    preferences.keep_downloads = false;
    save(&settings, &preferences).await;
    let second = service
        .install(
            format!("{}/discard.zip", server.uri()),
            Some("second-root".into()),
        )
        .await
        .unwrap();
    let second = stopped(&service, &second.id).await;
    assert_eq!(second.status, JobStatus::Completed, "{second:?}");
    assert!(
        !second_path
            .join(".caskade/downloads")
            .join(&second.id)
            .exists()
    );
    assert!(
        root.path
            .join(".caskade/downloads")
            .join(&first.id)
            .is_dir(),
        "the earlier job retained its settings"
    );
}

#[cfg(feature = "net")]
#[tokio::test(flavor = "multi_thread")]
async fn a_downloaded_installer_runs_by_default_and_asks_under_the_ask_policy() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let defaults = InstallerSettings::default();
    assert_eq!(defaults.execution_for(false), ExecutionPolicy::Allow);
    assert_eq!(defaults.execution_for(true), ExecutionPolicy::Allow);
    let never = InstallerSettings {
        execution: ExecutionPolicy::Never,
        ..Default::default()
    };
    assert_eq!(never.execution_for(true), ExecutionPolicy::Never);
    let ask = InstallerSettings {
        download_execution: ExecutionPolicy::Ask,
        ..Default::default()
    };
    assert_eq!(ask.execution_for(false), ExecutionPolicy::Allow);
    assert_eq!(ask.execution_for(true), ExecutionPolicy::Ask);

    let temp = tempfile::tempdir().unwrap();
    let (_store, settings, service) = setup(temp.path()).await;
    settings
        .set("core.installer", serde_json::to_value(&ask).unwrap(), None)
        .await
        .unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/setup.exe"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/octet-stream")
                .set_body_bytes(
                    std::fs::read(fixture("windows/gog-inno-unity/setup.exe")).unwrap(),
                ),
        )
        .mount(&server)
        .await;
    let job = service
        .install(format!("{}/setup.exe", server.uri()), None)
        .await
        .unwrap();
    let job = stopped(&service, &job.id).await;
    assert_eq!(job.status, JobStatus::NeedsUser, "{job:?}");
    assert!(
        matches!(job.question, Some(Question::ConfirmExecution { .. })),
        "{job:?}"
    );
}

#[tokio::test]
async fn version_2_settings_gain_an_asking_download_policy() {
    let temp = tempfile::tempdir().unwrap();
    let (store, settings, _) = setup(temp.path()).await;
    let mut value = serde_json::to_value(InstallerSettings::default()).unwrap();
    value.as_object_mut().unwrap().remove("downloadExecution");
    store
        .set_setting(
            "settings.section.core.installer",
            &serde_json::json!({"schemaVersion":2,"revision":5,"value":value}),
        )
        .await
        .unwrap();
    let snapshot = settings.get("core.installer").await.unwrap();
    assert_eq!(snapshot.version, 3);
    assert_eq!(snapshot.value["downloadExecution"], "ask");
    assert_eq!(snapshot.value["execution"], "allow");
}
