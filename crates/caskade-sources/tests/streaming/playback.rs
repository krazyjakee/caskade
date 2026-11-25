use super::{
    RealCaskade, is_host_busy, launch_callers, pair_client, paired_host_id, running_in,
    setup_stream_launch, start_host, stream_entry, visible_windows, wait_until,
};
use caskade_core::{
    AppPaths,
    bus::Bus,
    db::{LibraryStore, StreamApp},
    launch::{LaunchError, LaunchService, SessionStatus},
    managed::ManagedTools,
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId},
    settings::SettingsService,
};
use caskade_streaming::{
    StreamingClient,
    api::App,
    host::process::{ChildRunner, HostLaunch, HostRunner},
    play::Streams,
    publish::Publisher,
    secrets::HostSecrets,
};
use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// #264: Play streams a game from the real host through the launch
/// pipeline. The host checks come first; the client shows only its stream
/// window; ending the stream stops the game on the host and leaves no
/// client; a game the host doesn't offer and a host that's gone fail in
/// Caskade's words, with no client left. Nothing is stored on the client.
/// Run under Xvfb (`xvfb-run -a -s "-screen 0 1280x720x24"`).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client"]
async fn live_linux_streaming_plays_from_the_host() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let tools = ManagedTools::new(
        store.clone(),
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &paths,
    );
    let host = start_host(
        tools.clone(),
        settings.clone(),
        &paths,
        "Caskade streaming test",
        None,
        HostSecrets::memory(),
    )
    .await;
    let api = host.api().await.unwrap();
    // The host's apps run absolute programs, as Caskade's own do.
    api.add_app(&App::command("Live Game", "/bin/sleep 600"))
        .await
        .unwrap();
    let client = StreamingClient::new(tools);
    pair_client(&host, &client).await;
    let (state, host_id) = paired_host_id(&client, "Caskade streaming test").await;

    // The host's game, and one it doesn't offer, as the library has them.
    let apps = [
        StreamApp {
            id: "1".into(),
            name: "Live Game".into(),
        },
        StreamApp {
            id: "2".into(),
            name: "Gone Game".into(),
        },
    ];
    store.sync_stream_apps(&host_id, &apps).await.unwrap();
    let live = stream_entry(&store, "Live Game").await;
    let gone = stream_entry(&store, "Gone Game").await;

    let launch = LaunchService::new(store.clone());
    // Caskade's window fills the screen, as in console mode.
    launch.set_streamer(Arc::new(
        Streams::new(client.clone(), None, settings).with_placement(|| {
            Some(caskade_core::launch::WindowPlacement {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            })
        }),
    ));

    // Windows seen while the stream starts, plays and ends.
    let seen = Arc::new(Mutex::new(Vec::<(Duration, Vec<String>)>::new()));
    let watching = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let start = Instant::now();
    let sampler = std::thread::spawn({
        let (seen, watching) = (seen.clone(), watching.clone());
        move || {
            while watching.load(std::sync::atomic::Ordering::SeqCst) {
                let windows = visible_windows();
                seen.lock().unwrap().push((start.elapsed(), windows));
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });

    let handle = launch.launch(&live.id).await.unwrap();
    let streaming_at = start.elapsed();
    println!("streaming after {streaming_at:?}");
    assert!(is_host_busy().await, "the host plays the game");
    assert_eq!(launch.running_streams(None).len(), 1);
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!running_in(&state).is_empty(), "the client streams");
    let streaming = visible_windows();
    println!("windows while streaming: {streaming:?}");
    assert!(
        streaming
            .iter()
            .any(|name| name.starts_with("Caskade streaming test - Moonlight")),
        "the stream shows: {streaming:?}"
    );
    // End stream: the host's game stops and no client is left.
    assert_eq!(
        launch
            .stop(&handle.ticket.session_id, Duration::from_secs(5))
            .await
            .unwrap(),
        1
    );
    let (_, status) = handle.wait_status().await.unwrap();
    assert_eq!(status, SessionStatus::Exited);
    let deadline = Instant::now() + Duration::from_secs(10);
    while is_host_busy().await && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(!is_host_busy().await, "the game stopped on the host");
    assert!(running_in(&state).is_empty(), "no client is left");
    tokio::time::sleep(Duration::from_secs(1)).await;
    watching.store(false, std::sync::atomic::Ordering::SeqCst);
    sampler.join().unwrap();
    let seen = seen.lock().unwrap().clone();
    let windows: std::collections::BTreeSet<&String> =
        seen.iter().flat_map(|(_, names)| names).collect();
    println!("windows seen: {windows:?}");

    // Only the stream's own window shows: the client's start window is
    // one pixel (under Caskade's), and none is left after.
    assert!(
        windows.iter().all(|name| {
            name.starts_with("Caskade streaming test - Moonlight") || name.ends_with(" 1x1")
        }),
        "{windows:?}"
    );
    assert!(seen.last().unwrap().1.is_empty(), "{:?}", seen.last());
    assert!(
        store
            .sessions_for_game(&live.game_id, 10)
            .await
            .unwrap()
            .is_empty(),
        "playtime is the host's"
    );

    // A game the host doesn't offer.
    let Err(LaunchError::Refused(text)) = launch.launch(&gone.id).await else {
        panic!("a game the host doesn't offer can't stream");
    };
    assert_eq!(text, "Caskade streaming test no longer offers Gone Game.");
    assert!(running_in(&state).is_empty());

    // The host forgot this device.
    let device = host
        .paired_devices()
        .await
        .unwrap()
        .into_iter()
        .find(|device| device.name == "Living room")
        .unwrap();
    host.unpair(&device.uuid).await.unwrap();
    let Err(LaunchError::Refused(text)) = launch.launch(&live.id).await else {
        panic!("a host that forgot this device can't stream");
    };
    assert_eq!(
        text,
        "Caskade streaming test no longer knows this device. Pair them again in Settings → Streaming."
    );
    assert!(running_in(&state).is_empty());

    // The host gone.
    host.shutdown();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let Err(LaunchError::Refused(text)) = launch.launch(&live.id).await else {
        panic!("a host that's gone can't stream");
    };
    println!("host gone: {text}");
    assert!(text.contains("isn't answering"), "{text}");
    assert!(running_in(&state).is_empty());
}

