//! A language switch in the open app (#141): the frame over the fixture
//! library, its root following the language as the app's does. When the
//! core's language changes (the Language setting's doing), the text already
//! on screen follows in place, without a restart or a remount: the sidebar,
//! Home, the library, Settings, a menu's dialog and plugins' contributions.
//! What screen readers hear (Blitz's AccessKit tree) follows too. The
//! pseudo-locale marks every label it reaches and leaves the sidebar and
//! Settings' sections laid out.
//!
//! The language is the process's: each test puts English back when it
//! ends, passing or not.

use crate::data::use_data_source;
use crate::frame::tests::{ONE, START, Window, source};
use crate::input::InputRoot;
use crate::router::Route;
use blitz_dom::{DocumentConfig, NodeId};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::model::{CollectionKind, SaveCollection};
use caskade_ui_data::plugin_ui::{DrawnPlugin, FakePluginUi, UiContribution};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;
use std::time::{Duration, Instant};

/// The frame as `frame::tests` opens it, with the root's language as the
/// app's `main` has it.
fn app() -> Element {
    crate::i18n::use_language();
    use_data_source(|| source().clone());
    use_context_provider(crate::game::launch_tests::fake_steps);
    use_context_provider(crate::game::page_tests::page_data_tests::fake_data);
    use_context_provider(crate::game::editor::tests::fake_edit_data);
    use_context_provider(crate::store::harness::fake_store_data);
    rsx! {
        style { {include_str!("../../assets/app.css")} }
        InputRoot {
            super::Frame { start: *caskade_core::lock(&START) }
        }
    }
}

fn open(href: &'static str) -> Window {
    source();
    *caskade_core::lock(&START) = href;
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        DocumentConfig {
            viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("the sidebar", |window| {
        window.find("data-cs", "sidebar").is_some()
    });
    // Other tests' windows open on Home.
    *caskade_core::lock(&START) = "/";
    window
}

/// English again when a test ends, whatever happened.
struct English;

impl English {
    fn start() -> Self {
        caskade_core::i18n::set_locale("en");
        caskade_ui_i18n::set_language("en");
        Self
    }
}

impl Drop for English {
    fn drop(&mut self) {
        caskade_core::i18n::set_locale("en");
        caskade_ui_i18n::set_language("en");
    }
}

/// The core switches to `language`, as a change of the Language setting
/// makes it, and the window settles until `done`.
fn speak(window: &mut Window, language: &str, done: impl Fn(&Window) -> bool) {
    caskade_core::i18n::set_locale(language);
    window.settle_until(language, done);
}

/// The pseudo-locale, as the root shows it when the Language setting asks
/// for it (the core keeps English, so only the setting says so; a test has
/// no settings service, so this sets what the root would).
fn pseudo(window: &mut Window, done: impl Fn(&Window) -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        let shown = caskade_ui_i18n::set_language("en-XA");
        window.doc.vdom.in_runtime(|| {
            if *crate::i18n::LANGUAGE.peek() != shown {
                *crate::i18n::LANGUAGE.write() = shown;
            }
        });
        window.settle();
        if done(window) {
            return;
        }
    }
    panic!("never pseudo-localized: {}", window.view_text());
}

fn label(window: &Window, node: NodeId) -> String {
    window.text(node).trim().to_owned()
}

/// The sidebar link's label element: its text, kept across a switch.
fn nav_label(window: &Window, text: &str) -> NodeId {
    let link = window.nav_link(text);
    window
        .find_in(link, "data-part", "label")
        .unwrap_or_else(|| panic!("{text} has no label"))
}

/// Where `node`'s box starts across the window, and how wide it is.
fn span(window: &Window, node: NodeId) -> (f64, f64) {
    let rect = window
        .doc
        .inner
        .borrow()
        .get_client_bounding_rect(node)
        .unwrap_or_else(|| panic!("node {node} has no box"));
    (rect.x, rect.width)
}

/// `inner`'s box lies across `outer`'s width.
fn within(window: &Window, inner: NodeId, outer: NodeId) -> bool {
    let ((x, width), (left, room)) = (span(window, inner), span(window, outer));
    x >= left - 0.5 && x + width <= left + room + 0.5
}

