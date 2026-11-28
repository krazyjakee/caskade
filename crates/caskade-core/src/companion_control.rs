//! Companion remote control over `caskade/sync/1` (`Purpose::Control`).
//!
//! A paired companion app (iOS/Android `caskade-app`) opens a `control`
//! session presenting its paired grant. Only paired devices are admitted by
//! `Devices`'s gate; unauthorised peers get the same refusal as any other
//! purpose.
//!
//! Once admitted, the session stays open as a bidirectional line-framed JSON
//! stream:
//! - The host immediately sends an [`InstanceState`] snapshot and pushes fresh
//!   snapshots whenever the running game, active account, or console mode
//!   changes on the bus.
//! - The companion sends [`CompanionCommand`] frames to list library summaries,
//!   toggle favourites, start or stop games, switch console mode, send
//!   normalised pad actions, navigate top-level views, or request a sync.
//! - Mutating control commands honour `core.sync.remotePlay` and the signed-in
//!   account, and launches go through [`RemotePlay::request_local`] so age
//!   limits, admin PINs, and save checks still run on the host.

use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use caskade_sync::control::{
    CompanionCommand, CompanionEvent, CompanionGame, CompanionGameStatus, InstanceState,
    MAX_COMMAND_BYTES, MAX_GAMES_PAGE, is_valid_pad_action, normalize_route,
};
use caskade_sync::{Session, Sessions};

use crate::accounts::AccountService;
use crate::bus::{Bus, Event as BusEvent};
use crate::db::LibraryStore;
use crate::devices::Devices;
use crate::launch::LaunchService;
use crate::model::{GameFilter, GameSort, ListGames};
use crate::remote_play::{PlayOutcome, RemotePlay};
use crate::settings::{ConsoleSettings, SettingsService};
use crate::sync::policy::SyncSettings;

/// How long a running game gets to exit cleanly before forced stop.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Maximum character count included for a game's summary in a companion page.
const MAX_DESCRIPTION_CHARS: usize = 240;

/// Shell-level action requested by a connected companion device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlAction {
    /// Switch the window between desktop and fullscreen console mode.
    SetConsoleMode { enabled: bool },
    /// Inject a validated `caskade-input` wire action (`nav.up`, `confirm`, …).
    PadAction { action: String },
    /// Navigate the window to a validated top-level route (`/`, `/library`, …).
    OpenView { route: String },
    /// Request an immediate library sync with `device_id`.
    SyncNow { device_id: String },
}

type ActionHandler = Arc<dyn Fn(ControlAction) + Send + Sync>;
type ConsoleReader = Arc<dyn Fn() -> bool + Send + Sync>;

/// Companion control sessions over `devices`' endpoint. Cheap to clone.
#[derive(Clone)]
pub struct CompanionControl {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    devices: Devices,
    accounts: AccountService,
    launch: LaunchService,
    remote_play: RemotePlay,
    bus: Bus,
    settings: OnceLock<SettingsService>,
    handler: OnceLock<ActionHandler>,
    console_state: OnceLock<ConsoleReader>,
}

impl CompanionControl {
    /// Registers a `Purpose::Control` session handler on `devices`.
    pub fn new(
        store: LibraryStore,
        devices: Devices,
        accounts: AccountService,
        launch: LaunchService,
        remote_play: RemotePlay,
        bus: Bus,
    ) -> Self {
        let service = Self {
            inner: Arc::new(Inner {
                store,
                devices: devices.clone(),
                accounts,
                launch,
                remote_play,
                bus,
                settings: OnceLock::new(),
                handler: OnceLock::new(),
                console_state: OnceLock::new(),
            }),
        };
        devices.set_control_handler(Arc::new(Serve(Arc::downgrade(&service.inner))));
        service
    }

    /// Reads `core.sync.remotePlay` and `core.console` from `settings`.
    pub fn set_settings(&self, settings: SettingsService) {
        let _ = self.inner.settings.set(settings);
    }

    /// Supplies live console-mode state from the shell; falls back to
    /// `core.console.active` when unset.
    pub fn set_console_reader(&self, reader: impl Fn() -> bool + Send + Sync + 'static) {
        let _ = self.inner.console_state.set(Arc::new(reader));
    }

