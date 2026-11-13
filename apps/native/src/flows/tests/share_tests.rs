//! The share flows over two real cores on loopback with no relay (as
//! caskade-share's own tests run them): Desk makes a link to a game from
//! Share with someone… and stops it; Friend looks at a link, sees what it
//! shares, and receives the game, which arrives as an install.

use super::*;
use crate::flows::receive_share::ReceiveShareDialog;
use crate::flows::share_game::{ShareGameDialog, ShareTarget};
use caskade_core::accounts::AccountService;
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::install::{InstallOptions, InstallService, JobStatus};
use caskade_core::model::{
    Game, GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec, StorageRoot,
};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::transfer::Transfers;
use caskade_installer::manifest::RootManifest;
use caskade_installer::pipeline::input;
use caskade_ui_data::shares::{ShareExpiry, ShareState, Shares};
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Core {
    store: LibraryStore,
    devices: Devices,
    install: InstallService,
    shares: caskade_share::Shares,
    accounts: AccountService,
    dir: tempfile::TempDir,
}

impl Core {
    fn handle(&self) -> Shares {
        Shares::new(self.shares.clone(), self.accounts.clone())
    }
}

/// A device on loopback named `name`, with a games folder as its default
/// location and the share service started, as the app sets them up.
fn core(name: &'static str) -> Core {
    core_with(name, Reach::loopback())
}

/// As [`core`], reached as `reach` says.
fn core_with(name: &'static str, reach: Reach) -> Core {
    caskade_app::runtime::block_on(async move {
        let dir = tempfile::tempdir().unwrap();
        let store = LibraryStore::in_memory().await.unwrap();
        let accounts = AccountService::open(store.clone(), Bus::default())
            .await
            .unwrap();
        let settings = SettingsService::new(store.clone());
        settings
            .register_core(&AppPaths::under(&dir.path().join("app")))
            .unwrap();
        let devices = Devices::with_reach(
            store.clone(),
            accounts.clone(),
            Arc::new(MemoryKeyStore::default()),
            reach,
        );
        devices.set_settings(settings.clone());
        let cache = dir.path().join("cache");
        let transfers = Transfers::new(store.clone(), devices.clone(), &cache)
            .await
            .unwrap();
        let games = dir.path().join("Games");
        std::fs::create_dir_all(&games).unwrap();
        let identity = RootManifest::new(format!("{}-root", name.to_lowercase())).unwrap();
        identity.write(&games).unwrap();
        let mut root = StorageRoot::from_manifest(games.canonicalize().unwrap(), &identity);
        root.is_default = true;
        store.upsert_root(&root).await.unwrap();
        let install = InstallService::new(
            store.clone(),
            Bus::default(),
            InstallOptions {
                downloads: 2,
                aliases: None,
            },
        )
        .with_settings(settings.clone())
        .with_host(input::host(Some("linux")).unwrap());
        install.set_peer_fetcher(Arc::new(transfers.clone()));
        let shares = caskade_share::Shares::new(
            store.clone(),
            devices.clone(),
            transfers.clone(),
            install.clone(),
            &cache,
        );
        devices.start();
        let until = Instant::now() + Duration::from_secs(20);
        while !devices.status().await.this_device.listening {
            assert!(Instant::now() < until, "{name}'s endpoint starts");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        devices.rename(name).await.unwrap();
        Core {
            store,
            devices,
            install,
            shares,
            accounts,
            dir,
        }
    })
}

/// Celeste, a Windows game added by hand on `core`: its installation's id.
fn add_celeste(core: &Core) -> String {
    let folder = core.dir.path().join("My Games").join("Celeste");
    std::fs::create_dir_all(folder.join("Content")).unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../crates/caskade-installer/tests/fixtures/bin/fixture-game-windows-x86_64.exe",
        ),
        folder.join("Celeste.exe"),
    )
    .unwrap();
    std::fs::write(folder.join("Content/Celeste.pak"), vec![5u8; 200_000]).unwrap();
    let store = core.store.clone();
    caskade_app::runtime::block_on(async move {
        let game = store.upsert_game(&Game::new("Celeste")).await.unwrap();
        let mut entry = GameEntry::new(&game.id, "manual".to_owned().into(), "celeste");
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External {
            path: folder.clone(),
        });
        entry.launch = Some(LaunchSpec::Executable {
            path: folder.join("Celeste.exe"),
            args: Vec::new(),
            cwd: None,
            env: Default::default(),
        });
        entry.runtime = GameRuntime::Windows {
            prefix: None,
            runner: None,
        };
        store.upsert_entry(&entry).await.unwrap().id
    })
}