#[test]
fn switching_to_swedish_updates_home_the_library_and_settings_in_place() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let mut window = open("/");
    window.settle_until("Home", |window| window.find("data-view", "home").is_some());
    assert!(window.view_text().contains("Welcome back"));
    // The same elements before and after: nothing remounts.
    let home = nav_label(&window, "Home");
    let library = nav_label(&window, "Library");
    let settings = nav_label(&window, "Settings");
    let link = window.nav_link("Collections");
    window.focus(link);
    speak(&mut window, "sv", |window| {
        window.view_text().contains("Välkommen tillbaka")
    });
    assert_eq!(label(&window, home), "Hem");
    assert_eq!(label(&window, library), "Bibliotek");
    assert_eq!(label(&window, settings), "Inställningar");
    assert!(label(&window, link).contains("Samlingar"));
    assert_eq!(window.focused(), Some(link), "focus stays where it was");
    assert_eq!(window.route(), Route::Home);
    speak(&mut window, "en", |window| {
        window.view_text().contains("Welcome back")
    });
    assert_eq!(label(&window, home), "Home");

    // The library: its filters and their list's accessible name.
    window.go("/library?filter=all");
    window.settle_until("the library's filters", |window| {
        window.find("data-cs", "virtual-library").is_some()
            && window.find("data-part", "sub-item").is_some()
    });
    let all = window.nav_link("All Games");
    let all = window.find_in(all, "data-part", "label").expect("a label");
    let filters = window.get("aria-label", "Library filters");
    speak(&mut window, "sv", |window| {
        label(window, all) == "Alla spel"
    });
    assert_eq!(
        window.attr(filters, "aria-label").as_deref(),
        Some("Biblioteksfilter")
    );
    assert_eq!(label(&window, library), "Bibliotek");
    assert!(matches!(window.route(), Route::Library(_)));
    speak(&mut window, "en", |window| {
        label(window, all) == "All Games"
    });

    // Settings: its heading, and the open section's tab, still selected.
    window.go("/settings?tab=general");
    window.settle_until("Settings", |window| {
        window.find("data-cs", "settings-nav").is_some()
    });
    let general = window
        .find_all("role", "tab")
        .into_iter()
        .find(|&tab| label(&window, tab) == "General")
        .expect("the General tab");
    let appearance = window
        .find_all("role", "tab")
        .into_iter()
        .find(|&tab| label(&window, tab) == "Appearance")
        .expect("the Appearance tab");
    speak(&mut window, "sv", |window| {
        label(window, general) == "Allmänt"
    });
    assert_eq!(label(&window, appearance), "Utseende");
    assert_eq!(
        window.attr(general, "aria-selected").as_deref(),
        Some("true")
    );
    assert!(window.view_text().contains("Inställningar"));
    speak(&mut window, "en", |window| {
        label(window, general) == "General"
    });
}

/// What a screen reader hears of `node`, from Blitz's AccessKit tree.
fn heard(window: &Window, node: NodeId) -> String {
    crate::a11y_snapshot::snapshot_of(&window.doc.inner.borrow(), node)
}

#[test]
fn screen_readers_hear_the_sidebar_and_settings_in_the_new_language() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let mut window = open("/");
    window.settle_until("Home", |window| window.find("data-view", "home").is_some());
    let sidebar = window.get("data-cs", "sidebar");
    let tree = heard(&window, sidebar);
    // A link is named by the text inside it (a quoted line under it).
    let named = |tree: &str, link: &str, text: &str| {
        let lines: Vec<&str> = tree.lines().collect();
        lines.iter().enumerate().any(|(at, line)| {
            line.trim_start().starts_with(link)
                && lines
                    .get(at + 1)
                    .is_some_and(|next| next.trim() == format!("{text:?}"))
        })
    };
    assert!(named(&tree, "Link current=Page", "Home"), "{tree}");
    assert!(named(&tree, "Link", "Library"), "{tree}");
    speak(&mut window, "sv", |window| {
        named(&heard(window, sidebar), "Link current=Page", "Hem")
    });
    let tree = heard(&window, sidebar);
    assert!(named(&tree, "Link", "Bibliotek"), "{tree}");
    assert!(!tree.contains("\"Library\""), "{tree}");
    speak(&mut window, "en", |window| {
        named(&heard(window, sidebar), "Link current=Page", "Home")
    });

    // Settings' sections: the tab list, the open one selected.
    window.go("/settings?tab=general");
    window.settle_until("Settings", |window| {
        window.find("data-cs", "settings-nav").is_some()
    });
    let nav = window.get("data-cs", "settings-nav");
    let tree = heard(&window, nav);
    assert!(tree.contains("Tab \"General\" selected=true"), "{tree}");
    speak(&mut window, "sv", |window| {
        heard(window, nav).contains("Tab \"Allmänt\" selected=true")
    });
    let tree = heard(&window, nav);
    assert!(tree.contains("Tab \"Utseende\""), "{tree}");
    assert!(!tree.contains("\"General\""), "{tree}");
}

