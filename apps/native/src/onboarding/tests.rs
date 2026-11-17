//! First-run setup (#97) on the frame's fixture library, as the webview's
//! `onboarding.test.tsx`: setup shows in place of the app until it's done;
//! Skip setup keeps the defaults and opens the library; a first run goes
//! from the welcome through the questions to the work and the library, its
//! choices saved. Each test has a fixture library of its own (setup's scan
//! changes what discovery shows).

use crate::data::DataSource;
use crate::frame::tests::{ONE, Window};
use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_ui_data::storage::Storage;
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;
use serde_json::{Value, json};
use std::sync::Mutex;

/// A games folder setup can choose: storage with one location.
static STORAGE: Mutex<Option<Storage>> = Mutex::new(None);
/// The test's own library.
static LIBRARY: Mutex<Option<DataSource>> = Mutex::new(None);

fn source() -> DataSource {
    caskade_core::lock(&LIBRARY).clone().expect("a library")
}

fn storage(dir: &std::path::Path) -> Storage {
    caskade_app::runtime::block_on(async move {
        use caskade_core::{
            AppPaths, db::LibraryStore, launch::LaunchService, settings::SettingsService,
            storage::StorageService,
        };
        let paths = AppPaths::under(&dir.join("app"));
        let store = LibraryStore::in_memory().await.unwrap();
        let settings = SettingsService::new(store.clone());
        settings.register_core(&paths).unwrap();
        let launch = LaunchService::new(store.clone()).with_aliases(None);
        let service = StorageService::new(store.clone(), settings.clone(), launch)
            .unwrap()
            .with_paths(paths);
        service
            .register_root(dir.join("games"), Some("Games".into()))
            .await
            .unwrap();
        let bus = caskade_core::bus::Bus::default();
        let accounts = caskade_core::accounts::AccountService::open(store.clone(), bus.clone())
            .await
            .unwrap();
        let source = caskade_ui_data::storage::StorageSource::new(
            store.clone(),
            bus,
            service.reconciler().unwrap(),
        );
        Storage::new(service, None, settings).with_locations(source, accounts)
    })
}

/// The frame as the app has it, with storage.
fn app() -> Element {
    crate::data::use_data_source(source);
    use_context_provider(crate::library::view_tests::fake_drop_install);
    use_context_provider(crate::game::launch_tests::fake_steps);
    use_context_provider(crate::game::page_tests::page_data_tests::fake_data);
    use_context_provider(crate::game::editor::tests::fake_edit_data);
    if let Some(storage) = caskade_core::lock(&STORAGE).clone() {
        use_context_provider(move || storage);
    }
    rsx! {
        style { {include_str!("../../assets/app.css")} }
        crate::input::InputRoot {
            crate::frame::Frame { start: "/" }
        }
    }
}

/// The frame on the welcome.
fn open(storage: Option<Storage>) -> Window {
    *caskade_core::lock(&STORAGE) = storage;
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("setup", |window| step(window).as_deref() == Some("welcome"));
    window
}

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    caskade_app::runtime::block_on(future)
}

fn saved(namespace: &str) -> Value {
    block(source().settings.get(namespace)).unwrap().value
}

fn set(namespace: &str, key: &str, value: Value) {
    let key = key.to_owned();
    block(source().settings.update(namespace, move |mut current| {
        current[&key] = value;
        current
    }))
    .unwrap();
}

/// A library of its own, with setup to do.
struct FirstRun;

fn first_run() -> FirstRun {
    *caskade_core::lock(&LIBRARY) = Some(block(DataSource::fixtures(3)).unwrap());
    set("core.onboarding", "completed", json!(false));
    set("core.onboarding", "sounds", json!(false));
    FirstRun
}

impl Drop for FirstRun {
    fn drop(&mut self) {
        *caskade_core::lock(&LIBRARY) = None;
    }
}

fn step(window: &Window) -> Option<String> {
    window
        .find("data-cs", "onboarding")
        .and_then(|root| window.attr(root, "data-step"))
}

fn press(window: &mut Window, id: &str) {
    window.settle_until(id, |window| {
        window
            .find("id", id)
            .is_some_and(|node| window.attr(node, "aria-disabled").as_deref() != Some("true"))
    });
    let node = window.get("id", id);
    window.click(node);
}

