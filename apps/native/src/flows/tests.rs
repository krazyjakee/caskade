//! The flows driven end to end in a Blitz document without a window, over
//! the fixture library (the real core and app layer, in memory), with the
//! keyboard as a user would: the button, its menu, the dialog's fields and
//! buttons. What the core ends up holding is checked through the library.

use super::add_game::AddGame;
use crate::data::{DataSource, use_data_source};
use crate::input::InputRoot;
use crate::input::tests::ONE_WINDOW as ONE;
use blitz_dom::{DocumentConfig, NodeId};
use blitz_traits::events::{BlitzKeyEvent, KeyState, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::model::{LaunchSpec, ListGames};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use dioxus_native::prelude::*;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static SOURCE: OnceLock<DataSource> = OnceLock::new();
/// The games the flows asked to open, in order.
static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn source() -> &'static DataSource {
    SOURCE.get_or_init(|| caskade_app::runtime::block_on(DataSource::fixtures(12)).unwrap())
}

fn app() -> Element {
    use_data_source(|| source().clone());
    rsx! {
        style { {include_str!("../../assets/app.css")} }
        InputRoot {
            AddGame {
                on_open_game: |id: String| OPENED.lock().unwrap_or_else(|e| e.into_inner()).push(id),
            }
        }
    }
}

struct Window {
    doc: DioxusDocument,
}

impl Window {
    fn open() -> Self {
        source();
        OPENED.lock().unwrap_or_else(|e| e.into_inner()).clear();
        Self::open_root(app, "the Add game button", "#add-game-button")
    }

    /// `root` in a window, settled until `ready` (a selector) is there.
    fn open_root(root: fn() -> Element, what: &str, ready: &str) -> Self {
        let mut doc = DioxusDocument::new(
            VirtualDom::new(root),
            DocumentConfig {
                viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                ..Default::default()
            },
        );
        doc.initial_build();
        let mut window = Self { doc };
        window.until(what, |w| w.find(ready).is_some());
        window
    }

    /// A pad's action, as the app's events bring it.
    fn pad(&mut self, action: caskade_ui_input::Action) {
        use caskade_app::commands::input::InputActionEvent;
        use caskade_input::{InputAction, Source};
        let _ = crate::shell::events().send(caskade_app::events::Event::InputActionEvent(
            InputActionEvent(InputAction {
                action,
                source: Source::Pad,
                repeat: false,
                glyphs: caskade_ui_input::GlyphFamily::Xbox,
            }),
        ));
        self.settle();
    }

    fn settle(&mut self) {
        for _ in 0..12 {
            blitz_dom::Document::poll(&mut self.doc, None);
            self.doc.inner.borrow_mut().resolve(0.0);
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    /// Settles until `done`, or fails naming `what` it waited for.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            self.settle();
            if done(self) {
                self.settle();
                return;
            }
        }
        panic!("never saw {what}");
    }

    fn find(&self, selector: &str) -> Option<NodeId> {
        self.doc
            .inner
            .borrow()
            .query_selector(selector)
            .ok()
            .flatten()
    }

    fn count(&self, selector: &str) -> usize {
        self.doc
            .inner
            .borrow()
            .query_selector_all(selector)
            .map(|nodes| nodes.len())
            .unwrap_or(0)
    }

    fn text(&self, selector: &str) -> String {
        let Some(node) = self.find(selector) else {
            return String::new();
        };
        let doc = self.doc.inner.borrow();
        doc.get_node(node)
            .map(|node| node.text_content())
            .unwrap_or_default()
    }

