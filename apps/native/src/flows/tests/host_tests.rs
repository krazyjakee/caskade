//! The hosts over any view (#108): stream pairing requests, games other
//! devices ask to start here, and a release's tools.

use super::*;

use crate::flows::remote_play::{RemotePlayHost, RemotePlays};
use crate::flows::streaming::{HostBusy, StopHostDialog, StreamPairingHost};
use crate::flows::tool_request::ToolRequest;
use caskade_ui_data::streaming::{PairRequest, PairRequestHost, StreamPairing};

/// A streaming host that lists `requests` and takes the code 4321.
#[derive(Default)]
struct FakeStreamHost {
    requests: Mutex<Vec<PairRequest>>,
    accepted: Mutex<Vec<(String, String, String)>>,
    declined: Mutex<Vec<String>>,
}

type Pending<'a, T> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<T, caskade_core::error::ErrorReport>> + Send + 'a>,
>;

impl PairRequestHost for FakeStreamHost {
    fn ready(&self) -> bool {
        true
    }

    fn requests(&self) -> Pending<'_, Vec<PairRequest>> {
        let list = self.requests.lock().unwrap().clone();
        Box::pin(async move { Ok(list) })
    }

    fn accept<'a>(&'a self, id: &'a str, code: &'a str, name: &'a str) -> Pending<'a, ()> {
        Box::pin(async move {
            if code != "4321" {
                return Err("The code didn't match".into());
            }
            self.requests
                .lock()
                .unwrap()
                .retain(|request| request.id != id);
            self.accepted
                .lock()
                .unwrap()
                .push((id.to_owned(), code.to_owned(), name.to_owned()));
            Ok(())
        })
    }

    fn decline<'a>(&'a self, id: &'a str) -> Pending<'a, ()> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .retain(|request| request.id != id);
            self.declined.lock().unwrap().push(id.to_owned());
            Ok(())
        })
    }
}

/// What a host test shows.
#[derive(Clone)]
enum HostShown {
    Stream(StreamPairing),
    Stop {
        launching: bool,
    },
    Tool(Box<caskade_core::launch::LaunchService>),
    /// A tool the age limit holds back: the launcher, the accounts it asks,
    /// the release and the tool.
    GatedTool(
        Box<(
            caskade_core::launch::LaunchService,
            caskade_ui_data::accounts::Accounts,
            String,
            String,
        )>,
    ),
    Remote(RemotePlays),
}
static HOST_SHOWN: Mutex<Option<HostShown>> = Mutex::new(None);
/// What the host test heard: closed, confirmed, or a remote play's answer.
static HOST_HEARD: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn host_heard(what: impl Into<String>) {
    caskade_core::lock(&HOST_HEARD).push(what.into());
}

fn host_heard_all() -> Vec<String> {
    caskade_core::lock(&HOST_HEARD).clone()
}

fn host_app() -> Element {
    use_data_source(|| source().clone());
    let shown = caskade_core::lock(&HOST_SHOWN).clone().unwrap();
    if let HostShown::Tool(launch) = &shown {
        let launch = (**launch).clone();
        use_context_provider(move || launch);
    }
    if let HostShown::GatedTool(gated) = &shown {
        let (launch, accounts) = (gated.0.clone(), gated.1.clone());
        use_context_provider(move || launch);
        use_context_provider(move || accounts);
    }
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            div { id: "hosts-ready",
                match shown {
                    HostShown::Stream(pairing) => rsx! {
                        StreamPairingHost { pairing }
                    },
                    HostShown::Stop { launching } => rsx! {
                        StopHostDialog {
                            busy: HostBusy { device: "Desk".into(), playing: Some("Celeste".into()) },
                            title: Some("Hades".to_owned()),
                            launching,
                            on_close: |_| host_heard("closed"),
                            on_confirm: |_| host_heard("confirmed"),
                        }
                    },
                    HostShown::Tool(_) => rsx! {
                        ToolRequest {
                            entry_id: "no-such-entry",
                            action_id: "defrag",
                            name: "Defragmenter",
                            on_close: |_| host_heard("closed"),
                        }
                    },
                    HostShown::GatedTool(gated) => rsx! {
                        ToolRequest {
                            entry_id: gated.2.clone(),
                            action_id: gated.3.clone(),
                            name: "Settings tool",
                            on_close: |_| host_heard("closed"),
                        }
                    },
                    HostShown::Remote(plays) => rsx! {
                        RemotePlayHost { plays }
                    },
                }
            }
        }
    }
}

fn open_host(shown: HostShown) -> Window {
    source();
    *caskade_core::lock(&HOST_SHOWN) = Some(shown);
    caskade_core::lock(&HOST_HEARD).clear();
    Window::open_root(host_app, "the host", "#hosts-ready")
}

/// An account service whose first account, signed in, is an admin.
fn admin_accounts() -> caskade_core::accounts::AccountService {
    caskade_app::runtime::block_on(async {
        let store = caskade_core::db::LibraryStore::in_memory().await.unwrap();
        caskade_core::accounts::AccountService::open(store, caskade_core::bus::Bus::default())
            .await
            .unwrap()
    })
}

