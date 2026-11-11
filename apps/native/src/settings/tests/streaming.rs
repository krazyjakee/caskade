//! Settings → Streaming (#105), over a fake streaming host and client
//! (no streaming server runs in tests), as the webview's panel behaves:
//! hosting switched on with its states, a waiting device paired by its
//! code, paired devices forgotten after asking in their row; devices to
//! stream from listed with their reach, paired through the dialog's code
//! and added by address; the stream quality saved; programs updated.

use std::sync::{Arc, Mutex};

use crate::data::use_data_source;
use crate::frame::tests::{ONE, Window, source};
use crate::input::InputRoot;
use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_app::events::Event;
use caskade_core::error::ErrorReport;
use caskade_core::settings::SettingsSnapshot;
use caskade_ui_data::streaming::{
    HostOutcome, HostReach, HostState, HostStatus, PairRequest, PairRequestHost, PairedClient,
    PairingOutcome, PairingStarted, StreamHost, StreamQuality, Streaming, StreamingBackend,
    StreamingPairingFinished, StreamingPart, StreamingProgram,
};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;
use tokio::sync::broadcast;

type Pending<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, ErrorReport>> + Send + 'a>>;

fn ready<'a, T: Send + 'a>(value: T) -> Pending<'a, T> {
    Box::pin(async move { Ok(value) })
}

/// A streaming host and client in memory, recording what was asked.
struct Fake {
    state: Mutex<HostState>,
    /// What turning hosting on answers.
    on_outcome: Mutex<HostOutcome>,
    requests: Mutex<Vec<PairRequest>>,
    paired: Mutex<Vec<PairedClient>>,
    programs: Mutex<Vec<StreamingProgram>>,
    hosts: Mutex<Vec<StreamHost>>,
    heard: Mutex<Vec<String>>,
    events: broadcast::Sender<Event>,
}

impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(HostState::Off),
            on_outcome: Mutex::new(HostOutcome::On),
            requests: Mutex::new(Vec::new()),
            paired: Mutex::new(Vec::new()),
            programs: Mutex::new(Vec::new()),
            hosts: Mutex::new(Vec::new()),
            heard: Mutex::new(Vec::new()),
            events: broadcast::channel(16).0,
        })
    }

    fn hear(&self, what: impl Into<String>) {
        self.heard.lock().unwrap().push(what.into());
    }

    fn heard(&self) -> Vec<String> {
        self.heard.lock().unwrap().clone()
    }
}

impl PairRequestHost for Fake {
    fn ready(&self) -> bool {
        *self.state.lock().unwrap() == HostState::Ready
    }

    fn requests(&self) -> Pending<'_, Vec<PairRequest>> {
        ready(self.requests.lock().unwrap().clone())
    }

    fn accept<'a>(&'a self, id: &'a str, code: &'a str, name: &'a str) -> Pending<'a, ()> {
        self.hear(format!("accept {id} {code} {name}"));
        self.requests
            .lock()
            .unwrap()
            .retain(|request| request.id != id);
        self.paired.lock().unwrap().push(PairedClient {
            uuid: id.to_owned(),
            name: name.to_owned(),
        });
        ready(())
    }

    fn decline<'a>(&'a self, id: &'a str) -> Pending<'a, ()> {
        self.hear(format!("decline {id}"));
        self.requests
            .lock()
            .unwrap()
            .retain(|request| request.id != id);
        ready(())
    }
}

impl StreamingBackend for Fake {
    fn status(&self) -> HostStatus {
        HostStatus {
            state: *self.state.lock().unwrap(),
            version: None,
            problem: None,
            warning: None,
            unreachable: false,
        }
    }