/// #259 and #264 joined up: the real client starts a game the host
/// published, and everything between them is real: Moonlight, Sunshine, the
/// published `caskade --launch` and a running Caskade over the host's
/// library on disk. The game gets a session on the host, Sunshine's
/// description of the client shows it came through that path, and ending
/// the stream stops it, keeping its playtime on the host and none on the
/// client. Needs a built `caskade-native` (`cargo build -p caskade-native`, or
/// `CASKADE_LIVE_PROGRAM`). Its folders are the test's, so it can't reach
/// the user's own Caskade. Run under Xvfb
/// (`xvfb-run -a -s "-screen 0 1280x720x24"`).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads and runs the real streaming host and client, with a built caskade"]
async fn live_linux_published_game_plays_and_stops_with_the_stream() {
    /// The host, and so the `caskade --launch` it starts, in the test's
    /// folders.
    struct InFolders {
        runner: ChildRunner,
        env: Vec<(String, PathBuf)>,
    }
    impl HostRunner for InFolders {
        fn start(&self, launch: &HostLaunch) -> caskade_core::Result<()> {
            let mut launch = launch.clone();
            launch.env.extend(self.env.iter().cloned());
            self.runner.start(&launch)
        }
        fn stop(&self) -> caskade_core::Result<()> {
            self.runner.stop()
        }
        fn running(&self) -> bool {
            self.runner.running()
        }
    }

    let program = std::env::var_os("CASKADE_LIVE_PROGRAM").map_or_else(
        || {
            let test = std::env::current_exe().unwrap();
            test.parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("caskade-native")
        },
        PathBuf::from,
    );
    assert!(
        program.is_file(),
        "no {}: run `cargo build -p caskade-native`",
        program.display()
    );

    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let home = temp.path().join("home");
    // HOME first: the logs go there. The runtime folder keeps the host off
    // the desktop's sound server, whose default output it would change.
    let folders: Vec<(String, PathBuf)> = [
        ("HOME", home.clone()),
        ("XDG_CONFIG_HOME", home.join(".config")),
        ("XDG_DATA_HOME", home.join(".local/share")),
        ("XDG_CACHE_HOME", home.join(".cache")),
        ("XDG_RUNTIME_DIR", home.join("run")),
    ]
    .map(|(key, value)| (key.to_owned(), value))
    .to_vec();
    for (_, folder) in &folders {
        std::fs::create_dir_all(folder).unwrap();
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    // The host's library: one game, which says it started and then plays.
    let data = home.join(".local/share/caskade");
    std::fs::create_dir_all(&data).unwrap();
    let library = LibraryStore::open(&data.join("library.sqlite"))
        .await
        .unwrap();
    let folder = home.join("Published Game");
    std::fs::create_dir_all(&folder).unwrap();
    let script = folder.join("game.sh");
    caskade_core::test_programs::write_program(
        &script,
        "#!/bin/sh\necho \"$$ ${SUNSHINE_CLIENT_WIDTH:-none}\" > started.part\n\
         mv started.part started\nexec /bin/sleep 600\n",
    );
    let game = Game::new("Published Game");
    library.upsert_game(&game).await.unwrap();
    let mut release = GameEntry::new(&game.id, SourceId::new("fixture"), "Published Game");
    release.state = InstallState::Installed;
    release.location = Some(InstallLocation::External {
        path: folder.clone(),
    });
    release.launch = Some(LaunchSpec::Executable {
        path: script,
        args: Vec::new(),
        cwd: Some(folder.clone()),
        env: Default::default(),
    });
    library.upsert_entry(&release).await.unwrap();

    // The real Caskade, answering on the launch channel.
    let _caskade = RealCaskade::start(&program, &folders);
    let channel = home.join(".cache/caskade/run/launch.sock");
    wait_until("Caskade takes launches", async || channel.exists()).await;

    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let tools = ManagedTools::new(
        store.clone(),
        Bus::default(),
        caskade_sources::tools::catalog().unwrap(),
        &paths,
    );
    let host = start_host(
        tools.clone(),
        settings.clone(),
        &paths,
        "Caskade published test",
        Some(Arc::new(InFolders {
            runner: ChildRunner::new(paths.data.join("streaming/run.json")),
            env: folders.clone(),
        })),
        HostSecrets::memory(),
    )
    .await;

    // Published as the app publishes it.
    Publisher::new(library.clone(), Arc::new(host.clone()), None, None, &paths)
        .unwrap()
        .with(program.clone(), Duration::ZERO)
        .publish()
        .await
        .unwrap()
        .unwrap();
    let apps = host.api().await.unwrap().apps().await.unwrap();
    let app = apps.iter().find(|app| app.name == game.title).unwrap();
    assert_eq!(
        app.cmd.as_deref(),
        Some(format!("\"{}\" --launch {}", program.display(), game.id).as_str())
    );

    let client = StreamingClient::new(tools);
    pair_client(&host, &client).await;
    let (state, host_id) = paired_host_id(&client, "Caskade published test").await;
    let (entry, launch) =
        setup_stream_launch(&store, &client, settings, &host_id, &game.title).await;

    // The client starts it: a session on the host, held by the caller.
    let handle = launch.launch(&entry.id).await.unwrap();
    let started = folder.join("started");
    wait_until("the game starts on the host", async || started.exists()).await;
    let line = std::fs::read_to_string(&started).unwrap();
    println!("the game says (pid, client width): {line}");
    let (pid, width) = line.trim().split_once(' ').unwrap();
    let width: u32 = width.parse().expect("Sunshine's client width reaches it");
    assert!(width > 0);
    let sessions = library.sessions_for_game(&game.id, 10).await.unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0].ended_at_ms, None);
    assert_eq!(launch_callers(&game.id).len(), 1);
    assert!(is_host_busy().await, "the host plays the game");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Ending the stream ends everything on the host.
    assert_eq!(
        launch
            .stop(&handle.ticket.session_id, Duration::from_secs(5))
            .await
            .unwrap(),
        1
    );
    let (_, status) = handle.wait_status().await.unwrap();
    assert_eq!(status, SessionStatus::Exited);
    wait_until("the host's session ends", async || {
        let sessions = library.sessions_for_game(&game.id, 10).await.unwrap();
        sessions[0].ended_at_ms.is_some()
    })
    .await;
    let process = PathBuf::from(format!("/proc/{pid}"));
    wait_until("the game stops", async || !process.exists()).await;
    wait_until("the caller ends", async || {
        launch_callers(&game.id).is_empty()
    })
    .await;
    wait_until("the host goes idle", async || !is_host_busy().await).await;

    // Playtime is the host's alone.
    let sessions = library.sessions_for_game(&game.id, 10).await.unwrap();
    println!("the host's session: {sessions:?}");
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert!(sessions[0].duration_ms >= 5_000, "{sessions:?}");
    assert!(
        store
            .sessions_for_game(&entry.game_id, 10)
            .await
            .unwrap()
            .is_empty(),
        "playtime is the host's"
    );
    assert!(running_in(&state).is_empty(), "no client is left");
    host.shutdown();
}