#[test]
fn a_device_asking_to_stream_is_prompted_and_paired_with_the_code_it_shows() {
    let _one = caskade_core::lock(&ONE);
    let host = std::sync::Arc::new(FakeStreamHost::default());
    host.requests.lock().unwrap().push(PairRequest {
        id: "tv".into(),
        name: "Living room TV".into(),
        address: "192.0.2.7".into(),
    });
    let pairing = StreamPairing::new(host.clone(), admin_accounts());
    let mut window = open_host(HostShown::Stream(pairing));
    window.until("the prompt", |w| {
        w.find("[data-cs=stream-pair-prompt]").is_some()
    });
    assert_eq!(
        window.text("[data-cs=stream-pair-prompt] h2"),
        "Living room TV wants to play from this device"
    );
    assert_eq!(
        window.text("[data-cs=pair-request] [data-part=ask]"),
        "Living room TV wants to stream games from this device."
    );
    assert_eq!(
        window
            .attr("#pair-request-accept", "aria-disabled")
            .as_deref(),
        Some("true"),
        "no code yet"
    );
    // Only digits, four of them.
    window.focus("#pair-request-code");
    window.type_text("12a345");
    assert_eq!(
        window.attr("#pair-request-code", "value").as_deref(),
        Some("1234")
    );
    window.press("#pair-request-accept");
    window.until("why it didn't pair", |w| {
        w.text("[data-cs=pair-request]")
            .contains("The code didn't match")
    });
    assert!(host.accepted.lock().unwrap().is_empty());
    window.focus("#pair-request-code");
    for _ in 0..4 {
        window.key(Key::Named(NamedKey::Backspace));
    }
    window.type_text("4321");
    window.focus("#pair-request-name");
    window.key(Key::Named(NamedKey::End));
    window.type_text(" (den)");
    window.press("#pair-request-accept");
    window.until("the prompt gone", |w| {
        w.find("[data-cs=stream-pair-prompt]").is_none()
    });
    assert_eq!(
        *host.accepted.lock().unwrap(),
        [(
            "tv".to_owned(),
            "4321".to_owned(),
            "Living room TV (den)".to_owned()
        )]
    );
}

#[test]
fn a_stream_request_put_off_stays_away_and_the_next_one_shows() {
    let _one = caskade_core::lock(&ONE);
    let host = std::sync::Arc::new(FakeStreamHost::default());
    for (id, name) in [("tv", "Living room TV"), ("deck", "Deck")] {
        host.requests.lock().unwrap().push(PairRequest {
            id: id.into(),
            name: name.into(),
            address: "192.0.2.7".into(),
        });
    }
    let pairing = StreamPairing::new(host.clone(), admin_accounts());
    let mut window = open_host(HostShown::Stream(pairing));
    window.until("the first prompt", |w| {
        w.text("[data-cs=stream-pair-prompt] h2") == "Living room TV wants to play from this device"
    });
    window.press("#stream-pair-not-now");
    window.until("the second prompt", |w| {
        w.text("[data-cs=stream-pair-prompt] h2") == "Deck wants to play from this device"
    });
    assert!(host.declined.lock().unwrap().is_empty(), "not now isn't no");
    window.press("#pair-request-decline");
    window.until("no prompt", |w| {
        w.find("[data-cs=stream-pair-prompt]").is_none()
    });
    assert_eq!(*host.declined.lock().unwrap(), ["deck"]);
    // The one put off still waits, for Settings → Streaming.
    assert_eq!(host.requests.lock().unwrap().len(), 1);
}

#[test]
fn stopping_the_other_game_asks_in_the_webviews_words() {
    let _one = caskade_core::lock(&ONE);
    let mut window = open_host(HostShown::Stop { launching: false });
    window.until("the dialog", |w| w.find("[data-cs=stop-host]").is_some());
    assert_eq!(
        window.text("[data-cs=stop-host] h2"),
        "Stop the other game?"
    );
    assert_eq!(
        window.text("[data-cs=stop-host] [data-part=question]"),
        "Desk is playing Celeste. Stop it and stream Hades?"
    );
    window.press("#stop-host-confirm");
    window.press("#stop-host-cancel");
    assert_eq!(host_heard_all(), ["confirmed", "closed"]);

    let window = open_host(HostShown::Stop { launching: true });
    assert_eq!(window.text("#stop-host-confirm"), "Starting…");
    assert_eq!(
        window.attr("#stop-host-cancel", "aria-disabled").as_deref(),
        Some("true")
    );
}

#[test]
fn a_tool_that_cant_start_says_why() {
    let _one = caskade_core::lock(&ONE);
    let launch = caskade_app::runtime::block_on(async {
        let store = caskade_core::db::LibraryStore::in_memory().await.unwrap();
        caskade_core::launch::LaunchService::new(store).with_aliases(None)
    });
    let mut window = open_host(HostShown::Tool(Box::new(launch)));
    window.until("why", |w| w.find("[data-cs=tool-failed]").is_some());
    assert_eq!(
        window.text("[data-cs=tool-failed] h2"),
        "Defragmenter didn't start"
    );
    assert!(!window.text("[data-cs=tool-failed] p").is_empty());
    window.press("#tool-failed-ok");
    assert_eq!(host_heard_all(), ["closed"]);
}

