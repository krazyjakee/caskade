//! Storage flows over the real core: an in-memory library with two
//! locations, and a game installed by the installer onto one of them; and
//! Add game's install items, installed on the location chosen.
// The installing tests run on Linux only; elsewhere their helpers go unused.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use super::*;

use crate::flows::storage::{InstallRootPicker, MoveGameAction, Needs};
use caskade_core::install::{InstallOptions, InstallService, JobStatus};
use caskade_core::storage::{RootSummary, StorageService};
use caskade_ui_data::storage::Storage;

/// What a storage test shows, with the locations it runs on.
#[derive(Clone)]
enum Shown {
    Picker {
        needs: Needs,
        always_use_default: Option<bool>,
    },
    Move {
        game_id: String,
        running: bool,
    },
}
static SHOWN: Mutex<Option<(Storage, Shown)>> = Mutex::new(None);
static CHOSEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn storage_app() -> Element {
    let (storage, shown) = caskade_core::lock(&SHOWN).clone().unwrap();
    use_context_provider(|| storage.clone());
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            div { id: "storage-ready",
                match shown {
                    Shown::Picker { needs, always_use_default } => rsx! {
                        InstallRootPicker {
                            storage: storage.clone(),
                            needs,
                            always_use_default,
                            on_select: |id: String| caskade_core::lock(&CHOSEN).push(id),
                            on_cancel: |_| caskade_core::lock(&CHOSEN).push("cancelled".into()),
                        }
                    },
                    Shown::Move { game_id, running } => rsx! {
                        MoveGameAction { game_id, is_running: running }
                    },
                }
            }
        }
    }
}

struct Locations {
    _temp: tempfile::TempDir,
    storage: Storage,
    service: StorageService,
    /// The default location ("Archive") and the other ("Fast SSD").
    archive: String,
    fast: String,
    /// The game installed on Fast SSD.
    game_id: Option<String>,
    installer: InstallService,
}

/// Two locations; with `install`, a game installed on the second.
fn locations(install: bool) -> Locations {
    caskade_app::runtime::block_on(async move {
        use caskade_core::{
            AppPaths, db::LibraryStore, launch::LaunchService, settings::SettingsService,
        };
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(&temp.path().join("app"));
        let store = LibraryStore::in_memory().await.unwrap();
        let settings = SettingsService::new(store.clone());
        settings.register_core(&paths).unwrap();
        let launch = LaunchService::new(store.clone()).with_aliases(None);
        let service = StorageService::new(store.clone(), settings.clone(), launch)
            .unwrap()
            .with_paths(paths);
        let archive = service
            .register_root(temp.path().join("one"), Some("Archive".into()))
            .await
            .unwrap()
            .root
            .id;
        let fast = service
            .register_root(temp.path().join("two"), Some("Fast SSD".into()))
            .await
            .unwrap()
            .root
            .id;
        let installer = InstallService::new(
            store.clone(),
            caskade_core::bus::Bus::default(),
            InstallOptions {
                downloads: 1,
                aliases: None,
            },
        )
        .with_storage(service.clone());
        let mut game_id = None;
        if install {
            let folder = temp.path().join("sources").join("Starfall");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("data.pak"), vec![7u8; 2048]).unwrap();
            std::fs::copy(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                    "../../crates/caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64",
                ),
                folder.join("Starfall.x86_64"),
            )
            .unwrap();
            let job = installer
                .install(folder.to_string_lossy().into_owned(), Some(fast.clone()))
                .await
                .unwrap();
            let mut snapshot = job;
            for _ in 0..800 {
                if snapshot.status.is_finished() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
                snapshot = installer.job(&snapshot.id).unwrap();
            }
            assert_eq!(snapshot.status, JobStatus::Completed, "{snapshot:?}");
            game_id = snapshot.game_id.clone();
        }
        Locations {
            _temp: temp,
            storage: Storage::new(service.clone(), Some(installer.clone()), settings),
            installer,
            service,
            archive,
            fast,
            game_id,
        }
    })
}