    /// Receives shell-level actions (`SetConsoleMode`, `PadAction`,
    /// `OpenView`, `SyncNow`).
    pub fn on_action(&self, handler: impl Fn(ControlAction) + Send + Sync + 'static) {
        let _ = self.inner.handler.set(Arc::new(handler));
    }

    async fn remote_allowed(&self) -> bool {
        SyncSettings::read(self.inner.settings.get())
            .await
            .remote_play
    }

    async fn snapshot_state(&self) -> InstanceState {
        let device_name = self.inner.devices.status().await.this_device.name;
        let active_account = self
            .inner
            .accounts
            .active()
            .await
            .ok()
            .flatten()
            .map(|account| account.name);
        let console_mode = if let Some(reader) = self.inner.console_state.get() {
            reader()
        } else if let Some(settings) = self.inner.settings.get() {
            settings
                .read_or_default::<ConsoleSettings>("core.console")
                .await
                .active
        } else {
            false
        };
        let running = self
            .inner
            .launch
            .running_sessions()
            .await
            .ok()
            .and_then(|mut sessions| {
                if sessions.is_empty() {
                    None
                } else {
                    Some(sessions.remove(0))
                }
            });
        let (running_game_id, running_game_title, running_session_id) = match running {
            Some(ticket) => {
                let title = self
                    .inner
                    .store
                    .get_game(&ticket.game_id)
                    .await
                    .ok()
                    .flatten()
                    .map(|details| details.game.title);
                (Some(ticket.game_id), title, Some(ticket.session_id))
            }
            None => (None, None, None),
        };
        InstanceState {
            device_name,
            active_account,
            console_mode,
            running_game_id,
            running_game_title,
            running_session_id,
        }
    }