#[test]
fn skip_setup_keeps_the_defaults_and_opens_the_library() {
    let _one = caskade_core::lock(&ONE);
    let _run = first_run();
    let mut window = open(None);
    // Nothing of the app shows behind it.
    assert!(window.find("data-cs", "app-shell").is_none());
    press(&mut window, "onboarding-skip");
    window.settle_until("the library", |window| {
        window.find("data-cs", "app-shell").is_some() && window.href().starts_with("/library")
    });
    assert_eq!(saved("core.onboarding")["completed"], json!(true));
}

#[test]
fn a_first_run_goes_through_the_questions_and_the_work_to_the_library() {
    let _one = caskade_core::lock(&ONE);
    let _run = first_run();
    let dir = tempfile::tempdir().unwrap();
    let mut window = open(Some(storage(dir.path())));
    press(&mut window, "onboarding-start");
    window.settle_until("the language", |window| {
        step(window).as_deref() == Some("language")
    });
    // The progress dots: the first of the questions.
    let progress = window.get("data-part", "progress");
    assert!(
        window
            .attr(progress, "aria-label")
            .is_some_and(|label| label.starts_with("Step 1 of"))
    );
    press(&mut window, "onboarding-next");
    window.settle_until("the folder", |window| {
        step(window).as_deref() == Some("folder")
    });
    press(&mut window, "onboarding-next");
    window.settle_until("the sources", |window| {
        step(window).as_deref() == Some("sources")
    });
    press(&mut window, "onboarding-next");
    window.settle_until("the mode", |window| step(window).as_deref() == Some("mode"));
    press(&mut window, "onboarding-next");
    // The fixture library's admin names their account last.
    window.settle_until("the account", |window| {
        step(window).as_deref() == Some("account")
    });
    press(&mut window, "onboarding-next");
    window.settle_until("the work", |window| {
        step(window).as_deref() == Some("setup")
    });
    // Setup counts as done once its choices are saved.
    window.settle_until("marked done", |_| {
        saved("core.onboarding")["completed"] == json!(true)
    });
    assert_eq!(saved("core.discovery")["enabled"], json!(true));
    assert_eq!(saved("core.metadata")["online"], json!(true));
    // The fixture machine's launchers: games found, then Open my library.
    window.settle_until("the work done", |window| {
        window
            .find("data-part", "setup")
            .and_then(|setup| window.attr(setup, "data-status"))
            .as_deref()
            == Some("done")
    });
    press(&mut window, "onboarding-open-library");
    window.settle_until("the library", |window| {
        window.find("data-cs", "app-shell").is_some() && window.href().starts_with("/library")
    });
}

#[test]
fn the_choices_and_the_admins_account_are_saved() {
    use dioxus_native::prelude::keyboard_types::{Key, Modifiers, NamedKey};
    let _one = caskade_core::lock(&ONE);
    let _run = first_run();
    let accounts = source().settings.accounts();
    let admin = block(accounts.list()).unwrap().active_id.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut window = open(Some(storage(dir.path())));
    for step_name in ["language", "folder", "sources"] {
        if step_name == "language" {
            press(&mut window, "onboarding-start");
        } else {
            press(&mut window, "onboarding-next");
        }
        window.settle_until(step_name, |window| {
            step(window).as_deref() == Some(step_name)
        });
    }
    // Nothing to look for, nothing online.
    for id in ["onboarding-pc-games", "onboarding-online"] {
        let switch = window.get("id", id);
        window.focus(switch);
        window.key(Key::Character(" ".into()));
    }
    press(&mut window, "onboarding-next");
    window.settle_until("the mode", |window| step(window).as_deref() == Some("mode"));
    press(&mut window, "onboarding-next");
    window.settle_until("the account", |window| {
        step(window).as_deref() == Some("account")
    });
    let name = window.get("id", "onboarding-name");
    window.focus(name);
    window.key_with(Key::Character("a".into()), Modifiers::CONTROL);
    window.key(Key::Named(NamedKey::Backspace));
    window.type_text("Ada");
    let pin = window.get("id", "onboarding-pin");
    window.focus(pin);
    window.type_text("1357");
    press(&mut window, "onboarding-next");
    window.settle_until("the work", |window| {
        step(window).as_deref() == Some("setup")
    });
    window.settle_until("marked done", |_| {
        saved("core.onboarding")["completed"] == json!(true)
    });
    assert_eq!(saved("core.discovery")["enabled"], json!(false));
    assert_eq!(saved("core.metadata")["online"], json!(false));
    let after = block(accounts.list())
        .unwrap()
        .accounts
        .into_iter()
        .find(|account| account.id == admin)
        .unwrap();
    assert_eq!(after.name, "Ada");
    assert!(after.has_pin);
}