/// Collections a test made, deleted when it ends, passing or not.
struct Made(String);

impl Drop for Made {
    fn drop(&mut self) {
        let _ = caskade_app::runtime::block_on(source().library.delete_collection(&self.0));
    }
}

#[test]
fn an_open_dialog_follows_a_switch_and_keeps_its_focus() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let made = caskade_app::runtime::block_on(source().library.save_collection(&SaveCollection {
        id: None,
        name: "Spoken Picks".to_owned(),
        description: String::new(),
        kind: CollectionKind::Manual,
    }))
    .unwrap();
    let made = Made(made.id);
    let mut window = open("/");
    window.go(&Route::Collection { id: made.0.clone() }.href());
    window.settle_until("its page", |window| {
        window
            .find("data-view", "collection")
            .is_some_and(|view| window.text(view).contains("Spoken Picks"))
    });
    let more = window.get("aria-label", "More");
    window.click(more);
    let item = |window: &Window, text: &str| {
        window
            .find_all("role", "menuitem")
            .into_iter()
            .find(|&item| label(window, item) == text)
    };
    window.settle_until("its menu", |window| item(window, "Delete…").is_some());
    let delete = item(&window, "Delete…").unwrap();
    window.click(delete);
    window.settle_until("the question", |window| {
        window.find("role", "alertdialog").is_some()
    });
    let dialog = window.get("role", "alertdialog");
    assert!(window.text(dialog).contains("Delete Spoken Picks?"));
    // Cancel is the dialog's first stop.
    let cancel = window
        .find_all_in(dialog, "type", "button")
        .into_iter()
        .find(|&button| label(&window, button) == "Cancel")
        .expect("Cancel");
    window.settle_until("Cancel focused", |window| window.focused() == Some(cancel));
    speak(&mut window, "sv", |window| {
        window.text(dialog).contains("Radera Spoken Picks?")
    });
    let said = window.text(dialog);
    assert!(
        said.contains("Samlingen tas bort. Dess spel finns kvar i ditt bibliotek."),
        "{said}"
    );
    assert_eq!(label(&window, cancel), "Avbryt");
    assert_eq!(window.focused(), Some(cancel), "focus stays on Cancel");
    // The page behind follows too.
    assert_eq!(window.attr(more, "aria-label").as_deref(), Some("Mer"));
    speak(&mut window, "en", |window| {
        window.text(dialog).contains("Delete Spoken Picks?")
    });
    window.click(cancel);
    window.settle_until("the dialog closed", |window| {
        window.find("role", "alertdialog").is_none()
    });
}

const STATS: &str = "dev.caskade.playtime-stats";

fn fake() -> &'static FakePluginUi {
    source()
        .plugin_fake
        .as_ref()
        .expect("the fixtures' fake")
        .as_ref()
}

/// Empties the fake for the next test, whatever happened.
struct Clean;

impl Drop for Clean {
    fn drop(&mut self) {
        fake().clear();
        fake().set_plugins(Vec::new());
    }
}

