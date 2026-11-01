//! Torrent inputs (#159) against a local seeder: a second librqbit session
//! on 127.0.0.1 without DHT, trackers or local discovery, so nothing here
//! reaches a public swarm.
#![cfg(feature = "torrent")]

use caskade_installer::{
    InstallOutcome, Question,
    events::{EventSink, JobEvent},
    exec::CancellationToken,
    failure::{self, FailureKind, FailureStage, Probe, RecoveryAction},
    fetch::{
        FetchError,
        torrent::{Fetched, Source, TorrentConfig, TorrentEngine},
    },
    pipeline::{self, Context, NeedsDecision},
    testing::{FakeDiskSpace, RecordingSink},
};
use librqbit::{
    CreateTorrentOptions, ListenerMode, ListenerOptions, Session, SessionOptions,
    limits::LimitsConfig,
};
use std::{
    fs,
    net::SocketAddr,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin")
        .join(name)
}

/// Deterministic, incompressible-looking bytes.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

/// A librqbit session seeding one file or folder from 127.0.0.1.
struct Seeder {
    _session: Arc<Session>,
    peer: SocketAddr,
    torrent: Vec<u8>,
    magnet: String,
    _dir: tempfile::TempDir,
}

impl Seeder {
    /// Seeds what `build` writes into an empty folder (and returns).
    async fn new(build: impl FnOnce(&Path) -> PathBuf, upload_bps: Option<u32>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let content = build(dir.path());
        // librqbit's tracker client has no TLS provider of its own; the
        // engine installs ring before its session, and so does the seeder.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let session = Session::new_with_opts(
            dir.path().to_path_buf(),
            SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                persistence: None,
                listen: Some(ListenerOptions {
                    mode: ListenerMode::TcpOnly,
                    listen_addr: "127.0.0.1:0".parse().unwrap(),
                    ipv4_only: true,
                    ..Default::default()
                }),
                ratelimits: LimitsConfig {
                    upload_bps: upload_bps.and_then(NonZeroU32::new),
                    download_bps: None,
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (created, handle) = session
            .create_and_serve_torrent(
                &content,
                CreateTorrentOptions {
                    name: None,
                    trackers: vec![],
                    piece_length: Some(16 * 1024),
                },
            )
            .await
            .unwrap();
        handle.wait_until_initialized().await.unwrap();
        Self {
            peer: session.listen_addr().unwrap(),
            torrent: created.as_bytes().unwrap().to_vec(),
            magnet: created.as_magnet().to_string(),
            _session: session,
            _dir: dir,
        }
    }

    /// Seeds a copy of the zip fixture `name`.
    async fn file(name: &str) -> Self {
        Self::new(
            |dir| {
                let path = dir.join(name);
                fs::copy(fixture(name), &path).unwrap();
                path
            },
            None,
        )
        .await
    }
}

/// Only the local seeder, and short waits.
fn config(peers: &[SocketAddr]) -> TorrentConfig {
    TorrentConfig {
        dht: false,
        trackers: false,
        initial_peers: peers.to_vec(),
        metadata_timeout: Duration::from_secs(30),
        stall_timeout: Duration::from_secs(30),
        disk_margin_bytes: 0,
        ..TorrentConfig::default()
    }
}

fn context(peers: &[SocketAddr]) -> Context {
    let mut ctx = Context::new(Arc::new(RecordingSink::default()), CancellationToken::new());
    ctx.torrent = config(peers);
    ctx
}

fn engine(peers: &[SocketAddr], sink: Arc<dyn EventSink>) -> TorrentEngine {
    TorrentEngine::new(
        config(peers),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink,
    )
}

fn no_answer(_: &Question) -> Option<usize> {
    None
}

async fn downloaded(
    engine: &TorrentEngine,
    root: &Path,
    job: &str,
    source: &Source,
) -> Result<PathBuf, FetchError> {
    match engine
        .download(root, job, source, &no_answer, CancellationToken::new())
        .await?
    {
        Fetched::Downloaded(torrent) => Ok(torrent.path),
        Fetched::NeedsUser(question) => panic!("unexpected question {question:?}"),
    }
}

async fn install(input: String, root: &Path, keep: bool, ctx: &Context) -> PathBuf {
    let outcome = pipeline::install(input, root.into(), keep, false, None, ctx)
        .await
        .unwrap();
    let InstallOutcome::Installed(installed) = outcome else {
        panic!("{outcome:?}");
    };
    root.join(installed.dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_magnet_link_downloads_from_a_local_peer_and_installs() {
    let seeder = Seeder::file("unity.zip").await;
    let root = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let mut ctx = Context::new(sink.clone(), CancellationToken::new());
    ctx.torrent = config(&[seeder.peer]);
    let game = install(seeder.magnet.clone(), root.path(), false, &ctx).await;
    assert!(game.is_dir());
    // Without Keep downloads the torrent's data goes after the install.
    assert!(!root.path().join(".caskade/torrents").exists());
    let events = sink.events();
    for message in [
        "Finding peers for the torrent's metadata",
        "Download complete",
    ] {
        assert!(
            events.iter().any(|event| event.message == message),
            "{message}: {events:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_torrent_file_and_an_http_hosted_torrent_install() {
    let seeder = Seeder::file("unity.zip").await;
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("Unity Game.torrent");
    fs::write(&file, &seeder.torrent).unwrap();

    let root = temp.path().join("Games");
    let sink = Arc::new(RecordingSink::default());
    let mut ctx = Context::new(sink.clone(), CancellationToken::new());
    ctx.torrent = config(&[seeder.peer]);
    let game = install(file.to_string_lossy().into_owned(), &root, true, &ctx).await;
    assert!(game.is_dir());
    // Keep downloads keeps the verified data; the .torrent is the user's.
    assert!(root.join(".caskade/torrents").is_dir());
    assert!(file.is_file());
    let events = sink.events();
    assert!(
        events
            .iter()
            .any(|event| event.message == "Download complete"
                && event.total_bytes == Some(fs::metadata(fixture("unity.zip")).unwrap().len())),
        "{events:?}"
    );

    let server = MockServer::start().await;
    Mock::given(path("/unity.torrent"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-bittorrent")
                .set_body_bytes(seeder.torrent.clone()),
        )
        .mount(&server)
        .await;
    let other = temp.path().join("Other");
    let game = install(
        format!("{}/unity.torrent", server.uri()),
        &other,
        false,
        &context(&[seeder.peer]),
    )
    .await;
    assert!(game.is_dir());
    assert!(!other.join(".caskade/torrents").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_torrent_of_several_games_asks_which_and_downloads_only_that() {
    let seeder = Seeder::new(
        |dir| {
            let pack = dir.join("Pack");
            for (folder, zip) in [("Game A", "unity.zip"), ("Game B", "unreal.zip")] {
                fs::create_dir_all(pack.join(folder)).unwrap();
                fs::copy(fixture(zip), pack.join(folder).join(zip)).unwrap();
            }
            fs::write(pack.join("readme.txt"), b"two games").unwrap();
            pack
        },
        None,
    )
    .await;
    let root = tempfile::tempdir().unwrap();
    let ctx = context(&[seeder.peer]);
    let error = pipeline::install(
        seeder.magnet.clone(),
        root.path().into(),
        true,
        false,
        None,
        &ctx,
    )
    .await
    .unwrap_err();
    let Some(NeedsDecision(question)) = error.downcast_ref::<NeedsDecision>() else {
        panic!("{error:?}");
    };
    let Question::ChooseAsset { assets } = question else {
        panic!("{question:?}");
    };
    let names: Vec<_> = assets.iter().map(|asset| asset.name.as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names[0].starts_with("Everything"), "{names:?}");
    let game_a = names.iter().position(|name| *name == "Game A/").unwrap();
    assert_eq!(
        assets[game_a].size,
        Some(fs::metadata(fixture("unity.zip")).unwrap().len())
    );
    // Nothing but the metadata was downloaded before the answer.
    let data = root
        .path()
        .join(".caskade/torrents")
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("data");
    assert_eq!(fs::read_dir(&data).unwrap().count(), 0);

    let mut ctx = context(&[seeder.peer]);
    ctx.control.answers.push((question.clone(), game_a as u32));
    let game = install(seeder.magnet.clone(), root.path(), true, &ctx).await;
    assert!(game.is_dir());
    let other = data.join("Pack/Game B/unreal.zip");
    assert!(
        fs::read(&other).unwrap_or_default() != fs::read(fixture("unreal.zip")).unwrap(),
        "the other game was downloaded too"
    );
}

/// Cancels its job once bytes have arrived.
struct CancelOnProgress {
    token: CancellationToken,
}

impl EventSink for CancelOnProgress {
    fn emit(&self, event: JobEvent) {
        if event.message.starts_with("Downloading") && event.bytes > 0 {
            self.token.cancel();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_interrupted_download_keeps_verified_pieces_and_resumes() {
    let payload = noise(1024 * 1024, 7);
    let seeder = Seeder::new(
        |dir| {
            let path = dir.join("game.bin");
            fs::write(&path, &payload).unwrap();
            path
        },
        Some(256 * 1024),
    )
    .await;
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("game.torrent");
    fs::write(&file, &seeder.torrent).unwrap();
    let source = Source::File(file);
    let root = temp.path().join("Games");
    fs::create_dir(&root).unwrap();

    let token = CancellationToken::new();
    let first = Arc::new(CancelOnProgress {
        token: token.clone(),
    });
    let error = engine(&[seeder.peer], first)
        .download(&root, "first", &source, &no_answer, token)
        .await
        .unwrap_err();
    assert!(matches!(error, FetchError::Cancelled), "{error:?}");

    // The next job (as after a restart) verifies what is on disk: without
    // a peer, the bytes it counts can only be the kept pieces.
    let sink = Arc::new(RecordingSink::default());
    let mut offline = config(&[]);
    offline.stall_timeout = Duration::from_secs(1);
    let error = downloaded(
        &TorrentEngine::new(
            offline,
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            sink.clone(),
        ),
        &root,
        "second",
        &source,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, FetchError::NoPeers { .. }), "{error:?}");
    let events = sink.events();
    assert!(
        events
            .iter()
            .any(|event| event.message == "Connecting to peers" && event.bytes > 0),
        "{events:?}"
    );

    // With the seeder back, it finishes.
    let path = downloaded(
        &engine(&[seeder.peer], Arc::new(RecordingSink::default())),
        &root,
        "third",
        &source,
    )
    .await
    .unwrap();
    assert_eq!(fs::read(path).unwrap(), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn corrupt_pieces_on_disk_are_found_and_fetched_again() {
    let seeder = Seeder::file("unreal.zip").await;
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("game.torrent");
    fs::write(&file, &seeder.torrent).unwrap();
    let source = Source::File(file);
    let root = temp.path().join("Games");
    fs::create_dir(&root).unwrap();
    let sink: Arc<dyn EventSink> = Arc::new(RecordingSink::default());
    let path = downloaded(&engine(&[seeder.peer], sink.clone()), &root, "job", &source)
        .await
        .unwrap();
    let original = fs::read(fixture("unreal.zip")).unwrap();
    let mut damaged = original.clone();
    for byte in &mut damaged[40_000..40_100] {
        *byte = !*byte;
    }
    fs::write(&path, &damaged).unwrap();
    let path = downloaded(&engine(&[seeder.peer], sink), &root, "job", &source)
        .await
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), original);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_peers_the_job_fails_with_an_explanation_instead_of_hanging() {
    let seeder = Seeder::file("unity.zip").await;
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("game.torrent");
    fs::write(&file, &seeder.torrent).unwrap();
    let root = temp.path().join("Games");
    fs::create_dir(&root).unwrap();
    let mut config = config(&[]);
    config.stall_timeout = Duration::from_secs(1);
    config.metadata_timeout = Duration::from_secs(5);
    let sink: Arc<dyn EventSink> = Arc::new(RecordingSink::default());
    let engine = TorrentEngine::new(
        config.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink.clone(),
    );
    let error = downloaded(&engine, &root, "file", &Source::File(file))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            FetchError::NoPeers {
                metadata: false,
                ..
            }
        ),
        "{error:?}"
    );
    let failure = failure::classify(
        &error,
        &Probe {
            stage: FailureStage::Download,
            source: None,
            remote: true,
            root: Some(&root),
        },
    );
    assert_eq!(failure.kind, FailureKind::Download);
    assert!(failure.transient);
    assert!(
        failure.summary.contains("No one sharing this torrent"),
        "{failure:?}"
    );

    // A magnet link whose only peer is gone: no metadata, said so.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = closed.local_addr().unwrap();
    drop(closed);
    config.initial_peers = vec![gone];
    let engine = TorrentEngine::new(config, Arc::new(FakeDiskSpace::fixed(u64::MAX)), sink);
    let magnet = Source::parse(&seeder.magnet).unwrap();
    let error = downloaded(&engine, &root, "magnet", &magnet)
        .await
        .unwrap_err();
    assert!(
        matches!(error, FetchError::NoPeers { metadata: true, .. }),
        "{error:?}"
    );
}

/// A minimal multi-file torrent naming `paths` (piece hashes are zeros:
/// the names are refused before any piece is checked).
fn torrent_with(paths: &[&[&str]]) -> Vec<u8> {
    let mut info = b"d5:filesl".to_vec();
    for parts in paths {
        info.extend_from_slice(b"d6:lengthi1e4:pathl");
        for part in *parts {
            info.extend_from_slice(format!("{}:{part}", part.len()).as_bytes());
        }
        info.extend_from_slice(b"ee");
    }
    info.extend_from_slice(b"e4:name4:evil12:piece lengthi16384e6:pieces20:");
    info.extend_from_slice(&[0; 20]);
    info.push(b'e');
    let mut torrent = b"d4:info".to_vec();
    torrent.extend(info);
    torrent.push(b'e');
    torrent
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsafe_names_in_a_torrent_are_refused_before_anything_is_written() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Games");
    fs::create_dir(&root).unwrap();
    let sink: Arc<dyn EventSink> = Arc::new(RecordingSink::default());
    let engine = engine(&[], sink);
    let cases: [&[&[&str]]; 7] = [
        &[&["..", "escape.exe"]],
        &[&["CON"]],
        &[&["C:"], &["game.exe"]],
        &[&["stream:ads"]],
        &[&["trailing."]],
        &[&["Game.exe"], &["game.EXE"]],
        &[&["a"], &["a", "b"]],
    ];
    for (index, paths) in cases.into_iter().enumerate() {
        let file = temp.path().join(format!("evil-{index}.torrent"));
        fs::write(&file, torrent_with(paths)).unwrap();
        let error = downloaded(&engine, &root, &format!("job-{index}"), &Source::File(file))
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                FetchError::Safety(_)
                    | FetchError::Torrent { .. }
                    | FetchError::InvalidRequest { .. }
            ),
            "{paths:?}: {error:?}"
        );
    }
    // Nothing but empty job folders under the root; nothing beside it.
    let mut written = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else {
                written.push(entry.path());
            }
        }
    }
    assert!(
        written
            .iter()
            .all(|path| path.components().any(|part| part.as_os_str() == ".state")),
        "{written:?}"
    );
    let beside: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| !name.to_string_lossy().ends_with(".torrent") && name != "Games")
        .collect();
    assert!(beside.is_empty(), "{beside:?}");
}

/// Deletes the downloaded file just before the engine checks it, as
/// security software might.
struct Quarantine {
    root: PathBuf,
    name: &'static str,
}

impl EventSink for Quarantine {
    fn emit(&self, event: JobEvent) {
        if event.message == "Checking downloaded files" {
            let mut pending = vec![self.root.join(".caskade/torrents")];
            while let Some(dir) = pending.pop() {
                for entry in fs::read_dir(dir).unwrap().flatten() {
                    if entry.file_type().unwrap().is_dir() {
                        pending.push(entry.path());
                    } else if entry.file_name() == self.name {
                        fs::remove_file(entry.path()).unwrap();
                    }
                }
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_removed_after_downloading_is_reported_missing_not_fetched_again() {
    let seeder = Seeder::file("unity.zip").await;
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("game.torrent");
    fs::write(&file, &seeder.torrent).unwrap();
    let root = temp.path().join("Games");
    fs::create_dir(&root).unwrap();
    let sink = Arc::new(Quarantine {
        root: root.clone(),
        name: "unity.zip",
    });
    let error = downloaded(
        &engine(&[seeder.peer], sink),
        &root,
        "job",
        &Source::File(file),
    )
    .await
    .unwrap_err();
    let failure = failure::classify(
        &error,
        &Probe {
            stage: FailureStage::Download,
            source: None,
            remote: true,
            root: Some(&root),
        },
    );
    assert_eq!(failure.kind, FailureKind::MissingStaged, "{failure:?}");
    assert!(
        !failure.transient,
        "an automatic retry would download it again"
    );
    assert!(failure.actions.contains(&RecoveryAction::Redownload));
}