    fn set_hosting(&self, on: bool) -> Pending<'_, HostOutcome> {
        self.hear(format!("hosting {on}"));
        let outcome = if on {
            *self.on_outcome.lock().unwrap()
        } else {
            HostOutcome::Off
        };
        if outcome == HostOutcome::On {
            *self.state.lock().unwrap() = HostState::Ready;
        } else if outcome == HostOutcome::Off {
            *self.state.lock().unwrap() = HostState::Off;
        }
        ready(outcome)
    }

    fn repair(&self) -> Pending<'_, HostOutcome> {
        self.hear("repair");
        ready(HostOutcome::On)
    }

    fn use_existing<'a>(&'a self, user: &'a str, password: &'a str) -> Pending<'a, HostOutcome> {
        self.hear(format!("existing {user} {}", password.len()));
        *self.state.lock().unwrap() = HostState::Ready;
        ready(HostOutcome::On)
    }

    fn paired_devices(&self) -> Pending<'_, Vec<PairedClient>> {
        ready(self.paired.lock().unwrap().clone())
    }

    fn unpair<'a>(&'a self, uuid: &'a str) -> Pending<'a, ()> {
        self.hear(format!("unpair {uuid}"));
        self.paired
            .lock()
            .unwrap()
            .retain(|device| device.uuid != uuid);
        ready(())
    }

    fn unpair_all(&self) -> Pending<'_, ()> {
        self.hear("unpair all");
        self.paired.lock().unwrap().clear();
        ready(())
    }

    fn programs(&self) -> Pending<'_, Vec<StreamingProgram>> {
        ready(self.programs.lock().unwrap().clone())
    }

    fn update(&self, part: StreamingPart) -> Pending<'_, HostOutcome> {
        self.hear(format!("update {part:?}"));
        for program in self.programs.lock().unwrap().iter_mut() {
            if program.part == part {
                program.update = false;
            }
        }
        ready(HostOutcome::On)
    }

    fn skip(&self, part: StreamingPart) -> Pending<'_, ()> {
        self.hear(format!("skip {part:?}"));
        ready(())
    }

    fn hosts(&self) -> Vec<StreamHost> {
        self.hosts.lock().unwrap().clone()
    }

    fn look_again(&self) -> Pending<'_, Vec<StreamHost>> {
        self.hear("look again");
        ready(self.hosts())
    }

    fn add_host<'a>(&'a self, address: &'a str) -> Pending<'a, Vec<StreamHost>> {
        self.hear(format!("add {address}"));
        self.hosts.lock().unwrap().push(StreamHost {
            id: format!("at-{address}"),
            name: address.to_owned(),
            address: Some(address.to_owned()),
            reach: HostReach::Ready,
            paired: false,
            games: 0,
        });
        ready(self.hosts())
    }

    fn forget_host<'a>(&'a self, id: &'a str) -> Pending<'a, ()> {
        self.hear(format!("forget {id}"));
        for host in self.hosts.lock().unwrap().iter_mut() {
            if host.id == id {
                host.paired = false;
            }
        }
        ready(())
    }

    fn pair_with<'a>(&'a self, host_id: &'a str) -> Pending<'a, PairingStarted> {
        self.hear(format!("pair {host_id}"));
        let name = self
            .hosts()
            .into_iter()
            .find(|host| host.id == host_id)
            .map(|host| host.name)
            .unwrap_or_default();
        ready(PairingStarted {
            host_id: host_id.to_owned(),
            host_name: name,
            code: "4711".into(),
            automatic: false,
        })
    }

    fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    fn set_quality(&self, quality: StreamQuality) -> Pending<'_, SettingsSnapshot> {
        self.hear(format!(
            "quality {:?} {:?} {:?}",
            quality.resolution, quality.fps, quality.bitrate_kbps
        ));
        let settings = source().settings.clone();
        Box::pin(async move {
            settings
                .update("core.streaming", move |mut value| {
                    value["quality"] = serde_json::to_value(&quality).unwrap();
                    value
                })
                .await
        })
    }
}

static FAKE: Mutex<Option<Arc<Fake>>> = Mutex::new(None);

/// Accounts whose first, signed in, is an admin.
fn admin_accounts() -> caskade_core::accounts::AccountService {
    caskade_app::runtime::block_on(async {
        let store = caskade_core::db::LibraryStore::in_memory().await.unwrap();
        caskade_core::accounts::AccountService::open(store, caskade_core::bus::Bus::default())
            .await
            .unwrap()
    })
}

static ACCOUNTS: std::sync::OnceLock<caskade_core::accounts::AccountService> =
    std::sync::OnceLock::new();

fn app() -> Element {
    use_data_source(|| source().clone());
    let fake = FAKE.lock().unwrap().clone().expect("a fake");
    let accounts = ACCOUNTS.get_or_init(admin_accounts).clone();
    use_context_provider(move || Streaming::new(fake, accounts));
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            div { id: "panel",
                crate::settings::streaming::StreamingSettings {}
            }
        }
    }
}