    async fn list_games(&self, offset: u32, limit: u32) -> CompanionEvent {
        let bounded_limit = if limit == 0 {
            50
        } else {
            limit.min(MAX_GAMES_PAGE)
        };
        let query = ListGames {
            query: None,
            filter: GameFilter::All,
            local_only: false,
            platform: None,
            sort: GameSort::Recent,
            offset,
            limit: bounded_limit,
        };
        let active = self.inner.accounts.active_id();
        match self
            .inner
            .store
            .list_games_for_account(&query, active.as_deref())
            .await
        {
            Ok(page) => {
                let games = page
                    .items
                    .into_iter()
                    .map(|summary| {
                        let platform = summary
                            .platforms
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "PC".to_owned());
                        let status = if summary.installed {
                            CompanionGameStatus::Installed
                        } else {
                            CompanionGameStatus::Available
                        };
                        let description =
                            truncate_chars(&summary.description, MAX_DESCRIPTION_CHARS);
                        CompanionGame {
                            id: summary.id,
                            title: summary.title,
                            platform,
                            source: String::new(),
                            status,
                            playtime_ms: summary.playtime_ms,
                            favorite: summary.favorite,
                            last_played_at_ms: summary.last_played_at_ms,
                            release_date: summary.release_date,
                            genres: summary.genres,
                            description,
                        }
                    })
                    .collect();
                CompanionEvent::Games {
                    games,
                    total: page.total,
                }
            }
            Err(_) => CompanionEvent::CommandResult {
                ok: false,
                message: Some(crate::tr!("couldn't read the library")),
            },
        }
    }

    async fn handle_command(&self, peer: &str, command: CompanionCommand) -> CompanionEvent {
        match command {
            CompanionCommand::GetState => CompanionEvent::State(self.snapshot_state().await),
            CompanionCommand::ListGames { offset, limit } => self.list_games(offset, limit).await,
            CompanionCommand::SyncNow => {
                if let Some(handler) = self.inner.handler.get() {
                    handler(ControlAction::SyncNow {
                        device_id: peer.to_owned(),
                    });
                    CompanionEvent::CommandResult {
                        ok: true,
                        message: None,
                    }
                } else {
                    CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!("sync isn't ready on this device yet")),
                    }
                }
            }
            CompanionCommand::ToggleFavourite { game_id } => {
                if let Some(reason) = self.check_control_and_account().await {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(reason),
                    };
                }
                let Ok(Some(details)) = self.inner.store.get_game(&game_id).await else {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!("that game isn't in this device's library")),
                    };
                };
                let next = !details.game.favorite;
                match self.inner.store.set_game_favorite(&game_id, next).await {
                    Ok(_) => {
                        self.inner
                            .bus
                            .emit(BusEvent::LibraryGameUpdated { game_id });
                        CompanionEvent::CommandResult {
                            ok: true,
                            message: None,
                        }
                    }
                    Err(_) => CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!("couldn't update that game")),
                    },
                }
            }
            CompanionCommand::LaunchGame { game_id } => {
                if let Some(reason) = self.check_control_and_account().await {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(reason),
                    };
                }
                let local_id = match self.inner.store.get_game(&game_id).await {
                    Ok(Some(_)) => Some(game_id),
                    _ => self
                        .inner
                        .store
                        .sync_mapped_game(peer, &game_id)
                        .await
                        .ok()
                        .flatten(),
                };
                let Some(local_id) = local_id else {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!("that game isn't in this device's library")),
                    };
                };
                match self.inner.remote_play.request_local(peer, &local_id).await {
                    PlayOutcome::Started => CompanionEvent::CommandResult {
                        ok: true,
                        message: None,
                    },
                    PlayOutcome::NotStarted(reason) => CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(reason),
                    },
                }
            }
            CompanionCommand::StopGame { game_id } => {
                if let Some(reason) = self.check_control_and_account().await {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(reason),
                    };
                }
                let running = self
                    .inner
                    .launch
                    .running_sessions()
                    .await
                    .unwrap_or_default();
                let target = match game_id.as_deref() {
                    Some(id) => running.into_iter().find(|ticket| ticket.game_id == id),
                    None => running.into_iter().next(),
                };
                let Some(ticket) = target else {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!("no matching game is running")),
                    };
                };
                match self.inner.launch.stop(&ticket.session_id, STOP_GRACE).await {
                    Ok(_) => CompanionEvent::CommandResult {
                        ok: true,
                        message: None,
                    },
                    Err(_) => CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!("couldn't stop the running game")),
                    },
                }
            }
            CompanionCommand::SetConsoleMode { enabled } => {
                if !self.remote_allowed().await {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!(
                            "this device doesn't take remote control requests"
                        )),
                    };
                }
                if let Some(handler) = self.inner.handler.get() {
                    handler(ControlAction::SetConsoleMode { enabled });
                    CompanionEvent::CommandResult {
                        ok: true,
                        message: None,
                    }
                } else {
                    CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!(
                            "this device doesn't take remote control requests"
                        )),
                    }
                }
            }
            CompanionCommand::PadAction { action } => {
                if !self.remote_allowed().await {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!(
                            "this device doesn't take remote control requests"
                        )),
                    };
                }
                if !is_valid_pad_action(&action) {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!("unknown remote action")),
                    };
                }
                if let Some(handler) = self.inner.handler.get() {
                    handler(ControlAction::PadAction { action });
                    CompanionEvent::CommandResult {
                        ok: true,
                        message: None,
                    }
                } else {
                    CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!(
                            "this device doesn't take remote control requests"
                        )),
                    }
                }
            }
            CompanionCommand::OpenView { route } => {
                if !self.remote_allowed().await {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!(
                            "this device doesn't take remote control requests"
                        )),
                    };
                }
                let Some(normalized) = normalize_route(&route) else {
                    return CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!("that view can't be opened remotely")),
                    };
                };
                if let Some(handler) = self.inner.handler.get() {
                    handler(ControlAction::OpenView {
                        route: normalized.to_owned(),
                    });
                    CompanionEvent::CommandResult {
                        ok: true,
                        message: None,
                    }
                } else {
                    CompanionEvent::CommandResult {
                        ok: false,
                        message: Some(crate::tr!(
                            "this device doesn't take remote control requests"
                        )),
                    }
                }
            }
        }
    }

    async fn check_control_and_account(&self) -> Option<String> {
        if !self.remote_allowed().await {
            return Some(crate::tr!(
                "this device doesn't take remote control requests"
            ));
        }
        if self.inner.accounts.active_id().is_none() {
            return Some(crate::tr!("no account is signed in on this device"));
        }
        None
    }

    async fn serve(&self, mut session: Session) {
        let peer = session.peer().to_string();
        let mut bus = self.inner.bus.stream();
        // Push the initial state snapshot right after admission.
        let initial = CompanionEvent::State(self.snapshot_state().await);
        if session.writer().write(&initial).await.is_err() {
            session.end().await;
            return;
        }

        loop {
            let (writer, reader) = session.split();
            let event = tokio::select! {
                incoming = reader.read::<CompanionCommand>(MAX_COMMAND_BYTES) => match incoming {
                    Ok(command) => self.handle_command(&peer, command).await,
                    Err(_) => break,
                },
                bus_event = bus.recv() => match bus_event {
                    Ok(
                        BusEvent::GameStarted(_)
                        | BusEvent::GameExited(_)
                        | BusEvent::AccountChanged { .. }
                    ) => CompanionEvent::State(self.snapshot_state().await),
                    Ok(BusEvent::SettingsChanged { namespace, .. })
                        if namespace == "core.console" =>
                    {
                        CompanionEvent::State(self.snapshot_state().await)
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            };
            if writer.write(&event).await.is_err() {
                break;
            }
        }
        session.end().await;
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

struct Serve(Weak<Inner>);

#[async_trait]
impl Sessions for Serve {
    async fn opened(&self, session: Session) {
        let Some(inner) = self.0.upgrade() else {
            return session.end().await;
        };
        CompanionControl { inner }.serve(session).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use caskade_sync::control::{CompanionCommand, CompanionEvent, MAX_EVENT_BYTES};
    use caskade_sync::{MemoryKeyStore, Purpose, Reach};

    use super::*;
    use crate::model::{Game, GameEntry, InstallState, SourceId};

    struct Node {
        store: LibraryStore,
        devices: Devices,
        control: CompanionControl,
        remote_play: RemotePlay,
        bus: Bus,
        _task: tokio::task::JoinHandle<()>,
    }

    async fn start_node(name: &str) -> Node {
        let store = LibraryStore::in_memory().await.unwrap();
        let bus = Bus::default();
        let accounts = AccountService::open(store.clone(), bus.clone())
            .await
            .unwrap();
        let launch = LaunchService::new(store.clone()).with_bus(bus.clone());
        let devices = Devices::with_reach(
            store.clone(),
            accounts.clone(),
            Arc::new(MemoryKeyStore::default()),
            Reach::loopback(),
        );
        let remote_play = RemotePlay::new(store.clone(), devices.clone());
        let control = CompanionControl::new(
            store.clone(),
            devices.clone(),
            accounts,
            launch,
            remote_play.clone(),
            bus.clone(),
        );
        let task = devices.start();
        // Wait for the loopback endpoint to bind.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !devices.status().await.this_device.listening {
            assert!(tokio::time::Instant::now() < deadline, "endpoint bound");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        devices.rename(name).await.unwrap();
        Node {
            store,
            devices,
            control,
            remote_play,
            bus,
            _task: task,
        }
    }

    async fn pair(host: &Node, phone: &Node) -> String {
        let link = host.devices.pairing_link().await.unwrap();
        let paired = phone.devices.accept_pairing_link(&link.url).await.unwrap();
        paired.id
    }

    #[tokio::test]
    async fn paired_companion_controls_host_over_loopback() {
        let host = start_node("Living room TV").await;
        let phone = start_node("Phone").await;

        let mut game = Game::new("Hades II");
        game.platforms = vec!["PC".into()];
        let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), "1145350");
        entry.state = InstallState::Installed;
        host.store.upsert_game(&game).await.unwrap();
        host.store.upsert_entry(&entry).await.unwrap();

        let actions = Arc::new(Mutex::new(Vec::<ControlAction>::new()));
        let recorded = actions.clone();
        host.control.on_action(move |action| {
            recorded.lock().unwrap().push(action);
        });
        let play_reply = host.remote_play.clone();
        let launched_games = Arc::new(Mutex::new(Vec::<String>::new()));
        let launched_ref = launched_games.clone();
        host.remote_play.on_request(move |req| {
            launched_ref.lock().unwrap().push(req.game_id.clone());
            play_reply.finished(&req.id, PlayOutcome::Started);
        });

        let host_id = pair(&host, &phone).await;
        let mut session = phone
            .devices
            .connect(&host_id, Purpose::Control)
            .await
            .unwrap();

        // Initial state pushed on connect.
        let initial: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        match initial {
            CompanionEvent::State(state) => {
                assert_eq!(state.device_name, "Living room TV");
                assert!(state.running_game_id.is_none());
            }
            other => panic!("expected initial State, got {other:?}"),
        }

        // List games returns Hades II.
        session
            .writer()
            .write(&CompanionCommand::ListGames {
                offset: 0,
                limit: 20,
            })
            .await
            .unwrap();
        let listed: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        match listed {
            CompanionEvent::Games { games, total } => {
                assert_eq!(total, 1);
                assert_eq!(games.len(), 1);
                assert_eq!(games[0].title, "Hades II");
                assert!(!games[0].favorite);
            }
            other => panic!("expected Games, got {other:?}"),
        }

        // Toggle favourite updates the store.
        session
            .writer()
            .write(&CompanionCommand::ToggleFavourite {
                game_id: game.id.clone(),
            })
            .await
            .unwrap();
        let toggled: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        assert_eq!(
            toggled,
            CompanionEvent::CommandResult {
                ok: true,
                message: None
            }
        );
        assert!(
            host.store
                .get_game(&game.id)
                .await
                .unwrap()
                .unwrap()
                .game
                .favorite
        );

        // Launch routes through RemotePlay.
        session
            .writer()
            .write(&CompanionCommand::LaunchGame {
                game_id: game.id.clone(),
            })
            .await
            .unwrap();
        let launch_res: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        assert_eq!(
            launch_res,
            CompanionEvent::CommandResult {
                ok: true,
                message: None
            }
        );
        assert_eq!(*launched_games.lock().unwrap(), vec![game.id.clone()]);

        // Valid pad action & view navigation reach the shell handler.
        session
            .writer()
            .write(&CompanionCommand::PadAction {
                action: "guide".into(),
            })
            .await
            .unwrap();
        let pad_res: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        assert_eq!(
            pad_res,
            CompanionEvent::CommandResult {
                ok: true,
                message: None
            }
        );

        session
            .writer()
            .write(&CompanionCommand::OpenView {
                route: "/home".into(),
            })
            .await
            .unwrap();
        let view_res: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        assert_eq!(
            view_res,
            CompanionEvent::CommandResult {
                ok: true,
                message: None
            }
        );

        // Invalid pad action & non-allowlisted route are rejected cleanly.
        session
            .writer()
            .write(&CompanionCommand::PadAction {
                action: "not.real".into(),
            })
            .await
            .unwrap();
        let bad_pad: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        assert!(matches!(
            bad_pad,
            CompanionEvent::CommandResult { ok: false, .. }
        ));

        session
            .writer()
            .write(&CompanionCommand::OpenView {
                route: "/plugins/arbitrary".into(),
            })
            .await
            .unwrap();
        let bad_view: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        assert!(matches!(
            bad_view,
            CompanionEvent::CommandResult { ok: false, .. }
        ));

        // Bus event triggers a live State push to the companion.
        host.bus.emit(BusEvent::SettingsChanged {
            namespace: "core.console".into(),
            version: 1,
        });
        let pushed: CompanionEvent = session.reader().read(MAX_EVENT_BYTES).await.unwrap();
        assert!(matches!(pushed, CompanionEvent::State(_)));

        let seen = actions.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec![
                ControlAction::PadAction {
                    action: "guide".into()
                },
                ControlAction::OpenView { route: "/".into() },
            ]
        );

        session.end().await;
        host.devices.shutdown().await;
        phone.devices.shutdown().await;
    }

    #[tokio::test]
    async fn unpaired_peer_is_refused_control_session() {
        let host = start_node("Living room TV").await;
        let stranger = start_node("Stranger").await;

        let link = host.devices.pairing_link().await.unwrap();
        let parsed = caskade_sync::PairingLink::parse(&link.url).unwrap();
        let refused = stranger
            .devices
            .connect_to(parsed.addr().clone(), Purpose::Control, None)
            .await;
        assert!(refused.is_err());

        host.devices.shutdown().await;
        stranger.devices.shutdown().await;
    }
}
