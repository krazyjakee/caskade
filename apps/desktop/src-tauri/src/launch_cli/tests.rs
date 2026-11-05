//! `caskade --launch` on Tauri's mock runtime (#256): forwarding to the
//! running app (or starting it), the play gate, and the caller living as
//! long as the session.
use super::*;
use caskade_core::{
    accounts::{
        AccountAccess, AccountRole, AccountService, DEFAULT_AVATAR, NewAccount, Permissions,
    },
    bus::Bus,
    db::LibraryStore,
    igdb::IgdbAgeRating,
    launch::{LaunchConfig, LaunchService},
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

const ADMIN_PIN: &str = "2468";

/// The test games as `sh` scripts.
#[cfg(unix)]
mod game {
    use std::{path::Path, process::Command};

    pub const FILE: &str = "game.sh";
    pub const HEADER: &str = "#!/bin/sh\n";
    pub const CRASHES_LATER: &str = "sleep 0.3; exit 7";

    /// Writes the client's width and HDR, and a variable that must not
    /// arrive, to `out`.
    pub fn writes_env(out: &Path) -> String {
        format!(
            "printf '%s|%s|%s' \"$SUNSHINE_CLIENT_WIDTH\" \"$SUNSHINE_CLIENT_HDR\" \"$SUNSHINE_CLIENT_SECRET\" > '{}'",
            out.display()
        )
    }

    pub fn failing_program() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 3"]);
        command
    }
}

/// The test games as batch files. `ping` waits: `timeout` needs a console.
#[cfg(windows)]
mod game {
    use std::{path::Path, process::Command};

    pub const FILE: &str = "game.cmd";
    pub const HEADER: &str = "@echo off\r\n";
    pub const CRASHES_LATER: &str = "ping -n 2 127.0.0.1 > nul\r\nexit 7";

    /// Writes the client's width and HDR, and a variable that must not
    /// arrive, to `out`.
    pub fn writes_env(out: &Path) -> String {
        format!(
            "echo %SUNSHINE_CLIENT_WIDTH%^|%SUNSHINE_CLIENT_HDR%^|%SUNSHINE_CLIENT_SECRET%> \"{}\"",
            out.display()
        )
    }

    pub fn failing_program() -> Command {
        let mut command = Command::new("cmd");
        command.args(["/c", "exit 3"]);
        command
    }
}

struct Harness {
    app: tauri::App<MockRuntime>,
    store: LibraryStore,
    accounts: AccountService,
    launches: LaunchService,
    endpoint: Endpoint,
    temp: tempfile::TempDir,
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tauri::async_runtime::block_on(future)
}

/// An app with the library, accounts (a lone admin, signed in) and fast
/// launches managed, not yet listening.
fn harness() -> Harness {
    let temp = tempfile::tempdir().unwrap();
    let store = block_on(LibraryStore::in_memory()).unwrap();
    let bus = Bus::default();
    let accounts = block_on(AccountService::open(store.clone(), bus.clone())).unwrap();
    let launches = LaunchService::new(store.clone())
        .with_bus(bus)
        .with_aliases(None)
        .with_config(LaunchConfig {
            poll_interval: Duration::from_millis(10),
            exit_grace: Duration::from_millis(20),
            ..Default::default()
        })
        .with_play_gate(Arc::new(accounts.clone()));
    let app = mock_builder()
        .manage(store.clone())
        .manage(accounts.clone())
        .manage(launches.clone())
        .build(mock_context(noop_assets()))
        .unwrap();
    let endpoint = Endpoint::at(temp.path().join("run").join("launch.sock"));
    Harness {
        app,
        store,
        accounts,
        launches,
        endpoint,
        temp,
    }
}