fn open(fake: &Arc<Fake>) -> Window {
    source();
    *FAKE.lock().unwrap() = Some(fake.clone());
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 1600, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("the panel", |window| {
        window.find("data-cs", "streaming-settings").is_some()
    });
    window
}

fn press(window: &mut Window, id: &str) {
    let node = window.get("id", id);
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

fn button(window: &Window, text: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| window.text(button).trim() == text)
        .unwrap_or_else(|| panic!("no {text} button"))
}

fn press_button(window: &mut Window, text: &str) {
    let node = button(window, text);
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

fn text_of(window: &Window, name: &str, value: &str) -> String {
    window
        .find(name, value)
        .map(|node| window.text(node))
        .unwrap_or_default()
}

#[test]
fn hosting_turns_on_and_a_waiting_device_pairs_with_its_code() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let fake = Fake::new();
    let mut window = open(&fake);
    window.settle_until("hosting off", |window| {
        window
            .find("data-part", "host-state")
            .and_then(|node| window.attr(node, "data-status"))
            .as_deref()
            == Some("off")
    });
    assert_eq!(text_of(&window, "data-part", "host-state").trim(), "Off.");
    // The fixture's account is an admin: no note, and the switch works.
    assert!(window.find("role", "note").is_none());
    let switch = window.get("id", "streaming-hosting");
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("ready", |window| {
        text_of(window, "data-part", "host-state").trim()
            == "Ready. Devices on this network can pair and play."
    });
    assert_eq!(fake.heard(), ["hosting true"]);
    assert!(
        window
            .text(window.get("data-part", "paired"))
            .contains("None yet."),
        "nobody paired yet"
    );
    // A device asks; its form is in the panel, not in a prompt.
    fake.requests.lock().unwrap().push(PairRequest {
        id: "tv".into(),
        name: "Living room TV".into(),
        address: "192.0.2.7".into(),
    });
    window.settle_until("the request", |window| {
        window.find("data-part", "requests").is_some()
    });
    assert_eq!(
        text_of(&window, "data-part", "ask"),
        "Living room TV wants to stream games from this device."
    );
    let code = window.get("id", "pair-request-code");
    window.focus(code);
    window.type_text("2468");
    press(&mut window, "pair-request-accept");
    window.settle_until("paired", |window| {
        window.find("data-cs", "streaming-client").is_some()
            && window.find("data-part", "requests").is_none()
    });
    assert!(
        fake.heard()
            .contains(&"accept tv 2468 Living room TV".to_owned())
    );
    // Forget asks in its row first.
    press_button(&mut window, "Forget…");
    window.settle_until("the question", |window| {
        window
            .text(window.get("data-part", "paired"))
            .contains("It needs a new code to play from this device again.")
    });
    press_button(&mut window, "Forget Living room TV");
    window.settle_until("forgotten", |window| {
        window.find("data-cs", "streaming-client").is_none()
    });
    assert!(fake.heard().contains(&"unpair tv".to_owned()));
}

#[test]
fn streaming_set_up_before_asks_for_its_sign_in() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let fake = Fake::new();
    *fake.on_outcome.lock().unwrap() = HostOutcome::ExistingFound;
    let mut window = open(&fake);
    window.settle_until("hosting off", |window| {
        window.find("data-part", "host-state").is_some()
    });
    let switch = window.get("id", "streaming-hosting");
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("the dialog", |window| {
        window.find("data-cs", "streaming-existing").is_some()
    });
    let user = window.get("id", "streaming-existing-user");
    window.focus(user);
    window.type_text("moon");
    let password = window.get("id", "streaming-existing-password");
    window.focus(password);
    window.type_text("light");
    press(&mut window, "streaming-existing-use");
    window.settle_until("on", |window| {
        window.find("data-cs", "streaming-existing").is_none()
            && text_of(window, "data-part", "host-state").starts_with("Ready")
    });
    assert_eq!(fake.heard(), ["hosting true", "existing moon 5"]);
}

