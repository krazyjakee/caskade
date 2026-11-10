#![cfg(target_os = "linux")]
use super::*;
use crate::model::InstallLocation;
use std::{
    path::Path,
    time::{Duration, Instant},
};

async fn stopped(service: &InstallService, id: &str) -> JobSnapshot {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = service.job(id).unwrap();
        let active = lock(&service.inner.jobs).get(id).unwrap().active;
        if !active && (job.status.is_finished() || job.status == JobStatus::NeedsUser) {
            return job;
        }
        assert!(Instant::now() < deadline, "{job:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
async fn setup(root: &Path) -> (LibraryStore, InstallService, LaunchService) {
    std::fs::create_dir_all(root).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("operation-root").unwrap();
    identity.write(root).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let mut record = StorageRoot::from_manifest(root.into(), &identity);
    record.is_default = true;
    store.upsert_root(&record).await.unwrap();
    let launch = LaunchService::new(store.clone()).with_aliases(None);
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    (store, service, launch)
}
fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    crate::test_programs::copy_program(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        path,
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
#[tokio::test(flavor = "multi_thread")]
async fn question_selection_then_verify_and_uninstall_use_server_owned_records() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Games");
    let (store, service, launch) = setup(&root).await;
    let source = t.path().join("Choice Game");
    std::fs::create_dir(&source).unwrap();
    executable(&source.join("alpha.x86_64"));
    executable(&source.join("bravo.x86_64"));
    let job = service
        .install(source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let question = stopped(&service, &job.id).await;
    assert_eq!(question.status, JobStatus::NeedsUser, "{question:?}");
    let Some(Question::ChooseLaunchTarget { targets }) = &question.question else {
        panic!("{question:?}");
    };
    let chosen = targets[1].path.clone();
    assert!(
        service
            .answer(
                &job.id,
                question.revision - 1,
                JobAnswer::Select { index: 0 }
            )
            .await
            .is_err()
    );
    assert!(
        service
            .answer(&job.id, question.revision, JobAnswer::Select { index: 99 })
            .await
            .is_err()
    );
    assert!(
        service
            .answer(&job.id, question.revision - 1, JobAnswer::Cancel)
            .await
            .is_err()
    );
    service
        .answer(&job.id, question.revision, JobAnswer::Select { index: 1 })
        .await
        .unwrap();
    let completed = stopped(&service, &job.id).await;
    assert_eq!(completed.status, JobStatus::Completed, "{completed:?}");
    let entries = store
        .entries_for_game(completed.game_id.as_deref().unwrap())
        .await
        .unwrap();
    let entry = &entries[0];
    assert!(
        matches!(&entry.launch,Some(crate::model::LaunchSpec::Executable{path,..}) if path==&chosen)
    );
    let verify = service.verify(entry.id.clone()).await.unwrap();
    let verified = stopped(&service, &verify.id).await;
    assert!(verified.verification.unwrap().valid);
    let Some(InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
        panic!()
    };
    let payload = root.join(rel_dir).join(&chosen);
    let original = std::fs::read(&payload).unwrap();
    std::fs::write(&payload, b"changed").unwrap();
    let verify = service.verify(entry.id.clone()).await.unwrap();
    assert!(
        !stopped(&service, &verify.id)
            .await
            .verification
            .unwrap()
            .valid
    );
    std::fs::write(&payload, original).unwrap();
    let lease = launch.try_game_lease(&entry.game_id).unwrap();
    assert!(
        service
            .uninstall(entry.id.clone(), launch.clone())
            .await
            .is_err()
    );
    drop(lease);
    let remove = service.uninstall(entry.id.clone(), launch).await.unwrap();
    assert_eq!(
        stopped(&service, &remove.id).await.status,
        JobStatus::Completed
    );
    assert!(store.get_entry(&entry.id).await.unwrap().is_none());
    assert!(!root.join(rel_dir).exists());
    assert!(source.exists());
}

#[cfg(feature = "net")]
#[tokio::test(flavor = "multi_thread")]
async fn standalone_fetch_reports_download_phase_without_creating_a_library_game() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let t = tempfile::tempdir().unwrap();
    let (store, service, _) = setup(&t.path().join("Games")).await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/game.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/octet-stream")
                .set_body_bytes(b"fixture download".to_vec()),
        )
        .mount(&server)
        .await;
    let job = service
        .fetch(format!("{}/game.bin", server.uri()), None)
        .await
        .unwrap();
    let finished = stopped(&service, &job.id).await;
    assert_eq!(finished.status, JobStatus::Completed, "{finished:?}");
    assert_eq!(finished.phase, Some(JobPhase::Download));
    assert_eq!(
        std::fs::read(finished.target_dir.unwrap()).unwrap(),
        b"fixture download"
    );
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
    assert!(finished.game_id.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_install_retries_as_it_ran() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Games");
    let (_store, service, _launch) = setup(&root).await;
    let source = t.path().join("Retry Game");
    let job = service
        .install(source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let failed = stopped(&service, &job.id).await;
    assert_eq!(failed.status, JobStatus::Failed, "{failed:?}");
    std::fs::create_dir(&source).unwrap();
    executable(&source.join("game.x86_64"));
    let retried = service.retry(&job.id, None).await.unwrap();
    assert_eq!(retried.status, JobStatus::Queued);
    assert_eq!(retried.error, None);
    let done = stopped(&service, &job.id).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert!(done.game_id.is_some());
    assert!(service.retry(&job.id, None).await.is_err());
}

/// A job shows its question just before its task ends (#117): an answer to
/// the question as shown, sent while the task is still ending, is taken.
/// The ending used to bump the revision without changing anything, so the
/// answer was refused as stale.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_sent_while_the_task_is_ending_is_taken() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Games");
    let (_store, service, _launch) = setup(&root).await;
    let source = t.path().join("Choice Game");
    std::fs::create_dir(&source).unwrap();
    executable(&source.join("alpha.x86_64"));
    executable(&source.join("bravo.x86_64"));
    let job = service
        .install(source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let question = stopped(&service, &job.id).await;
    assert_eq!(question.status, JobStatus::NeedsUser, "{question:?}");
    // As if the task had posted its question and not ended yet.
    lock(&service.inner.jobs).get_mut(&job.id).unwrap().active = true;
    let answering = tokio::spawn({
        let service = service.clone();
        let (id, revision) = (job.id.clone(), question.revision);
        async move {
            service
                .answer(&id, revision, JobAnswer::Select { index: 0 })
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    // The task ends: nothing about the job changes.
    service.end_operation(&job.id, Ok(()), false);
    assert_eq!(service.job(&job.id).unwrap().revision, question.revision);
    answering.await.unwrap().unwrap();
    assert_eq!(
        stopped(&service, &job.id).await.status,
        JobStatus::Completed
    );
}