/// The playtime plugin with a Home section titled `title`, drawing `note`:
/// what its own catalog says in a language.
fn stats(title: &str, note: &str) {
    fake().set_plugins(vec![DrawnPlugin {
        id: STATS.into(),
        name: "Playtime stats".into(),
        version: "0.2.0".into(),
        contributes: vec![UiContribution {
            slot: "home.sections".into(),
            id: "week".into(),
            title: Some(title.to_owned()),
            modes: Vec::new(),
        }],
    }]);
    fake().draw(
        STATS,
        "home.sections",
        "week",
        "*",
        Ok(vec![caskade_ui_data::plugin_ui::ui::note(note)]),
    );
}

/// The core announces a language change as the app layer does
/// (`commands::i18n::announce`), which fixtures don't run.
fn announce(window: &mut Window, language: &str) {
    caskade_core::i18n::set_locale(language);
    let _ = source()
        .events
        .send(caskade_app::events::Event::LocaleChanged(
            caskade_app::events::LocaleChanged {
                locale: language.to_owned(),
            },
        ));
    window.settle();
}

fn section_title(window: &Window) -> Option<String> {
    window
        .find("data-part", "plugin-section")
        .and_then(|section| window.attr(section, "aria-label"))
}

#[test]
fn plugins_contributions_are_asked_again_in_the_new_language() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let _clean = Clean;
    stats("This week", "3 hours across 2 games");
    let mut window = open("/");
    window.settle_until("the section", |window| {
        section_title(window).as_deref() == Some("This week")
    });
    // The plugin answers in Swedish now.
    stats("Den här veckan", "3 timmar i 2 spel");
    announce(&mut window, "sv");
    window.settle_until("the section's Swedish title", |window| {
        section_title(window).as_deref() == Some("Den här veckan")
    });
    let section = window.get("data-part", "plugin-section");
    assert!(window.text(section).contains("Den här veckan"));
}

/// What a plugin draws is asked again too, not only its title: its own
/// catalog answers in the new language. Gap found for #141: `DrawnOne`'s
/// tree is a resource that restarts on its state and mode, not the
/// language, so the drawing keeps the old one until something else redraws
/// it.
#[test]
fn a_plugins_drawing_is_asked_again_in_the_new_language() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let _clean = Clean;
    stats("This week", "3 hours across 2 games");
    let mut window = open("/");
    window.settle_until("the section", |window| {
        window
            .find("data-part", "plugin-section")
            .is_some_and(|section| window.text(section).contains("3 hours across 2 games"))
    });
    stats("Den här veckan", "3 timmar i 2 spel");
    announce(&mut window, "sv");
    window.settle_until("the drawing in Swedish", |window| {
        window
            .find("data-part", "plugin-section")
            .is_some_and(|section| window.text(section).contains("3 timmar i 2 spel"))
    });
}

#[test]
fn the_pseudo_locale_marks_the_frames_labels_and_keeps_them_laid_out() {
    if !cfg!(debug_assertions) {
        // Release builds have no pseudo-locale.
        return;
    }
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let mut window = open("/settings?tab=general");
    window.settle_until("Settings", |window| {
        window.find("data-cs", "settings-nav").is_some()
    });
    let sidebar = window.get("data-cs", "sidebar");
    let (_, width) = span(&window, sidebar);
    pseudo(&mut window, |window| {
        window
            .find_all("data-part", "nav-item")
            .into_iter()
            .all(|link| label(window, link).starts_with('['))
    });
    // Every label the frame shows here is marked: none is left unmarked.
    for link in window.find_all("data-part", "nav-item") {
        let said = label(&window, link);
        assert!(said.starts_with('[') && said.ends_with(']'), "{said:?}");
        assert!(
            within(&window, link, sidebar),
            "{said:?} spills out of the sidebar"
        );
    }
    let nav = window.get("data-cs", "settings-nav");
    for tab in window.find_all("role", "tab") {
        let said = label(&window, tab);
        assert!(said.starts_with('[') && said.ends_with(']'), "{said:?}");
        assert!(within(&window, tab, nav), "{said:?} spills out of the list");
    }
    // Longer words don't widen the sidebar.
    assert!((span(&window, sidebar).1 - width).abs() < 0.5);
    // The page's heading too: "Settings", accented.
    let said = window.view_text();
    assert!(said.contains("Śēţţĩńĝś"), "{said}");
}

/// Empties the fixtures' job queue for the next test, whatever happened.
struct NoJobs;