#[test]
fn a_device_to_stream_from_pairs_through_the_dialogs_code() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let fake = Fake::new();
    fake.hosts.lock().unwrap().extend([
        StreamHost {
            id: "den".into(),
            name: "Den".into(),
            address: Some("192.0.2.9".into()),
            reach: HostReach::Ready,
            paired: false,
            games: 0,
        },
        StreamHost {
            id: "attic".into(),
            name: "Attic".into(),
            address: None,
            reach: HostReach::Asleep,
            paired: true,
            games: 12,
        },
    ]);
    let mut window = open(&fake);
    window.settle_until("the devices", |window| {
        window.find_all("data-cs", "stream-host").len() == 2
    });
    let states: Vec<String> = window
        .find_all("data-part", "state")
        .into_iter()
        .map(|node| window.text(node))
        .collect();
    assert_eq!(
        states,
        [
            "Ready · Not paired",
            "Asleep: it wakes when you stream · Paired · 12 games"
        ]
    );
    press_button(&mut window, "Pair…");
    window.settle_until("the code", |window| {
        window.find("data-part", "code").is_some()
    });
    assert_eq!(text_of(&window, "data-part", "code"), "4711");
    let _ = fake
        .events
        .send(Event::StreamingPairingFinished(StreamingPairingFinished {
            host_id: "den".into(),
            outcome: PairingOutcome::WrongCode,
            message: None,
        }));
    window.settle_until("why", |window| {
        text_of(window, "data-cs", "pair-dialog")
            .contains("The code entered on Den didn't match. Try again with a new code.")
    });
    press(&mut window, "pair-dialog-again");
    window.settle_until("asked again", |window| {
        fake.heard()
            .iter()
            .filter(|said| *said == "pair den")
            .count()
            == 2
            && window.find("data-part", "code").is_some()
    });
    let _ = fake
        .events
        .send(Event::StreamingPairingFinished(StreamingPairingFinished {
            host_id: "den".into(),
            outcome: PairingOutcome::Paired,
            message: None,
        }));
    window.settle_until("paired", |window| {
        text_of(window, "data-cs", "pair-dialog")
            .contains("Paired. Den's games show in your library as Stream from Den.")
    });
    press(&mut window, "pair-dialog-done");
    window.settle_until("closed", |window| {
        window.find("data-cs", "pair-dialog").is_none()
    });
    // Added by its address, then the field empties.
    let field = window.get("id", "streaming-address");
    window.focus(field);
    window.type_text("192.0.2.30");
    press(&mut window, "streaming-add");
    window.settle_until("added", |window| {
        window.find_all("data-cs", "stream-host").len() == 3
    });
    assert!(fake.heard().contains(&"add 192.0.2.30".to_owned()));
}

#[test]
fn the_stream_quality_saves_and_programs_update() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let before = caskade_app::runtime::block_on(source().settings.get("core.streaming"))
        .unwrap()
        .value;
    let fake = Fake::new();
    fake.programs.lock().unwrap().push(StreamingProgram {
        part: StreamingPart::Client,
        label: "Game streaming".into(),
        installed: Some("1.0".into()),
        latest: Some("1.1".into()),
        update: true,
    });
    let mut window = open(&fake);
    window.settle_until("the programs", |window| {
        window.find("data-cs", "streaming-program").is_some()
    });
    assert_eq!(
        text_of(&window, "data-part", "version"),
        "A newer version, 1.1, is ready."
    );
    press_button(&mut window, "Update");
    window.settle_until("up to date", |window| {
        text_of(window, "data-part", "version") == "Up to date."
    });
    assert!(fake.heard().contains(&"update Client".to_owned()));
    // Frame rate by its arrows: Automatic, then 30, then 60.
    let trigger = window.get("id", "streaming-fps");
    window.focus(trigger);
    window.key(Key::Named(NamedKey::ArrowDown));
    window.settle_until("the list", |window| {
        window.find("role", "listbox").is_some()
    });
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("saved", |window| {
        window
            .text(window.get("id", "streaming-fps"))
            .contains("60 frames a second")
    });
    assert!(
        fake.heard()
            .contains(&"quality None Some(60) None".to_owned())
    );
    caskade_app::runtime::block_on(
        source()
            .settings
            .update("core.streaming", move |_| before.clone()),
    )
    .unwrap();
}