#[test]
fn choosing_console_mode_switches_the_window_at_once() {
    let _one = caskade_core::lock(&ONE);
    let _run = first_run();
    let dir = tempfile::tempdir().unwrap();
    let mut window = open(Some(storage(dir.path())));
    press(&mut window, "onboarding-start");
    window.settle_until("the language", |window| {
        step(window).as_deref() == Some("language")
    });
    // Past the questions to the mode.
    for _ in 0..4 {
        if step(&window).as_deref() == Some("mode") {
            break;
        }
        let at = step(&window);
        press(&mut window, "onboarding-next");
        window.settle_until("the next question", |window| step(window) != at);
    }
    window.settle_until("the mode", |window| step(window).as_deref() == Some("mode"));
    assert!(window.find("data-mode", "desktop").is_some());
    press(&mut window, "onboarding-mode-console");
    // The window is in console mode now, still on setup, and stays so.
    window.settle_until("console mode", |window| {
        window.find("data-mode", "console").is_some()
    });
    assert_eq!(step(&window).as_deref(), Some("mode"));
    window.settle_until("remembered", |_| {
        saved("core.console")["active"] == json!(true)
    });
    press(&mut window, "onboarding-mode-desktop");
    window.settle_until("the desktop", |window| {
        window.find("data-mode", "desktop").is_some()
    });
}

#[test]
fn a_readonly_settings_database_pauses_setup_and_retry_finishes() {
    use super::run::{Choices, Report, SetupParts, Task, TaskState, run_setup};
    use super::sound::Cue;
    use caskade_core::{
        AppPaths, accounts::AccountService, bus::Bus, db::LibraryStore, settings::SettingsService,
    };
    use std::cell::{Cell, RefCell};
    struct Progress {
        tasks: RefCell<Vec<Task>>,
        ready: Cell<bool>,
    }
    impl Report for Progress {
        fn update(&self, tasks: Vec<Task>) {
            *self.tasks.borrow_mut() = tasks;
        }
        fn cue(&self, _: Cue) {}
        fn ready(&self) {
            self.ready.set(true);
        }
    }
    let _one = caskade_core::lock(&ONE);
    let dir = tempfile::tempdir().unwrap();
    block(async {
        let paths = AppPaths::under(dir.path());
        let store = LibraryStore::open(&paths.database()).await.unwrap();
        let settings = SettingsService::new(store.clone());
        settings.register_core(&paths).unwrap();
        let accounts = AccountService::open(store.clone(), Bus::default())
            .await
            .unwrap();
        let parts = SetupParts {
            settings: caskade_ui_data::Settings::new(settings.clone(), accounts),
            discovery: None,
            emulation: None,
            romm: None,
        };
        // Hold three connections so every setup read/write uses the fourth,
        // with SQLite itself refusing writes. This works even as root.
        let _a = store.pool.acquire().await.unwrap();
        let _b = store.pool.acquire().await.unwrap();
        let _c = store.pool.acquire().await.unwrap();
        sqlx::query("PRAGMA query_only = ON")
            .execute(&store.pool)
            .await
            .unwrap();
        let progress = Progress {
            tasks: RefCell::new(Vec::new()),
            ready: Cell::new(false),
        };
        let choices = Choices::default();
        let failure = run_setup(&parts, &choices, &progress).await.unwrap_err();
        assert!(failure.summary.contains("Saving discovery choices"));
        assert!(
            failure
                .summary
                .contains(&paths.database().display().to_string())
        );
        assert!(failure.detail.is_some());
        assert!(!progress.ready.get());
        assert!(
            progress
                .tasks
                .borrow()
                .iter()
                .all(|task| task.state == TaskState::Skipped)
        );
        // The test restores access on the same pool, then retries the same choices.
        sqlx::query("PRAGMA query_only = OFF")
            .execute(&store.pool)
            .await
            .unwrap();
        let tasks = run_setup(&parts, &choices, &progress).await.unwrap();
        assert!(progress.ready.get());
        assert!(
            tasks
                .iter()
                .all(|task| !matches!(task.state, TaskState::Waiting | TaskState::Running))
        );
        assert_eq!(
            settings.get("core.onboarding").await.unwrap().value["completed"],
            json!(true)
        );
    });
}