/// What a share test shows.
#[derive(Clone)]
enum Shown {
    Share {
        shares: Shares,
        entry_id: String,
    },
    Receive {
        shares: Shares,
    },
    Pair {
        pairing: caskade_ui_data::pairing::Pairing,
    },
}
static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);
/// What the dialog said: closed, or the job a share arrives as.
static HEARD: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn heard(what: impl Into<String>) {
    caskade_core::lock(&HEARD).push(what.into());
}

fn heard_all() -> Vec<String> {
    caskade_core::lock(&HEARD).clone()
}

fn app() -> Element {
    let shown = caskade_core::lock(&SHOWN).clone().unwrap();
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            div { id: "share-ready",
                match shown {
                    Shown::Share { shares, entry_id } => rsx! {
                        ShareGameDialog {
                            shares,
                            target: ShareTarget::Game { entry_id, title: "Celeste".into() },
                            on_close: |_| heard("closed"),
                        }
                    },
                    Shown::Pair { pairing } => rsx! {
                        crate::flows::pairing::PairingHost { pairing }
                    },
                    Shown::Receive { shares } => rsx! {
                        ReceiveShareDialog {
                            shares,
                            on_close: |_| heard("closed"),
                            on_received: |job: String| heard(format!("job {job}")),
                        }
                    },
                }
            }
        }
    }
}

/// `shown` in a window, once its host is there.
fn open(shown: Shown) -> Window {
    *caskade_core::lock(&SHOWN) = Some(shown);
    caskade_core::lock(&HEARD).clear();
    Window::open_root(app, "the host", "#share-ready")
}

#[test]
fn a_link_is_made_to_a_game_and_stopping_it_ends_the_share() {
    let _one = caskade_core::lock(&ONE);
    let desk = core("Desk");
    let entry_id = add_celeste(&desk);
    let mut window = open(Shown::Share {
        shares: desk.handle(),
        entry_id,
    });
    assert_eq!(window.text("[role=dialog] h2"), "Share Celeste");
    assert!(
        window
            .text("[data-cs=share-game] p")
            .starts_with("Caskade makes a link. Whoever opens it")
    );
    window.press("#share-make");
    window.until_for("the link", 30, |w| {
        w.text("[data-part=link]")
            .starts_with("caskade://receive?t=")
    });
    let send = window.text("[data-cs=share-game] p");
    assert!(
        send.starts_with("Send this link to whoever you're sharing Celeste"),
        "{send}"
    );
    window.until_for("the share ready", 30, |w| {
        w.text("[data-part=state]")
            .starts_with("The link works until ")
    });
    assert_eq!(
        window.text("[role=status]:not([data-part])"),
        "Nobody has opened the link yet."
    );
    window.press("#share-stop");
    window.until("the dialog closed", |_| heard_all() == ["closed"]);
    let shares = desk.shares.clone();
    let list = caskade_app::runtime::block_on(async move { shares.list().await }).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].state, ShareState::Stopped);
}