    /// The text of every element `selector` finds, in document order.
    fn texts(&self, selector: &str) -> Vec<String> {
        let doc = self.doc.inner.borrow();
        doc.query_selector_all(selector)
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|&node| doc.get_node(node).map(|node| node.text_content()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn attr(&self, selector: &str, name: &str) -> Option<String> {
        let node = self.find(selector)?;
        let doc = self.doc.inner.borrow();
        doc.get_node(node)?
            .attr(blitz_dom::LocalName::from(name))
            .map(str::to_owned)
    }

    fn focus(&mut self, selector: &str) {
        let node = self
            .find(selector)
            .unwrap_or_else(|| panic!("no {selector}"));
        self.doc.inner.borrow_mut().set_focus_to(node);
        self.settle();
    }

    fn key_event(&mut self, key: Key, state: KeyState) {
        let text = match &key {
            Key::Character(text) if state == KeyState::Pressed => Some(text.clone().into()),
            _ => None,
        };
        let event = BlitzKeyEvent {
            key,
            code: Code::Unidentified,
            modifiers: Modifiers::empty(),
            location: Location::Standard,
            is_auto_repeating: false,
            is_composing: false,
            state,
            text,
        };
        let event = match state {
            KeyState::Pressed => UiEvent::KeyDown(event),
            KeyState::Released => UiEvent::KeyUp(event),
        };
        blitz_dom::Document::handle_ui_event(&mut self.doc, event);
    }

    fn key(&mut self, key: Key) {
        self.key_event(key.clone(), KeyState::Pressed);
        self.settle();
        self.key_event(key, KeyState::Released);
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            self.key_event(Key::Character(character.to_string()), KeyState::Pressed);
            self.key_event(Key::Character(character.to_string()), KeyState::Released);
        }
        self.settle();
    }

    /// Opens Add game's menu from the keyboard and chooses its `index`th item.
    fn choose(&mut self, index: usize) {
        self.focus("#add-game-button");
        self.key(Key::Named(NamedKey::ArrowDown));
        self.until("the menu", |w| w.find("[role=menu]").is_some());
        for _ in 0..index {
            self.key(Key::Named(NamedKey::ArrowDown));
        }
        self.key(Key::Named(NamedKey::Enter));
        self.until("the dialog", |w| w.find("[data-cs=manual-game]").is_some());
    }

    /// Presses a button from the keyboard.
    fn press(&mut self, selector: &str) {
        self.focus(selector);
        self.key(Key::Named(NamedKey::Enter));
    }

    fn opened(&self) -> Vec<String> {
        OPENED.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn game_title(id: &str) -> Option<String> {
    let library = source().library.clone();
    let id = id.to_owned();
    caskade_app::runtime::block_on(async move { library.game(&id).await })
        .unwrap()
        .map(|game| game.game.title)
}

/// A fixture game's title, for a duplicate.
fn a_fixture_title() -> String {
    let library = source().library.clone();
    let page = caskade_app::runtime::block_on(async move {
        library
            .page(&ListGames {
                limit: 1,
                ..ListGames::default()
            })
            .await
    })
    .unwrap();
    page.items[0].title.clone()
}

#[test]
fn a_game_by_title_is_added_and_opened() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let mut window = Window::open();
    window.choose(1);
    assert_eq!(
        window.attr("[data-cs=manual-game]", "data-mode").as_deref(),
        Some("title")
    );
    assert_eq!(
        window.text("[data-cs=manual-game] h2"),
        "Add a game by title"
    );
    assert!(
        window.find("[data-part=target]").is_none(),
        "no target to choose"
    );
    // Nothing to add without a title.
    assert!(window.attr("#manual-add", "aria-disabled").as_deref() == Some("true"));
    window.focus("#manual-title-input");
    window.type_text("Shelf Cartridge Quest");
    assert_eq!(window.text("#manual-add"), "Add game");
    window.press("#manual-add");
    window.until("the game opened", |w| !w.opened().is_empty());
    assert!(
        window.find("[data-cs=manual-game]").is_none(),
        "the dialog closed"
    );
    let id = window.opened()[0].clone();
    assert_eq!(game_title(&id).as_deref(), Some("Shelf Cartridge Quest"));
}

#[test]
fn a_title_already_in_the_library_asks_and_can_be_kept_apart() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let title = a_fixture_title();
    let mut window = Window::open();
    window.choose(1);
    window.focus("#manual-title-input");
    window.type_text(&title);
    // Enter in the title adds, as the webview's form submits.
    window.key(Key::Named(NamedKey::Enter));
    window.until("the duplicates", |w| {
        w.find("[data-part=duplicates]").is_some()
    });
    assert_eq!(
        window.text("[data-part=duplicates] h3"),
        "This may already be in your library"
    );
    let line = window.text("[data-part=duplicate]");
    assert!(
        line.contains(&title) && line.contains("has the same title"),
        "{line}"
    );
    // A title-only record has nothing to join to a game.
    assert_eq!(window.count("[data-part=link]"), 0);
    assert_eq!(window.text("#manual-add"), "Keep separate");
    assert!(window.opened().is_empty(), "nothing added yet");
    window.press("#manual-add");
    window.until("the game opened", |w| !w.opened().is_empty());
    let id = window.opened()[0].clone();
    assert_eq!(game_title(&id).as_deref(), Some(title.as_str()));
}

#[test]
fn a_uri_is_resolved_summarised_added_and_then_offered_to_join() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let uri = "steam://rungameid/620";
    let mut window = Window::open();
    window.choose(0);
    assert_eq!(
        window.attr("[data-cs=manual-game]", "data-mode").as_deref(),
        Some("target")
    );
    assert_eq!(
        window.text("[data-cs=manual-game] h2"),
        "Add a game on this computer"
    );
    window.focus("#manual-target-input");
    window.type_text(uri);
    // Leaving the field resolves it, as the webview's blur does.
    window.key(Key::Named(NamedKey::Tab));
    window.until("the summary", |w| w.find("[data-part=summary]").is_some());
    let summary = window.text("[data-part=summary]");
    assert!(
        summary.contains("Opens") && summary.contains(uri),
        "{summary}"
    );
    // A URI suggests no title.
    assert!(window.attr("#manual-add", "aria-disabled").as_deref() == Some("true"));
    window.focus("#manual-title-input");
    window.type_text("Portal Two By Hand");
    window.press("#manual-add");
    window.until("the game opened", |w| !w.opened().is_empty());
    let first = window.opened()[0].clone();
    let library = source().library.clone();
    let game = {
        let first = first.clone();
        caskade_app::runtime::block_on(async move { library.game(&first).await })
            .unwrap()
            .unwrap()
    };
    assert!(
        game.entries.iter().any(
            |entry| matches!(&entry.launch, Some(LaunchSpec::Uri { uri: found }) if found == uri)
        ),
        "the entry starts the URI"
    );