#[test]
fn a_tool_the_age_limit_holds_back_asks_for_an_admins_pin_then_runs() {
    use caskade_core::accounts::{AccountAccess, AccountRole, NewAccount, Permissions};
    use caskade_core::model::{
        ActionKind, ActionTracking, Game, GameAction, GameEntry, InstallLocation, InstallState,
        SourceId,
    };
    let _one = caskade_core::lock(&ONE);
    let dir = tempfile::tempdir().unwrap();
    let (launch, accounts, entry, tool) = caskade_app::runtime::block_on(async {
        let store = caskade_core::db::LibraryStore::in_memory().await.unwrap();
        let accounts = caskade_core::accounts::AccountService::open(
            store.clone(),
            caskade_core::bus::Bus::default(),
        )
        .await
        .unwrap();
        let admin = accounts.signed_in().unwrap();
        accounts
            .set_pin(&admin.id, Some("2468".into()))
            .await
            .unwrap();
        let kit = accounts
            .create(NewAccount {
                name: "Kit".into(),
                avatar: crate::ui::DEFAULT_AVATAR.into(),
                access: AccountAccess {
                    role: AccountRole::Standard,
                    permissions: Permissions::default(),
                    max_age: Some(12),
                    unrated_needs_pin: true,
                },
                pin: None,
            })
            .await
            .unwrap();
        // An unrated game, whose tool runs a program that's always there.
        let game = Game::new("Grown-up Game");
        store.upsert_game(&game).await.unwrap();
        let program = LaunchSpec::Executable {
            path: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: Default::default(),
        };
        let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "release".to_owned());
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External {
            path: dir.path().into(),
        });
        entry.launch = Some(program.clone());
        store.upsert_entry(&entry).await.unwrap();
        let tool = store
            .save_game_action(&GameAction {
                id: caskade_core::model::new_id(),
                entry_id: entry.id.clone(),
                name: "Settings tool".into(),
                kind: ActionKind::Utility,
                target: program,
                tracking: ActionTracking::default(),
                proton: None,
            })
            .await
            .unwrap();
        accounts.sign_in(&kit.id, None).await.unwrap();
        let launch = caskade_core::launch::LaunchService::new(store)
            .with_aliases(None)
            .with_play_gate(std::sync::Arc::new(accounts.clone()));
        (launch, accounts, entry.id, tool.id)
    });
    let handle = caskade_ui_data::accounts::Accounts::new(accounts);
    let mut window = open_host(HostShown::GatedTool(Box::new((
        launch, handle, entry, tool,
    ))));
    // Held back: the admin's PIN first, nothing started yet.
    window.until("the age gate", |w| w.find("[data-cs=pin-pad]").is_some());
    assert!(
        window
            .text("[role=dialog] h2")
            .contains("An admin PIN is needed to play Grown-up Game")
    );
    assert!(host_heard_all().is_empty());
    window.focus("[data-cs=pin-pad] [data-part=field]");
    window.type_text("0000");
    window.key(Key::Named(NamedKey::Enter));
    window.until("refused", |w| {
        w.text("[data-cs=pin-pad] [data-part=message]")
            .contains("That PIN isn't right.")
    });
    assert!(host_heard_all().is_empty());
    window.focus("[data-cs=pin-pad] [data-part=field]");
    window.type_text("2468");
    window.key(Key::Named(NamedKey::Enter));
    // Approved: the tool runs once.
    window.until("it ran", |_| host_heard_all() == ["closed"]);
    assert!(window.find("[data-cs=tool-failed]").is_none());
}

#[test]
fn a_game_another_device_asks_for_is_answered_with_how_it_went() {
    let _one = caskade_core::lock(&ONE);
    let (events, _keep) = tokio::sync::broadcast::channel(16);
    let plays = RemotePlays::new(
        events.clone(),
        std::sync::Arc::new(|id: &str, started: bool, reason: Option<String>| {
            host_heard(format!("{id} {started} {}", reason.unwrap_or_default()));
        }),
    );
    let mut window = open_host(HostShown::Remote(plays));
    let _ = events.send(caskade_app::events::Event::RemotePlayRequested(
        caskade_app::events::RemotePlayRequested {
            request_id: "r1".into(),
            game_id: "no-such-game".into(),
            device_name: "Deck".into(),
        },
    ));
    window.until("the answer", |_| !host_heard_all().is_empty());
    assert_eq!(
        host_heard_all(),
        ["r1 false It has nothing installed that can launch."]
    );
    window.settle();
    assert!(
        window.find("[role=dialog], [role=alertdialog]").is_none(),
        "the other device hears why; nothing stays open here"
    );
}
