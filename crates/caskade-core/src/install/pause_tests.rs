//! #131: a downloading install pauses with its download kept, stays paused
//! across a restart (a new service over the same library), and resumes with
//! a range request from where it stopped, then installs.
use super::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A portable game as a stored (uncompressed) zip, big enough to stop in
/// the middle of.
fn game_zip() -> Vec<u8> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    writer
        .start_file("Paused Game/fixture-runner.x86_64", options)
        .unwrap();
    writer
        .write_all(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
            ))
            .unwrap(),
        )
        .unwrap();
    writer
        .start_file("Paused Game/data.pak", options.unix_permissions(0o644))
        .unwrap();
    writer.write_all(&vec![7u8; 4 * 1024 * 1024]).unwrap();
    writer.finish().unwrap().into_inner()
}

/// A local HTTP server for one file. While `stall` is set, a full request
/// gets the headers and the first half of the body, then nothing more; a
/// range request gets the rest. Records each request's `Range`.
struct Server {
    url: String,
    ranges: Arc<Mutex<Vec<Option<String>>>>,
    stall: Arc<AtomicBool>,
}

async fn serve(body: Vec<u8>) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/paused-game.zip", listener.local_addr().unwrap());
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let stall = Arc::new(AtomicBool::new(true));
    let body = Arc::new(body);
    let (seen, stalling) = (ranges.clone(), stall.clone());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (body, seen, stalling) = (body.clone(), seen.clone(), stalling.clone());
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => request.extend_from_slice(&buffer[..read]),
                    }
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                let head = text.starts_with("HEAD ");
                let range = text.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("range")
                        .then(|| value.trim().to_owned())
                });
                if !head {
                    lock(&seen).push(range.clone());
                }
                let total = body.len();
                let start = range
                    .as_deref()
                    .and_then(|range| range.strip_prefix("bytes="))
                    .and_then(|range| range.trim_end_matches('-').parse::<usize>().ok());
                let (status, from) = match start {
                    Some(start) if start < total => ("206 Partial Content", start),
                    _ => ("200 OK", 0),
                };
                let mut headers = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nETag: \"v1\"\r\n\
                     Accept-Ranges: bytes\r\nContent-Type: application/zip\r\n\
                     Connection: close\r\n",
                    total - from
                );
                if from > 0 {
                    headers.push_str(&format!(
                        "Content-Range: bytes {from}-{}/{total}\r\n",
                        total - 1
                    ));
                }
                headers.push_str("\r\n");
                if socket.write_all(headers.as_bytes()).await.is_err() || head {
                    return;
                }
                if from == 0 && stalling.load(Ordering::SeqCst) {
                    let _ = socket.write_all(&body[..total / 2]).await;
                    let _ = socket.flush().await;
                    // Nothing more until the client goes.
                    let _ = socket.read(&mut buffer).await;
                    return;
                }
                let _ = socket.write_all(&body[from..]).await;
            });
        }
    });
    Server { url, ranges, stall }
}

async fn root(store: &crate::db::LibraryStore, parent: &Path) -> StorageRoot {
    let path = parent.join("root-a");
    std::fs::create_dir_all(&path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-a").unwrap();
    identity.write(&path).unwrap();
    let root = StorageRoot::from_manifest(std::fs::canonicalize(&path).unwrap(), &identity);
    store.upsert_root(&root).await.unwrap();
    root
}

fn service(store: &crate::db::LibraryStore) -> InstallService {
    InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    )
}

async fn until(
    service: &InstallService,
    id: &str,
    what: &str,
    done: impl Fn(&JobSnapshot) -> bool,
) -> JobSnapshot {
    for _ in 0..1200 {
        if let Some(job) = service.job(id)
            && done(&job)
        {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {id} never {what}: {:?}", service.job(id));
}

/// The partial download's size.
fn part_len(root: &Path, id: &str) -> u64 {
    std::fs::metadata(
        root.join(".caskade/downloads")
            .join(id)
            .join("paused-game.zip.part"),
    )
    .map_or(0, |meta| meta.len())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_download_survives_a_restart_and_resumes_where_it_stopped() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("library.db");
    let store = crate::db::LibraryStore::open(&database).await.unwrap();
    let root = root(&store, temp.path()).await;
    let body = game_zip();
    let half = (body.len() / 2) as u64;
    let server = serve(body).await;

    let first = service(&store);
    let queued = first.install(server.url.clone(), None).await.unwrap();
    assert!(queued.can_pause());
    let id = queued.id.clone();
    until(&first, &id, "downloaded half", |job| {
        job.status == JobStatus::Downloading && part_len(&root.path, &id) >= half
    })
    .await;
    let paused = first.pause(&id).await.unwrap();
    assert_eq!(paused.status, JobStatus::Paused, "{paused:?}");
    assert!(!paused.can_pause());
    assert!(first.pause(&id).await.is_err(), "already paused");
    // Its download stays.
    assert_eq!(part_len(&root.path, &id), half);
    // It doesn't run again on its own.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(first.job(&id).unwrap().status, JobStatus::Paused);
    assert_eq!(lock(&server.ranges).len(), 1);

    // The app quits and starts again.
    drop(first);
    store.close().await;
    let store = crate::db::LibraryStore::open(&database).await.unwrap();
    let second = service(&store);
    assert_eq!(second.restore_paused(None).await, 1);
    let restored = second.job(&id).expect("the paused job is listed again");
    assert_eq!(restored.status, JobStatus::Paused);
    assert_eq!(restored.input, server.url);
    // Restoring twice doesn't list it twice.
    assert_eq!(second.restore_paused(None).await, 0);

    server.stall.store(false, Ordering::SeqCst);
    let resumed = second.resume(&id).await.unwrap();
    assert_eq!(resumed.status, JobStatus::Queued);
    assert!(
        second.resume(&id).await.is_err(),
        "only a paused job resumes"
    );
    let done = until(&second, &id, "finished", |job| job.status.is_finished()).await;
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert!(done.game_id.is_some());
    // The second run asked for the rest only.
    let ranges = lock(&server.ranges).clone();
    assert_eq!(
        ranges,
        [None, Some(format!("bytes={half}-"))],
        "requests: {ranges:?}"
    );
    // No longer kept as paused.
    assert!(store.paused_jobs().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_pause_is_forgotten_and_a_local_install_does_not_pause() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let root = root(&store, temp.path()).await;
    let server = serve(game_zip()).await;
    let service = service(&store);
    let queued = service.install(server.url.clone(), None).await.unwrap();
    let id = queued.id.clone();
    until(&service, &id, "downloading", |job| {
        job.status == JobStatus::Downloading && part_len(&root.path, &id) > 0
    })
    .await;
    service.pause(&id).await.unwrap();
    assert_eq!(store.paused_jobs().await.unwrap().len(), 1);
    let cancelled = service.cancel(&id).unwrap();
    assert_eq!(cancelled.status, JobStatus::Cancelled);
    for _ in 0..100 {
        if store.paused_jobs().await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(store.paused_jobs().await.unwrap().is_empty());
    assert!(service.resume(&id).await.is_err());

    // A folder on disk has nothing to download, so nothing to pause.
    let folder = temp.path().join("Local Game");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("game.bin"), b"x").unwrap();
    let local = service
        .install(folder.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    assert!(!local.can_pause());
    assert!(service.pause(&local.id).await.is_err());
}