    // The same URI again: it may be that game, and can join it.
    window.choose(0);
    window.focus("#manual-target-input");
    window.type_text(uri);
    window.key(Key::Named(NamedKey::Enter));
    window.until("the summary", |w| w.find("[data-part=summary]").is_some());
    window.focus("#manual-title-input");
    window.type_text("Portal Two Again");
    window.press("#manual-add");
    window.until("the duplicates", |w| {
        w.find("[data-part=duplicate]").is_some()
    });
    let line = window.text("[data-part=duplicate]");
    assert!(line.contains("starts the same program"), "{line}");
    window.press(&format!("#manual-link-{first}"));
    window.until("the game opened", |w| w.opened().len() == 2);
    assert_eq!(window.opened()[1], first, "joined to the first game");
}

#[test]
fn a_target_that_cant_start_a_game_says_why() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let mut window = Window::open();
    window.choose(0);
    window.focus("#manual-target-input");
    window.type_text("/no/such/game.sh");
    window.key(Key::Named(NamedKey::Enter));
    window.until("the error", |w| w.find("[data-part=error]").is_some());
    assert_eq!(
        window.attr("[data-part=error]", "role").as_deref(),
        Some("alert")
    );
    assert!(!window.text("[data-part=error]").is_empty());
    assert!(window.find("[data-part=summary]").is_none());
    window.focus("#manual-title-input");
    window.type_text("Ghost");
    assert!(
        window.attr("#manual-add", "aria-disabled").as_deref() == Some("true"),
        "nothing to start"
    );
    // Cancel closes it with nothing added.
    window.press("#manual-cancel");
    window.until("the dialog closed", |w| {
        w.find("[data-cs=manual-game]").is_none()
    });
    assert!(window.opened().is_empty());
}

// The path browser (console mode's picker), over a folder of its own.

use super::paths::{PathPickerProvider, PathRequest, PickKind, use_path_picker};