impl Harness {
    fn listen(&self) {
        server::listen(self.app.handle(), self.endpoint.clone());
        // Bound once a connection gets through.
        let deadline = Instant::now() + Duration::from_secs(5);
        while block_on(channel::connect(&self.endpoint)).is_err() {
            assert!(Instant::now() < deadline, "never listened");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// An installed game running `script` ([`game`]), in its own folder.
    fn game(&self, title: &str, script: &str) -> (Game, GameEntry) {
        let folder = self.folder(title);
        let program = folder.join(game::FILE);
        std::fs::write(&program, format!("{}{script}\n", game::HEADER)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        self.install(title, folder, program, Vec::new())
    }

    /// A game of one process that runs for 30 s, so a test that stops it
    /// isn't about which of a game's processes Stop knows yet.
    #[cfg(unix)]
    fn long_game(&self, title: &str) -> (Game, GameEntry) {
        self.game(title, "exec sleep 30")
    }

    /// A game of one process that runs for 30 s, so a test that stops it
    /// isn't about which of a game's processes Stop knows yet. A batch file
    /// can't `exec`: its wait would be a child.
    #[cfg(windows)]
    fn long_game(&self, title: &str) -> (Game, GameEntry) {
        let folder = self.folder(title);
        let program = folder.join("ping.exe");
        let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        std::fs::copy(system.join("System32").join("PING.EXE"), &program).unwrap();
        let args = ["-n", "31", "127.0.0.1"].map(str::to_owned).to_vec();
        self.install(title, folder, program, args)
    }

    fn folder(&self, title: &str) -> std::path::PathBuf {
        let folder = self.temp.path().join(title.replace(' ', "-"));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn install(
        &self,
        title: &str,
        folder: std::path::PathBuf,
        program: std::path::PathBuf,
        args: Vec<String>,
    ) -> (Game, GameEntry) {
        block_on(async {
            let game = Game::new(title);
            self.store.upsert_game(&game).await.unwrap();
            let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), title);
            entry.state = InstallState::Installed;
            entry.location = Some(InstallLocation::External {
                path: folder.clone(),
            });
            entry.launch = Some(LaunchSpec::Executable {
                path: program,
                args,
                cwd: Some(folder),
                env: BTreeMap::new(),
            });
            self.store.upsert_entry(&entry).await.unwrap();
            (game, entry)
        })
    }

    fn request(&self, game: &Game) -> Request {
        Request {
            game_id: game.id.clone(),
            ..Default::default()
        }
    }

    /// The caller, against a Caskade that must already be running.
    fn call(&self, request: &Request) -> Outcome {
        block_on(run(
            &self.endpoint,
            request,
            || panic!("Caskade was running"),
            Duration::from_secs(5),
        ))
    }

    fn sessions(&self, game: &Game) -> Vec<caskade_core::model::PlaySession> {
        block_on(self.store.sessions_for_game(&game.id, 10)).unwrap()
    }

    /// Waits for `game`'s session to be running with known processes.
    fn running(&self, game: &Game) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let running = block_on(self.launches.running_sessions()).unwrap();
            if let Some(ticket) = running.iter().find(|ticket| ticket.game_id == game.id) {
                let session = self
                    .sessions(game)
                    .into_iter()
                    .find(|session| session.id == ticket.session_id);
                let known = session
                    .and_then(|session| session.tracking)
                    .and_then(|tracking| {
                        serde_json::from_value::<caskade_core::sessions::RecoveryState>(tracking)
                            .ok()
                    })
                    .is_some_and(|state| !state.processes.is_empty());
                if known {
                    return ticket.session_id.clone();
                }
            }
            assert!(Instant::now() < deadline, "the game never ran");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn ended(&self, game: &Game) -> bool {
        self.sessions(game)
            .first()
            .is_some_and(|session| session.ended_at_ms.is_some())
    }

    /// A standard account limited to 12, signed in.
    fn kid(&self) -> String {
        block_on(async {
            let admin = self.accounts.active_id().unwrap();
            self.accounts
                .set_pin(&admin, Some(ADMIN_PIN.into()))
                .await
                .unwrap();
            let kid = self
                .accounts
                .create(NewAccount {
                    name: "Kid".into(),
                    avatar: DEFAULT_AVATAR.into(),
                    access: AccountAccess {
                        role: AccountRole::Standard,
                        permissions: Permissions::default(),
                        max_age: Some(12),
                        unrated_needs_pin: false,
                    },
                    pin: None,
                })
                .await
                .unwrap()
                .id;
            self.accounts.sign_in(&kid, None).await.unwrap();
            kid
        })
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_launch_is_forwarded_plays_through_the_pipeline_and_ends_with_the_game() {
    let h = harness();
    h.listen();
    let out = h.temp.path().join("seen");
    let (game, _) = h.game("Short Game", &game::writes_env(&out));
    let mut request = h.request(&game);
    request.env = BTreeMap::from([
        ("SUNSHINE_CLIENT_WIDTH".into(), "1920".into()),
        ("SUNSHINE_CLIENT_HDR".into(), "true".into()),
        // Neither an unlisted variable nor an implausible value crosses.
        ("SUNSHINE_CLIENT_SECRET".into(), "x".into()),
    ]);
    let outcome = h.call(&request);
    assert_eq!(
        outcome,
        Outcome {
            code: code::EXITED,
            message: None
        }
    );
    assert_eq!(
        std::fs::read_to_string(&out).unwrap().trim_end(),
        "1920|true|"
    );
    let sessions = h.sessions(&game);
    assert_eq!(sessions.len(), 1, "one recorded session");
    assert!(sessions[0].ended_at_ms.is_some());
}

#[test]
fn caskade_is_started_when_it_isnt_running() {
    let h = harness();
    let (game, _) = h.game("Started Game", "exit 0");
    let starts = AtomicUsize::new(0);
    let outcome = block_on(run(
        &h.endpoint,
        &h.request(&game),
        || {
            starts.fetch_add(1, Ordering::SeqCst);
            // What the started Caskade does once its library is open.
            server::listen(h.app.handle(), h.endpoint.clone());
            Ok(None)
        },
        Duration::from_secs(10),
    ));
    assert_eq!(outcome.code, code::EXITED, "{outcome:?}");
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(h.sessions(&game).len(), 1);
    // Running now, so the next caller starts nothing.
    assert_eq!(h.call(&h.request(&game)).code, code::EXITED);
}

#[test]
fn a_caskade_that_wont_start_fails_the_caller() {
    let h = harness();
    let (game, _) = h.game("Lonely Game", "exit 0");
    let outcome = block_on(run(
        &h.endpoint,
        &h.request(&game),
        || game::failing_program().spawn().map(Some),
        Duration::from_secs(10),
    ));
    assert_eq!(outcome.code, code::FAILED);
    assert!(h.sessions(&game).is_empty());
}

#[test]
fn the_caller_lives_as_long_as_the_session_and_a_crash_says_so() {
    let h = harness();
    h.listen();
    let (game, _) = h.game("Crashing Game", game::CRASHES_LATER);
    let started = Instant::now();
    let outcome = h.call(&h.request(&game));
    assert_eq!(outcome.code, code::CRASHED);
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert!(h.ended(&game));
}

#[test]
fn ending_the_caller_stops_the_game() {
    let h = harness();
    h.listen();
    let (game, _) = h.long_game("Long Game");
    let caller = std::thread::spawn({
        let endpoint = h.endpoint.clone();
        let request = h.request(&game);
        move || {
            block_on(async {
                let stream = channel::connect(&endpoint).await.unwrap();
                let (read, mut write) = tokio::io::split(stream);
                let mut line = serde_json::to_vec(&request).unwrap();
                line.push(b'\n');
                write.write_all(&line).await.unwrap();
                let mut lines = BufReader::new(read).lines();
                let started: Reply =
                    serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
                assert!(matches!(started, Reply::Started { .. }), "{started:?}");
                // The caller is ended (a signal, WM_CLOSE): its end closes.
                (lines, write)
            })
        }
    });
    let session = h.running(&game);
    let (lines, write) = caller.join().unwrap();
    assert!(!h.ended(&game));
    drop((lines, write));
    wait_until("the game was never stopped", || h.ended(&game));
    assert!(block_on(h.launches.running_sessions()).unwrap().is_empty());
    // The server takes its own stop's record, once the session has ended.
    wait_until("the server took its stop's record", || {
        !h.launches.is_stopped(&session)
    });
    assert!(!h.launches.take_stopped(&session));
}

#[test]
fn caskades_stop_ends_the_caller_as_stopped() {
    let h = harness();
    h.listen();
    let (game, _) = h.long_game("Stopped Game");
    let caller = std::thread::spawn({
        let endpoint = h.endpoint.clone();
        let request = h.request(&game);
        move || {
            block_on(run(
                &endpoint,
                &request,
                || panic!("Caskade was running"),
                Duration::from_secs(5),
            ))
        }
    });
    let session = h.running(&game);
    block_on(h.launches.stop(&session, Duration::from_secs(2))).unwrap();
    assert_eq!(caller.join().unwrap().code, code::STOPPED);
}

#[test]
fn an_age_restricted_game_is_refused_without_asking_or_a_session() {
    let h = harness();
    h.listen();
    let (game, _) = h.game("Grown-up Game", "exit 0");
    block_on(h.store.set_game_age_ratings(
        &game.id,
        &[IgdbAgeRating {
            board: "pegi".into(),
            rating: "18".into(),
        }],
    ))
    .unwrap();
    let kid = h.kid();
    let outcome = h.call(&h.request(&game));
    assert_eq!(outcome.code, code::REFUSED);
    let message = outcome.message.unwrap();
    assert!(
        message.contains("admin PIN") && message.contains("Grown-up Game"),
        "{message}"
    );
    assert!(h.sessions(&game).is_empty(), "nothing started");
    // An admin's permanent approval lets it through, as for Play.
    block_on(
        h.accounts
            .approve_play_for(&kid, &game.id, ADMIN_PIN.into(), true),
    )
    .unwrap();
    assert_eq!(h.call(&h.request(&game)).code, code::EXITED);
}

#[test]
fn nobody_signed_in_unknown_games_and_releases_are_refused() {
    let h = harness();
    h.listen();
    let (game, _) = h.game("Some Game", "exit 0");
    let refused = |request: &Request| {
        let outcome = h.call(request);
        assert_eq!(outcome.code, code::REFUSED, "{outcome:?}");
        outcome.message.unwrap()
    };
    let mut other = h.request(&game);
    other.entry_id = Some("not-a-release".into());
    assert!(refused(&other).contains("Some Game"));
    refused(&Request {
        game_id: "not-a-game".into(),
        ..Default::default()
    });
    h.accounts.sign_out();
    refused(&h.request(&game));
    assert!(h.sessions(&game).is_empty());
}

#[test]
fn a_game_already_running_is_refused() {
    let h = harness();
    h.listen();
    let (game, _) = h.long_game("Busy Game");
    let first = std::thread::spawn({
        let endpoint = h.endpoint.clone();
        let request = h.request(&game);
        move || {
            block_on(run(
                &endpoint,
                &request,
                || panic!("Caskade was running"),
                Duration::from_secs(5),
            ))
        }
    });
    let session = h.running(&game);
    assert_eq!(h.call(&h.request(&game)).code, code::REFUSED);
    block_on(h.launches.stop(&session, Duration::from_secs(2))).unwrap();
    assert_eq!(first.join().unwrap().code, code::STOPPED);
}

#[test]
fn arguments_name_the_game_its_release_and_action() {
    let args = |line: &str| from_args(line.split(' ').map(str::to_owned));
    assert_eq!(args("caskade"), None);
    assert_eq!(args("caskade --console"), None);
    let request = args("caskade --launch g1 --entry e1 --action a1")
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            request.game_id.as_str(),
            request.entry_id.as_deref(),
            request.action_id.as_deref()
        ),
        ("g1", Some("e1"), Some("a1"))
    );
    for wrong in [
        "caskade --launch",
        "caskade --launch --entry e1",
        "caskade --launch g1 --entry",
        "caskade --launch g1 --entry e1 --entry e2",
        "caskade --launch g1 --other x",
    ] {
        assert!(args(wrong).unwrap().is_err(), "{wrong}");
    }
}

#[test]
fn only_the_clients_display_reaches_the_game() {
    let vars = |pairs: &[(&str, &str)]| {
        client_env(
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
        )
    };
    assert_eq!(
        vars(&[
            ("SUNSHINE_CLIENT_WIDTH", "2560"),
            ("SUNSHINE_CLIENT_HEIGHT", "1440"),
            ("SUNSHINE_CLIENT_FPS", "120"),
            ("SUNSHINE_CLIENT_HDR", "false"),
        ])
        .len(),
        4
    );
    assert!(
        vars(&[
            ("SUNSHINE_CLIENT_WIDTH", "-1"),
            ("SUNSHINE_CLIENT_FPS", "60; rm"),
            ("SUNSHINE_CLIENT_HDR", "yes"),
            ("LD_PRELOAD", "/tmp/x.so"),
            ("PATH", "/tmp"),
        ])
        .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn the_socket_is_the_users_alone() {
    use std::{os::unix::fs::PermissionsExt, path::Path};
    let h = harness();
    h.listen();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let socket = h.temp.path().join("run").join("launch.sock");
    assert_eq!(mode(socket.parent().unwrap()), 0o700);
    assert_eq!(mode(&socket), 0o600);
    // A second Caskade doesn't take over the first one's socket.
    assert_eq!(
        block_on(channel::Listener::bind(&h.endpoint))
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::AddrInUse)
    );
}

#[cfg(windows)]
#[test]
fn a_second_caskade_doesnt_take_over_the_pipe() {
    let h = harness();
    h.listen();
    assert!(block_on(channel::Listener::bind(&h.endpoint)).is_err());
}
