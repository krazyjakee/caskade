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

/// The picker with the language followed, as the app's root does.
fn speaking_storage_app() -> Element {
    crate::i18n::use_language();
    storage_app()
}

#[test]
fn every_control_in_the_install_picker_has_a_name() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(false);
    let mut window = show(
        &locations,
        Shown::Picker {
            needs: Needs::default(),
            always_use_default: Some(false),
        },
    );
    window.until("the locations", |w| w.find("#root-options").is_some());
    window.settle();
    // The picker sits in whatever asks for a location (the install dialog
    // in the app), here on its own: its own tree is checked.
    let picker = window.find("[data-cs=install-root-picker]").unwrap();
    let tree = crate::a11y_snapshot::snapshot_of(&window.doc.inner.borrow(), picker);
    let unnamed = crate::frame::a11y_tests::unnamed(&tree);
    assert!(
        unnamed.is_empty(),
        "the install picker: controls a screen reader can't name:\n{}",
        unnamed.join("\n")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn every_control_in_the_move_dialog_has_a_name() {
    let _one = caskade_core::lock(&ONE);
    let locations = locations(true);
    let game_id = locations.game_id.clone().unwrap();
    let mut window = show(
        &locations,
        Shown::Move {
            game_id,
            running: false,
        },
    );
    window.press("#move-game");
    window.until("the destination", |w| w.find("#move-destination").is_some());
    window.settle();
    names_every_control(&window, "the move dialog");
}

/// A language switch while the picker is open (#153): its words follow,
/// in place.
#[test]
fn the_install_picker_follows_a_language_switch() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let locations = locations(false);
    *caskade_core::lock(&SHOWN) = Some((
        locations.storage.clone(),
        Shown::Picker {
            needs: Needs::default(),
            always_use_default: Some(false),
        },
    ));
    caskade_core::lock(&CHOSEN).clear();
    let mut window = Window::open_root(speaking_storage_app, "the flow", "#storage-ready");
    window.until("the locations", |w| {
        w.text("[data-cs=install-root-picker] h2") == "Install location"
    });
    assert_eq!(window.text("#root-install").trim(), "Install here");
    speak(&mut window, "sv", |w| {
        w.text("[data-cs=install-root-picker] h2") == "Installationsplats"
    });
    assert_eq!(window.text("#root-install").trim(), "Installera här");
    assert_eq!(window.text("#root-cancel").trim(), "Avbryt");
    // Still the same choice: nothing was picked or cancelled.
    assert!(chosen().is_empty());
    speak(&mut window, "en", |w| {
        w.text("[data-cs=install-root-picker] h2") == "Install location"
    });
}

/// The install picker by a pad alone (#153): a pad enters on the default
/// location; down to Fast SSD and A chooses it; Install here, reached by a
/// pad's moves, installs there with A. Another picker's Cancel by pad.
#[test]
fn the_install_picker_goes_by_a_pad_alone() {
    use crate::frame::pad_tests::{attr_of, reach};
    use caskade_ui_input::Action;
    let _one = caskade_core::lock(&ONE);
    let locations = locations(false);
    let picker = Shown::Picker {
        needs: Needs::default(),
        always_use_default: Some(false),
    };
    let mut window = show(&locations, picker.clone());
    window.until("the locations", |w| w.find("#root-options").is_some());
    let root = window.find("[data-cs=install-root-picker]").unwrap();
    let option = |w: &Window, node: NodeId, name: &str| {
        attr_of(w, node, "role").as_deref() == Some("radio")
            && w.doc
                .inner
                .borrow()
                .get_node(node)
                .is_some_and(|node| node.text_content().contains(name))
    };
    reach(&mut window, root, "Fast SSD", |w, node| {
        option(w, node, "Fast SSD")
    });
    window.pad(Action::Confirm);
    window.until("Fast SSD chosen", |w| {
        w.doc
            .inner
            .borrow()
            .query_selector_all("[role=radio][aria-checked=true]")
            .map(|nodes| nodes.to_vec())
            .unwrap_or_default()
            .into_iter()
            .any(|node| option(w, node, "Fast SSD"))
    });
    reach(&mut window, root, "Install here", |w, node| {
        attr_of(w, node, "id").as_deref() == Some("root-install")
    });
    window.pad(Action::Confirm);
    window.until("a choice", |_| !chosen().is_empty());
    assert_eq!(chosen(), std::slice::from_ref(&locations.fast));
    drop(window);
    let mut window = show(&locations, picker);
    window.until("the locations", |w| w.find("#root-options").is_some());
    let root = window.find("[data-cs=install-root-picker]").unwrap();
    reach(&mut window, root, "Cancel", |w, node| {
        attr_of(w, node, "id").as_deref() == Some("root-cancel")
    });
    window.pad(Action::Confirm);
    window.until("the cancel", |_| !chosen().is_empty());
    assert_eq!(chosen(), ["cancelled"]);
}