/// What the picker was asked, and what it answered (`Some(None)`: cancelled).
static REQUEST: Mutex<Option<PathRequest>> = Mutex::new(None);
static ANSWER: Mutex<Option<Option<String>>> = Mutex::new(None);

fn picker_app() -> Element {
    rsx! {
        style { {include_str!("../../assets/app.css")} }
        InputRoot {
            PathPickerProvider { in_app: true, Asker {} }
        }
    }
}

#[component]
fn Asker() -> Element {
    let picker = use_path_picker();
    use_hook(move || {
        let request = REQUEST.lock().unwrap_or_else(|e| e.into_inner()).clone();
        spawn(async move {
            if let Some(request) = request {
                let answer = picker.pick(request).await;
                *ANSWER.lock().unwrap_or_else(|e| e.into_inner()) = Some(answer);
            }
        });
    });
    rsx! { div { id: "asker" } }
}

/// `root/{alpha/, beta/, game.sh, notes.txt}`.
fn tree() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("alpha")).unwrap();
    std::fs::create_dir(root.path().join("beta")).unwrap();
    std::fs::write(root.path().join("game.sh"), "#!/bin/sh\n").unwrap();
    std::fs::write(root.path().join("notes.txt"), "notes").unwrap();
    root
}

fn browse(request: PathRequest) -> Window {
    *REQUEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(request);
    *ANSWER.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let mut window = Window::open_root(picker_app, "the browser", "[data-cs=folder-browser]");
    window.until("the listing", |w| w.count("[data-part=entry]") > 0);
    window
}

fn answer() -> Option<Option<String>> {
    ANSWER.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn path(of: &std::path::Path) -> String {
    of.to_string_lossy().into_owned()
}

#[test]
fn the_browser_opens_folders_goes_up_and_picks_the_folder_shown() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let root = tree();
    let mut request = PathRequest::new(PickKind::Folder, "Choose a library folder".into());
    request.start_at = Some(path(root.path()));
    let mut window = browse(request);
    assert_eq!(
        window
            .attr("[data-cs=folder-browser]", "data-kind")
            .as_deref(),
        Some("folder")
    );
    assert_eq!(
        window.text("[data-cs=folder-browser] h2"),
        "Choose a library folder"
    );
    assert_eq!(window.text("[data-part=location]"), path(root.path()));
    // Folders first; files can't be picked in folder mode.
    let names = window.texts("[data-part=entry] [data-part=name]");
    assert_eq!(names, ["alpha", "beta", "game.sh", "notes.txt"]);
    assert_eq!(window.count("[data-part=entry][data-disabled]"), 2);
    // Focus lands on the first folder; Enter opens it.
    assert_eq!(
        window
            .attr("[data-part=entry][data-focus-default]", "data-kind")
            .as_deref(),
        Some("folder")
    );
    window.focus("[data-part=entry][data-focus-default]");
    window.key(Key::Named(NamedKey::Enter));
    let alpha = path(&root.path().join("alpha"));
    window.until("alpha", |w| w.text("[data-part=location]") == alpha);
    assert_eq!(window.text("[data-part=empty]"), "This folder is empty.");
    // Up lands back on the folder just left.
    window.focus("[data-part=up]");
    window.key(Key::Named(NamedKey::Enter));
    window.until("the root again", |w| {
        w.text("[data-part=location]") == path(root.path())
    });
    assert_eq!(
        window.text("[data-part=entry][data-focus-default] [data-part=name]"),
        "alpha"
    );
    window.press("#folder-select");
    window.until("an answer", |_| answer().is_some());
    assert_eq!(answer(), Some(Some(path(root.path()))));
    assert!(
        window.find("[data-cs=folder-browser]").is_none(),
        "it closed"
    );
}