fn show(locations: &Locations, shown: Shown) -> Window {
    *caskade_core::lock(&SHOWN) = Some((locations.storage.clone(), shown));
    caskade_core::lock(&CHOSEN).clear();
    Window::open_root(storage_app, "the flow", "#storage-ready")
}

fn chosen() -> Vec<String> {
    caskade_core::lock(&CHOSEN).clone()
}

fn roots(service: &StorageService) -> Vec<RootSummary> {
    let service = service.clone();
    caskade_app::runtime::block_on(async move { service.list_roots().await }).unwrap()
}

#[test]
fn the_install_picker_starts_on_the_default_and_installs_there() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(false);
    let mut window = show(
        &locations,
        Shown::Picker {
            needs: Needs::default(),
            always_use_default: Some(false),
        },
    );
    window.until("the locations", |w| {
        w.find("[data-cs=install-root-picker] h2").is_some()
    });
    assert_eq!(
        window.text("[data-cs=install-root-picker] h2"),
        "Install location"
    );
    let estimate = window.text("[data-cs=install-root-picker] p");
    assert!(
        estimate.starts_with("Estimated space: Unknown"),
        "{estimate}"
    );
    let options = window.text("#root-options");
    assert!(
        options.contains("Archive · Default") && options.contains("Fast SSD"),
        "{options}"
    );
    assert!(options.contains(" free"), "{options}");
    window.press("#root-install");
    window.until("a choice", |_| !chosen().is_empty());
    assert_eq!(chosen(), std::slice::from_ref(&locations.archive));
}

#[test]
fn a_location_without_room_is_refused_with_the_reason() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(false);
    let mut window = show(
        &locations,
        Shown::Picker {
            needs: Needs {
                estimated_bytes: Some(u64::MAX / 2),
                ..Needs::default()
            },
            always_use_default: Some(false),
        },
    );
    window.until("the locations", |w| {
        w.find("[data-cs=install-root-picker] h2").is_some()
    });
    let estimate = window.text("[data-cs=install-root-picker] p");
    assert!(
        estimate.starts_with("Estimated space: ") && !estimate.contains("Unknown"),
        "{estimate}"
    );
    assert!(
        window
            .text("#root-options")
            .contains("Not enough free space")
    );
    // Nothing can take it: no location chosen, and Install here refuses.
    assert_eq!(
        window.text("[data-cs=install-root-picker] [role=alert]"),
        "Add a storage location in Settings → Storage."
    );
    assert_eq!(
        window.attr("#root-install", "aria-disabled").as_deref(),
        Some("true")
    );
    window.press("#root-cancel");
    window.until("the cancel", |_| !chosen().is_empty());
    assert_eq!(chosen(), ["cancelled"]);
}

#[test]
fn always_using_the_default_skips_the_picker() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(false);
    let mut window = show(
        &locations,
        Shown::Picker {
            needs: Needs::default(),
            always_use_default: Some(true),
        },
    );
    window.until("a choice", |_| !chosen().is_empty());
    assert_eq!(chosen(), std::slice::from_ref(&locations.archive));
    let _ = locations.fast;
}

#[cfg(target_os = "linux")]
#[test]
fn a_game_moves_to_another_location_and_its_files_go_with_it() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(true);
    let game_id = locations.game_id.clone().unwrap();
    let on = |root: &str| {
        roots(&locations.service)
            .into_iter()
            .find(|summary| summary.root.id == root)
            .unwrap()
            .games
            .iter()
            .any(|game| game.game_id == game_id)
    };
    assert!(on(&locations.fast) && !on(&locations.archive));
    let mut window = show(
        &locations,
        Shown::Move {
            game_id: game_id.clone(),
            running: false,
        },
    );
    window.press("#move-game");
    window.until("the destination", |w| w.find("#move-destination").is_some());
    assert_eq!(window.text("[role=dialog] h2"), "Move installed files");
    let line = window.text("[role=dialog] p.text-sm");
    assert!(
        line.starts_with("Starfall · ")
            && line.ends_with(". Playtime and library identity are kept."),
        "{line}"
    );
    assert!(window.text("#move-destination").contains("Archive"));
    assert!(
        window.find("[data-part=reason]").is_none(),
        "nothing blocks it"
    );
    window.press("#move-confirm");
    window.until("the move to finish", |w| w.find("[role=dialog]").is_none());
    assert!(
        on(&locations.archive) && !on(&locations.fast),
        "the files moved"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_running_game_isnt_moved() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(true);
    let game_id = locations.game_id.clone().unwrap();
    let window = show(
        &locations,
        Shown::Move {
            game_id,
            running: true,
        },
    );
    // The button itself is off while the game runs, as in the webview.
    assert_eq!(
        window.attr("#move-game", "aria-disabled").as_deref(),
        Some("true")
    );
}

