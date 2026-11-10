//! #158 in the job queue: failures carry their cause and recovery actions,
//! only transient ones retry on their own (a bounded number of times), and
//! the user's recovery choices run the job again from a checked state.
use super::*;
use caskade_installer::failure::SecurityNote;
use std::os::unix::fs::PermissionsExt;
#[cfg(feature = "net")]
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

async fn stopped(service: &InstallService, id: &str) -> JobSnapshot {
    for _ in 0..1200 {
        let job = service.job(id).unwrap();
        if job.status.is_finished() || job.status == JobStatus::NeedsUser {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {id} did not stop: {:?}", service.job(id));
}

/// A root registered under `id` in `parent`.
async fn root(store: &crate::db::LibraryStore, parent: &Path, id: &str) -> StorageRoot {
    let path = parent.join(id);
    std::fs::create_dir_all(&path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new(id).unwrap();
    identity.write(&path).unwrap();
    let root = StorageRoot::from_manifest(std::fs::canonicalize(&path).unwrap(), &identity);
    store.upsert_root(&root).await.unwrap();
    root
}

async fn service() -> (crate::db::LibraryStore, InstallService) {
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    (store, service)
}

/// A portable copy of the installer's fixture game.
fn game(dir: &Path) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let exe = dir.join("fixture-runner.x86_64");
    crate::test_programs::copy_program(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &exe,
    );
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("data.pak"), vec![7u8; 4096]).unwrap();
    dir.to_string_lossy().into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_job_can_be_removed_from_activity() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service().await;
    root(&store, temp.path(), "root-a").await;
    let gone = temp.path().join("Removed Game");
    let queued = service
        .install(gone.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let job = stopped(&service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Failed, "{job:?}");
    service.remove(&queued.id).unwrap();
    assert!(service.job(&queued.id).is_none());
    assert!(service.jobs().iter().all(|job| job.id != queued.id));
    assert!(service.remove(&queued.id).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_source_waits_for_the_user_then_installs_from_the_located_one() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service().await;
    root(&store, temp.path(), "root-a").await;
    let gone = temp.path().join("Removed Game");
    let queued = service
        .install(gone.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let job = stopped(&service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Failed, "{job:?}");
    let failure = job.failure.clone().expect("classified");
    assert_eq!(failure.kind, FailureKind::MissingSource);
    assert_eq!(failure.path.as_deref(), Some(gone.as_path()));
    assert_eq!(failure.security, Some(SecurityNote::Possible));
    assert_eq!(
        failure.actions,
        [RecoveryAction::LocateSource, RecoveryAction::Cancel]
    );
    assert!(failure.retry_at_ms.is_none());

    // Nothing retries on its own.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(service.job(&queued.id).unwrap().revision, job.revision);

    // Repeats are counted.
    service.retry(&queued.id, None).await.unwrap();
    let again = stopped(&service, &queued.id).await;
    let failure = again.failure.expect("classified");
    assert_eq!(failure.occurrences, 2);
    assert!(failure.summary.contains("happened again"), "{failure:?}");

    let located = game(&temp.path().join("Fixture Runner"));
    service
        .recover(&queued.id, Recovery::LocateSource { input: located })
        .await
        .unwrap();
    let done = stopped(&service, &queued.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert!(done.failure.is_none());
    assert!(done.game_id.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_destination_can_install_into_another_root() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service().await;
    let locked = root(&store, temp.path(), "root-a").await;
    let other = root(&store, temp.path(), "root-b").await;
    std::fs::set_permissions(&locked.path, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::write(locked.path.join("probe"), b"").is_ok() {
        // Running as root: permissions aren't enforced.
        return;
    }
    let source = game(&temp.path().join("Fixture Runner"));
    let queued = service
        .install(source.clone(), Some(locked.id.clone()))
        .await
        .unwrap();
    let job = stopped(&service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Failed, "{job:?}");
    let failure = job.failure.expect("classified");
    assert_eq!(failure.kind, FailureKind::AccessDenied, "{failure:?}");
    assert!(!failure.transient);
    assert!(failure.actions.contains(&RecoveryAction::ChooseDestination));
    service
        .recover(
            &queued.id,
            Recovery::ChooseDestination {
                root_id: other.id.clone(),
            },
        )
        .await
        .unwrap();
    let done = stopped(&service, &queued.id).await;
    std::fs::set_permissions(&locked.path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert_eq!(done.root_id.as_deref(), Some("root-b"));
    assert!(done.target_dir.unwrap().starts_with(&other.path));
    // The source is the user's: still there.
    assert!(Path::new(&source).join("data.pak").is_file());
}

#[cfg(feature = "net")]
#[tokio::test(flavor = "multi_thread")]
async fn network_loss_retries_a_bounded_number_of_times_then_waits() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service().await;
    root(&store, temp.path(), "root-a").await;
    service.set_retry_delays(vec![Duration::from_millis(20), Duration::from_millis(20)]);
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let queued = service
        .install(format!("{}/game.zip?token=secret", server.uri()), None)
        .await
        .unwrap();
    // The first run and both automatic retries fail; then it waits.
    let mut last = None;
    for _ in 0..1200 {
        let job = service.job(&queued.id).unwrap();
        if job.status == JobStatus::Failed
            && job
                .failure
                .as_ref()
                .is_some_and(|failure| failure.retry_at_ms.is_none())
        {
            last = Some(job);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let job = last.expect("the job stopped retrying");
    let failure = job.failure.unwrap();
    assert_eq!(failure.kind, FailureKind::Download);
    assert_eq!(failure.stage, FailureStage::Download);
    assert!(failure.transient);
    assert!(!failure.detail.contains("secret"), "{failure:?}");
    assert!(!job.error.unwrap_or_default().contains("secret"));
    let requests = server.received_requests().await.unwrap().len();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        requests,
        "no more retries"
    );
    // Each run tries the download as often as the fetch engine allows.
    let per_run = caskade_installer::fetch::FetchConfig::default().retries as usize + 1;
    assert!(requests >= per_run * 3, "{requests}");
}

#[cfg(feature = "net")]
#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_failed_job_stops_its_automatic_retry() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service().await;
    root(&store, temp.path(), "root-a").await;
    service.set_retry_delays(vec![Duration::from_secs(600)]);
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let queued = service
        .install(format!("{}/game.zip", server.uri()), None)
        .await
        .unwrap();
    let mut waiting = None;
    for _ in 0..1200 {
        let job = service.job(&queued.id).unwrap();
        if job
            .failure
            .as_ref()
            .is_some_and(|failure| failure.retry_at_ms.is_some())
        {
            waiting = Some(job);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    waiting.expect("an automatic retry is scheduled");
    let job = service.cancel(&queued.id).unwrap();
    assert!(job.failure.unwrap().retry_at_ms.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_installs_are_rolled_back_at_startup_and_games_stay() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service().await;
    let root = root(&store, temp.path(), "root-a").await;
    let staged = root.path.join(".caskade/staging/interrupted");
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::write(staged.join("partial.bin"), b"half").unwrap();
    let installed = root.path.join("Installed Game");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(installed.join("game.bin"), b"keep").unwrap();
    service.recover_interrupted_installs().await;
    assert!(!staged.exists());
    assert_eq!(std::fs::read(installed.join("game.bin")).unwrap(), b"keep");
    assert!(
        caskade_installer::manifest::RootManifest::read(&root.path).is_ok(),
        "the root's identity stays"
    );
}