#[test]
fn in_file_mode_only_files_it_takes_can_be_picked() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let root = tree();
    let mut request = PathRequest::new(PickKind::File, "The game's program or shortcut".into());
    request.start_at = Some(path(root.path()));
    request.extensions = vec!["sh".into()];
    let mut window = browse(request);
    assert_eq!(
        window
            .attr("[data-cs=folder-browser]", "data-kind")
            .as_deref(),
        Some("file")
    );
    assert!(
        window.find("#folder-select").is_none(),
        "no Select this folder"
    );
    assert_eq!(
        window.count("[data-part=entry][data-disabled]"),
        1,
        "notes.txt"
    );
    let game = path(&root.path().join("game.sh"));
    window.focus("[data-part=entry][data-kind=file]:not([data-disabled])");
    window.key(Key::Named(NamedKey::Enter));
    window.until("an answer", |_| answer().is_some());
    assert_eq!(answer(), Some(Some(game)));
}

#[test]
fn a_pad_picks_the_folder_with_start_and_cancels_with_b() {
    use caskade_ui_input::Action;
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let root = tree();
    let mut request = PathRequest::new(PickKind::Folder, "Choose a library folder".into());
    request.start_at = Some(path(&root.path().join("beta")));
    let mut window = browse_empty(request.clone());
    window.focus("[data-part=up]");
    window.pad(Action::Menu);
    window.until("an answer", |_| answer().is_some());
    assert_eq!(answer(), Some(Some(path(&root.path().join("beta")))));

    request.start_at = Some(path(root.path()));
    let mut window = browse(request);
    window.focus("[data-part=entry][data-focus-default]");
    window.pad(Action::Back);
    window.until("an answer", |_| answer().is_some());
    assert_eq!(answer(), Some(None), "cancelled");
}

/// As [`browse`], for a folder with nothing in it.
fn browse_empty(request: PathRequest) -> Window {
    *REQUEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(request);
    *ANSWER.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let mut window = Window::open_root(picker_app, "the browser", "[data-cs=folder-browser]");
    window.until("the listing", |w| w.find("[data-part=empty]").is_some());
    window
}

#[test]
fn places_find_the_longest_match() {
    use super::folder_browser::current_place;
    use caskade_core::browse::Place;
    let places = [
        Place {
            label: "Home".into(),
            path: "/home/me".into(),
        },
        Place {
            label: "/".into(),
            path: "/".into(),
        },
        Place {
            label: "Games".into(),
            path: "/home/me/games/".into(),
        },
    ];
    assert_eq!(current_place(&places, "/home/me/games/x"), Some(2));
    assert_eq!(current_place(&places, "/home/me/docs"), Some(0));
    assert_eq!(current_place(&places, "/etc"), Some(1));
    assert_eq!(current_place(&places[..1], "/etc"), None);
}

// Storage flows over the real core: an in-memory library with two
// locations, and a game installed by the installer onto one of them.

use super::storage::{InstallRootPicker, MoveGameAction, Needs};
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
    let (storage, shown) = SHOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap();
    use_context_provider(|| storage.clone());
    rsx! {
        style { {include_str!("../../assets/app.css")} }
        InputRoot {
            div { id: "storage-ready",
                match shown {
                    Shown::Picker { needs, always_use_default } => rsx! {
                        InstallRootPicker {
                            storage: storage.clone(),
                            needs,
                            always_use_default,
                            on_select: |id: String| CHOSEN.lock().unwrap_or_else(|e| e.into_inner()).push(id),
                            on_cancel: |_| CHOSEN.lock().unwrap_or_else(|e| e.into_inner()).push("cancelled".into()),
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
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = Some((locations.storage.clone(), shown));
    CHOSEN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    Window::open_root(storage_app, "the flow", "#storage-ready")
}

fn chosen() -> Vec<String> {
    CHOSEN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn roots(service: &StorageService) -> Vec<RootSummary> {
    let service = service.clone();
    caskade_app::runtime::block_on(async move { service.list_roots().await }).unwrap()
}

#[test]
fn the_install_picker_starts_on_the_default_and_installs_there() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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
    let storage = ADD_STORAGE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap();
    use_context_provider(|| storage);
    rsx! {
        style { {include_str!("../../assets/app.css")} }
        InputRoot {
            AddGame {
                on_open_game: |id: String| OPENED.lock().unwrap_or_else(|e| e.into_inner()).push(id),
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_game_installed_from_a_link_goes_where_the_user_chose() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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
    *ADD_STORAGE.lock().unwrap_or_else(|e| e.into_inner()) = Some(locations.storage.clone());
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