/// Moving a game by keys alone and by a pad alone (#153). Keys: Tab to
/// Move installed files… and Enter opens the dialog, Escape closes it;
/// Enter again, Tab to Move game and Enter moves it. A pad: A opens the
/// dialog, B closes it, A again, Move game reached by a pad's moves and A
/// moves it back.
#[cfg(target_os = "linux")]
#[test]
fn a_game_moves_by_keys_alone_and_by_a_pad_alone() {
    use crate::frame::pad_tests::{attr_of, focused_node, reach};
    use caskade_ui_input::Action;
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
    let shown = Shown::Move {
        game_id: game_id.clone(),
        running: false,
    };
    let focused_id = |w: &Window| focused_node(w).and_then(|node| attr_of(w, node, "id"));
    let tab_to = |w: &mut Window, id: &str| {
        for _ in 0..20 {
            if focused_id(&*w).as_deref() == Some(id) {
                return;
            }
            w.key(Key::Named(NamedKey::Tab));
        }
        panic!("Tab never reached #{id}; focus is on {:?}", focused_id(&*w));
    };
    // Keys.
    let mut window = show(&locations, shown.clone());
    tab_to(&mut window, "move-game");
    window.key(Key::Named(NamedKey::Enter));
    window.until("the dialog", |w| w.find("#move-destination").is_some());
    window.key(Key::Named(NamedKey::Escape));
    window.until("the dialog closed", |w| w.find("[role=dialog]").is_none());
    tab_to(&mut window, "move-game");
    window.key(Key::Named(NamedKey::Enter));
    window.until("the dialog again", |w| {
        w.find("#move-destination").is_some() && w.find("[data-part=reason]").is_none()
    });
    tab_to(&mut window, "move-confirm");
    window.key(Key::Named(NamedKey::Enter));
    window.until("the move to finish", |w| w.find("[role=dialog]").is_none());
    assert!(
        on(&locations.archive) && !on(&locations.fast),
        "moved by keys"
    );
    drop(window);
    // A pad, back to Fast SSD.
    let mut window = show(&locations, shown);
    let start = window.find("#storage-ready").unwrap();
    reach(&mut window, start, "Move installed files…", |w, node| {
        attr_of(w, node, "id").as_deref() == Some("move-game")
    });
    window.pad(Action::Confirm);
    window.until("the dialog", |w| w.find("#move-destination").is_some());
    window.pad(Action::Back);
    window.until("the dialog closed", |w| w.find("[role=dialog]").is_none());
    assert_eq!(
        focused_id(&window).as_deref(),
        Some("move-game"),
        "focus came back"
    );
    window.pad(Action::Confirm);
    window.until("the dialog again", |w| {
        w.find("#move-destination").is_some() && w.find("[data-part=reason]").is_none()
    });
    let dialog = window.find("[role=dialog]").unwrap();
    reach(&mut window, dialog, "Move game", |w, node| {
        attr_of(w, node, "id").as_deref() == Some("move-confirm")
    });
    window.pad(Action::Confirm);
    window.until("the move to finish", |w| w.find("[role=dialog]").is_none());
    assert!(
        on(&locations.fast) && !on(&locations.archive),
        "moved back by pad"
    );
}