impl Drop for NoJobs {
    fn drop(&mut self) {
        if let Some(fakes) = source().job_fakes.as_ref() {
            fakes.queue.clear();
        }
    }
}

/// An install downloading, as the fixtures' queue reports it.
fn downloading(id: &str) -> caskade_core::install::JobSnapshot {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "revision": 1,
        "kind": "install",
        "status": "downloading",
        "title": "Celeste",
        "input": "https://example.invalid/celeste.zip",
        "rootId": null,
        "phase": "download",
        "progress": {"bytes": 2048, "totalBytes": 8192, "files": 0, "etaSecs": null, "message": "Downloading celeste.zip"},
        "warnings": [],
        "question": null,
        "gameId": null,
        "error": null,
        "candidates": [],
        "selectedCandidateIndex": null,
        "targetDir": null,
        "dependencies": [],
        "verification": null,
        "failure": null,
        "createdAtMs": 1,
        "updatedAtMs": 1
    }))
    .unwrap()
}

/// Activity (#153): a job's card and the page's header follow a switch in
/// place, and so does the notification feed.
#[test]
fn activity_its_jobs_and_its_notifications_follow_a_switch() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let _no_jobs = NoJobs;
    source()
        .job_fakes
        .as_ref()
        .expect("the fixtures' fakes")
        .queue
        .put(downloading("j1"));
    let mut window = open("/activity?tab=jobs");
    window.settle_until("the job", |window| {
        window.find("data-cs", "activity-job").is_some()
    });
    assert!(window.view_text().contains("Jobs and notifications"));
    let cancel = window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| label(&window, button) == "Cancel job")
        .expect("Cancel job");
    speak(&mut window, "sv", |window| {
        label(window, cancel) == "Avbryt jobb"
    });
    let said = window.view_text();
    assert!(said.contains("Jobb och aviseringar"), "{said}");
    assert!(!said.contains("Jobs and notifications"), "{said}");
    // The feed, opened in Swedish, then back in English in place.
    window.go("/activity?tab=notifications");
    window.settle_until("the feed in Swedish", |window| {
        window.view_text().contains(
            "Inga aviseringar än. Nya spel, misslyckade jobb och uppdateringar visas här.",
        )
    });
    speak(&mut window, "en", |window| {
        window
            .view_text()
            .contains("No notifications yet. New games, failed jobs and updates show here.")
    });
}

/// The library's grid, with the id of the game on card `index`.
fn library_card(window: &mut Window, index: &str) -> (NodeId, String) {
    window.settle_until("the cards", |window| {
        !window.find_all("data-index", index).is_empty()
    });
    let card = window.get("data-index", index);
    let id = window
        .attr(card, "data-focus-key")
        .and_then(|key| key.strip_prefix("game-").map(str::to_owned))
        .expect("the card's game");
    (card, id)
}

/// What Play asks first, open over the library (#153): its words follow a
/// switch and focus stays on the button it was on.
#[test]
fn a_question_play_asks_follows_a_switch_and_keeps_its_focus() {
    use crate::game::launch_tests::{Script, script};
    use caskade_core::updates::GameUpdate;
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let mut window = open("/library?filter=all&sort=title&layout=grid");
    let (card, game) = library_card(&mut window, "0");
    let _script = script(Script {
        update: Some(GameUpdate {
            entry_id: "e".to_owned(),
            game_id: game,
            provider: "github".to_owned(),
            project: "tide/water".to_owned(),
            project_url: None,
            installed: Some("1.1".to_owned()),
            latest: Some("1.2".to_owned()),
            latest_url: None,
            available: true,
            skipped: None,
            prereleases: false,
            checked_at_ms: 0,
            error: None,
        }),
        ..Script::default()
    });
    window.double_click(card);
    window.settle_until("the offer", |window| {
        window.find("data-cs", "update-offer").is_some()
    });
    let offer = window.get("data-cs", "update-offer");
    assert!(window.text(offer).contains("Update before playing?"));
    let play_now = window
        .find_in(offer, "data-part", "play-now")
        .expect("Play now");
    window.focus(play_now);
    speak(&mut window, "sv", |window| {
        window.text(offer).contains("Uppdatera innan du spelar?")
    });
    assert_eq!(label(&window, play_now), "Spela nu");
    assert_eq!(window.focused(), Some(play_now), "focus stays on Play now");
    speak(&mut window, "en", |window| {
        window.text(offer).contains("Update before playing?")
    });
    window.key(dioxus_native::prelude::keyboard_types::Key::Named(
        dioxus_native::prelude::keyboard_types::NamedKey::Escape,
    ));
    window.settle_until("the offer closed", |window| {
        window.find("data-cs", "update-offer").is_none()
    });
}