/// Setup with the language followed, as the app's root does.
fn speaking_app() -> Element {
    crate::i18n::use_language();
    app()
}

/// English again when a test ends, whatever happened: the language is the
/// process's.
struct English;

impl Drop for English {
    fn drop(&mut self) {
        caskade_core::i18n::set_locale("en");
        caskade_ui_i18n::set_language("en");
    }
}

/// A language switch during setup (#153), as choosing a language makes
/// it: the open question's words follow in place, and setup stays on it.
#[test]
fn setup_follows_a_language_switch_in_place() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _run = first_run();
    caskade_core::i18n::set_locale("en");
    caskade_ui_i18n::set_language("en");
    let _english = English;
    *caskade_core::lock(&STORAGE) = None;
    let mut doc = DioxusDocument::new(
        VirtualDom::new(speaking_app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("setup", |window| step(window).as_deref() == Some("welcome"));
    press(&mut window, "onboarding-start");
    window.settle_until("the language", |window| {
        step(window).as_deref() == Some("language")
    });
    let next = window.get("id", "onboarding-next");
    let said = |window: &Window| window.text(window.get("data-cs", "onboarding"));
    assert!(window.text(next).contains("Continue"));
    assert!(said(&window).contains("Choose your language"));
    caskade_core::i18n::set_locale("sv");
    window.settle_until("setup in Swedish", |window| {
        window.text(next).contains("Fortsätt")
    });
    assert!(said(&window).contains("Välj språk"), "{}", said(&window));
    assert_eq!(
        step(&window).as_deref(),
        Some("language"),
        "still on the same question"
    );
    caskade_core::i18n::set_locale("en");
    window.settle_until("setup in English", |window| {
        window.text(next).contains("Continue")
    });
}

/// First-run setup by a pad alone (#153): A on Get started, each
/// question's Continue reached by a pad's moves and pressed with A, B back
/// a question, then the work and Open my library.
#[test]
fn a_pad_alone_goes_through_setup_and_back_a_question() {
    use crate::frame::pad_tests::reach;
    use caskade_ui_input::Action;
    let _one = caskade_core::lock(&ONE);
    let _run = first_run();
    let dir = tempfile::tempdir().unwrap();
    let mut window = open(Some(storage(dir.path())));
    // A pad presses `id`, reached from where it is in the setup.
    fn press_by_pad(window: &mut Window, id: &str) {
        window.settle_until(id, |window| {
            window
                .find("id", id)
                .is_some_and(|node| window.attr(node, "aria-disabled").as_deref() != Some("true"))
        });
        let setup = window.get("data-cs", "onboarding");
        reach(&mut *window, setup, id, |window, node| {
            window.attr(node, "id").as_deref() == Some(id)
        });
        window.pad(Action::Confirm);
    }
    press_by_pad(&mut window, "onboarding-start");
    window.settle_until("the language", |window| {
        step(window).as_deref() == Some("language")
    });
    press_by_pad(&mut window, "onboarding-next");
    window.settle_until("the folder", |window| {
        step(window).as_deref() == Some("folder")
    });
    // B goes back a question.
    window.pad(Action::Back);
    window.settle_until("the language again", |window| {
        step(window).as_deref() == Some("language")
    });
    for (from, to) in [
        ("language", "folder"),
        ("folder", "sources"),
        ("sources", "mode"),
        ("mode", "account"),
        ("account", "setup"),
    ] {
        assert_eq!(step(&window).as_deref(), Some(from));
        press_by_pad(&mut window, "onboarding-next");
        window.settle_until(to, |window| step(window).as_deref() == Some(to));
    }
    window.settle_until("marked done", |_| {
        saved("core.onboarding")["completed"] == json!(true)
    });
    window.settle_until("the work done", |window| {
        window
            .find("data-part", "setup")
            .and_then(|setup| window.attr(setup, "data-status"))
            .as_deref()
            == Some("done")
    });
    press_by_pad(&mut window, "onboarding-open-library");
    window.settle_until("the library", |window| {
        window.find("data-cs", "app-shell").is_some() && window.href().starts_with("/library")
    });
}