#[test]
fn a_game_caskade_didnt_install_says_why_it_cant_move() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(false);
    let mut window = show(
        &locations,
        Shown::Move {
            game_id: "not-managed".into(),
            running: false,
        },
    );
    window.press("#move-game");
    window.until("the reason", |w| w.find("[data-part=reason]").is_some());
    assert_eq!(
        window.text("[data-part=reason]"),
        "Only Caskade-managed installations can be moved."
    );
    assert_eq!(
        window.text("[role=dialog] p.text-sm"),
        "Games owned by other launchers stay in their launcher locations."
    );
    assert_eq!(
        window.attr("#move-confirm", "aria-disabled").as_deref(),
        Some("true")
    );
    window.press("#move-cancel");
    window.until("the dialog closed", |w| w.find("[role=dialog]").is_none());
}

// Add game's install items: a link (here a local folder, so nothing goes
// to the network) installs on the location chosen.

static ADD_STORAGE: Mutex<Option<Storage>> = Mutex::new(None);

fn add_with_storage_app() -> Element {
    use_data_source(|| source().clone());
    let storage = caskade_core::lock(&ADD_STORAGE).clone().unwrap();
    use_context_provider(|| storage);
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            AddGame {
                on_open_game: |id: String| caskade_core::lock(&OPENED).push(id),
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_game_installed_from_a_link_goes_where_the_user_chose() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(false);
    let folder = locations._temp.path().join("downloads").join("Moonrise");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("data.pak"), vec![3u8; 1024]).unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"),
        folder.join("Moonrise.x86_64"),
    )
    .unwrap();
    *caskade_core::lock(&ADD_STORAGE) = Some(locations.storage.clone());
    let mut window = Window::open_root(
        add_with_storage_app,
        "the Add game button",
        "#add-game-button",
    );
    // Install from file, from folder, from link; then the manual items.
    window.focus("#add-game-button");
    window.key(Key::Named(NamedKey::ArrowDown));
    window.until("the menu", |w| w.find("[role=menu]").is_some());
    let menu = window.text("[role=menu]");
    for item in [
        "Install from file…",
        "Install from folder…",
        "Install from link…",
        "Add a game on this computer…",
        "Add a game by title…",
    ] {
        assert!(menu.contains(item), "{item} in {menu}");
    }
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(Key::Named(NamedKey::Enter));
    window.until("the link dialog", |w| {
        w.find("[data-cs=install-url]").is_some()
    });
    assert_eq!(window.text("[data-cs=install-url] h2"), "Install from link");
    assert_eq!(
        window.attr("#install-url-add", "aria-disabled").as_deref(),
        Some("true")
    );
    window.focus("#install-url-input");
    window.type_text(&folder.to_string_lossy());
    assert_eq!(window.text("#install-url-add"), "Add installation");
    window.press("#install-url-add");
    // Where it goes: the default first.
    window.until("the location picker", |w| {
        w.find("[data-cs=install-root-picker]").is_some()
    });
    assert!(
        window.find("[data-cs=install-url]").is_none(),
        "the link dialog closed"
    );
    window.press("#root-install");
    window.until("the picker closed", |w| {
        w.find("[data-cs=install-root-picker]").is_none()
    });
    let installer = locations.installer.clone();
    let mut job = None;
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        window.settle();
        job = caskade_app::commands::install::jobs_list(&installer)
            .into_iter()
            .find(|job| job.status.is_finished());
        if job.is_some() {
            break;
        }
    }
    let job = job.expect("the install finished");
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    assert_eq!(job.root_id.as_deref(), Some(locations.archive.as_str()));
}