#[test]
fn a_link_is_looked_at_and_the_game_received_from_it() {
    let _one = caskade_core::lock(&ONE);
    let desk = core("Desk");
    let friend = core("Friend's PC");
    let entry_id = add_celeste(&desk);
    let shares = desk.shares.clone();
    let link = caskade_app::runtime::block_on(async move {
        let share = shares.share(&entry_id, ShareExpiry::Week).await.unwrap();
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let ready = shares
                .list()
                .await
                .unwrap()
                .iter()
                .any(|record| record.id == share.id && record.state == ShareState::Ready);
            if ready {
                break;
            }
            assert!(Instant::now() < until, "the share's files are read");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        shares.link(&share.id).await.unwrap()
    });
    let mut window = open(Shown::Receive {
        shares: friend.handle(),
    });
    assert_eq!(window.text("[role=dialog] h2"), "Receive a shared game");
    window.focus("#share-link-input");
    window.type_text(&link);
    // Enter in the field looks, as the webview's form submits.
    window.key(Key::Named(NamedKey::Enter));
    window.until_for("what it shares", 30, |w| {
        w.find("[data-cs=share-info]").is_some()
    });
    assert_eq!(
        window.text("[data-cs=share-info] p"),
        "Desk shares Celeste with you."
    );
    let rows = window.text("[data-cs=share-info] dl");
    assert!(
        rows.contains("From")
            && rows.contains("Desk")
            && rows.contains("Windows (runs through Proton on Linux)"),
        "{rows}"
    );
    window.press("#share-receive");
    window.until_for("the job", 30, |_| {
        heard_all().iter().any(|what| what.starts_with("job "))
    });
    assert!(heard_all().contains(&"closed".to_owned()));
    let job = heard_all()
        .iter()
        .find_map(|what| what.strip_prefix("job ").map(str::to_owned))
        .unwrap();
    let install = friend.install.clone();
    let finished = caskade_app::runtime::block_on(async move {
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            let snapshot = install.job(&job).unwrap();
            if snapshot.status.is_finished() || Instant::now() > until {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    assert_eq!(finished.status, JobStatus::Completed, "{finished:?}");
    let _ = (&desk.devices, &friend.devices, PathBuf::new());
}

/// Two devices on a local network with no internet (mDNS only, as
/// caskade-core's pairing test): Desk asks to pair, and the TV's pairing
/// host shows the request and its code; both confirm, and it says so.
#[test]
fn a_pairing_by_code_is_shown_confirmed_and_finished() {
    if std::env::var_os("CASKADE_TEST_NO_MULTICAST").is_some() {
        eprintln!("skipped: CASKADE_TEST_NO_MULTICAST");
        return;
    }
    let _one = caskade_core::lock(&ONE);
    let service = format!("cskn{:08x}", std::process::id() ^ 0x3c3c);
    let lan = || Reach {
        lan: Some(service.clone()),
        ..Reach::loopback()
    };
    let desk = core_with("Desk", lan());
    let tv = core_with("Living room", lan());
    let id = |core: &Core| {
        let devices = core.devices.clone();
        caskade_app::runtime::block_on(async move { devices.status().await.this_device.id })
            .unwrap()
    };
    let (desk_id, tv_id) = (id(&desk), id(&tv));
    let desk_devices = desk.devices.clone();
    let wanted = tv_id.clone();
    caskade_app::runtime::block_on(async move {
        let until = Instant::now() + Duration::from_secs(20);
        while !desk_devices
            .status()
            .await
            .nearby
            .iter()
            .any(|device| device.id == wanted)
        {
            assert!(Instant::now() < until, "the desk sees the TV nearby");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    let mut window = open(Shown::Pair {
        pairing: caskade_ui_data::pairing::Pairing::new(
            tv.devices.clone(),
            tv.accounts.clone(),
            None,
        ),
    });
    assert!(
        window.find("[data-cs=pair-device]").is_none(),
        "nothing to show yet"
    );
    let desk_devices = desk.devices.clone();
    let asked = tv_id.clone();
    let pairing =
        caskade_app::runtime::block_on(async move { desk_devices.pair_nearby(&asked).await })
            .unwrap();
    window.until("the request", |w| w.find("[data-cs=pair-device]").is_some());
    assert_eq!(
        window.text("[data-cs=pair-device] h2"),
        "Desk wants to pair"
    );
    let code = format!("{} {}", &pairing.code[..3], &pairing.code[3..]);
    assert_eq!(window.text("[data-part=code]"), code);
    window.press("#pair-confirm");
    window.until("the TV's confirmation", |w| {
        w.text("[data-cs=pair-device] [role=status]")
            .starts_with("Waiting for Desk to confirm")
    });
    let desk_devices = desk.devices.clone();
    caskade_app::runtime::block_on(async move { desk_devices.confirm_pairing(&tv_id).await })
        .unwrap();
    window.until("paired", |w| {
        w.text("[data-cs=pair-device] h2") == "Paired with Desk"
    });
    assert!(
        window
            .text("[data-cs=pair-device]")
            .contains("The first sync starts now."),
        "{}",
        window.text("[data-cs=pair-device]")
    );
    window.press("#pair-close");
    window.until("the dialog gone", |w| {
        w.find("[data-cs=pair-device]").is_none()
    });
    let tv_devices = tv.devices.clone();
    let paired = caskade_app::runtime::block_on(async move { tv_devices.status().await.devices });
    assert!(paired.iter().any(|device| device.id == desk_id));
}