/// The game editor open over a game's page (#153): its title and tabs
/// follow a switch in place.
#[test]
fn the_game_editor_follows_a_switch() {
    use crate::game::editor::tests::{EditScript, edit_script};
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let mut game = caskade_core::model::Game::new("Fixture Game 0002");
    game.id = "fixture-1".to_owned();
    let _script = edit_script(EditScript {
        state: Some(caskade_core::model::GameEditState {
            automatic: game.clone(),
            game,
            manual: Vec::new(),
        }),
        ..EditScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("Edit details", |window| {
        window.find("data-part", "edit").is_some()
    });
    let edit = window.get("data-part", "edit");
    window.click(edit);
    window.settle_until("the editor", |window| {
        window.find("data-cs", "game-editor").is_some()
            && window.find("data-field", "title").is_some()
    });
    let editor = window.get("data-cs", "game-editor");
    assert!(window.text(editor).contains("Edit Fixture Game 0002"));
    let details = window
        .find_all_in(editor, "role", "tab")
        .into_iter()
        .find(|&tab| label(&window, tab).starts_with("Details"))
        .expect("the Details tab");
    speak(&mut window, "sv", |window| {
        window.text(editor).contains("Redigera Fixture Game 0002")
    });
    assert!(label(&window, details).starts_with("Detaljer"));
    speak(&mut window, "en", |window| {
        window.text(editor).contains("Edit Fixture Game 0002")
    });
}

/// The toasts' messages shown now, in order.
fn toasts(window: &Window) -> Vec<String> {
    window
        .find_all("data-part", "toast")
        .into_iter()
        .filter_map(|toast| window.find_in(toast, "data-part", "message"))
        .map(|message| label(window, message))
        .collect()
}

/// Toasts are words made when they're shown (#153), as in the webview: one
/// already shown keeps its language through a switch, and the next one
/// comes in the new language.
#[test]
fn a_toast_keeps_its_words_and_the_next_comes_in_the_new_language() {
    use crate::game::launch_tests::{Script, script, started, version};
    use caskade_core::saves::versions::{BeforePlay, SaveCheck};
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _english = English::start();
    let restored = || BeforePlay {
        check: SaveCheck::Restored {
            version: version("theirs", false),
        },
        checked: true,
        warnings: Vec::new(),
    };
    let mut window = open("/library?filter=all&sort=title&layout=grid");
    let (first, game) = library_card(&mut window, "0");
    let scripted = script(Script {
        saves: [restored()].into(),
        outcomes: [started(&game)].into(),
        ..Script::default()
    });
    window.double_click(first);
    window.settle_until("the English toast", |window| {
        toasts(window)
            .iter()
            .any(|toast| toast.starts_with("Started with the save from"))
    });
    let library = nav_label(&window, "Library");
    speak(&mut window, "sv", |window| {
        label(window, library) == "Bibliotek"
    });
    // Shown before the switch: still in English.
    let shown = toasts(&window);
    assert!(
        shown
            .iter()
            .any(|toast| toast.starts_with("Started with the save from")),
        "{shown:?}"
    );
    assert!(
        !shown
            .iter()
            .any(|toast| toast.starts_with("Startade med sparfilen från")),
        "{shown:?}"
    );
    drop(scripted);
    let (second, game) = library_card(&mut window, "1");
    let _scripted = script(Script {
        saves: [restored()].into(),
        outcomes: [started(&game)].into(),
        ..Script::default()
    });
    window.double_click(second);
    window.settle_until("the Swedish toast", |window| {
        toasts(window)
            .iter()
            .any(|toast| toast.starts_with("Startade med sparfilen från"))
    });
}
