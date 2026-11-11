//! A game's page (#100) in the frame over the fixture library, headless,
//! per kind of game, as `GameView.test.tsx` has them: an installed game,
//! one that isn't, and an emulated one; the way back; and what isn't there.

use super::launch_tests::{Script, launches, script, started};
use crate::frame::tests::{ONE, Window};
use crate::router::Route;

/// `href`'s page, once its title shows.
fn open(href: &str) -> Window {
    let mut window = Window::open();
    window.go(href);
    window.settle_until("the game's page", |window| {
        window.find("data-view", "game").is_some()
            || window
                .find_all("role", "status")
                .into_iter()
                .any(|node| window.text(node).contains("Game not found"))
            || window.find("data-part", "back").is_some()
    });
    window.settle_until("its title", |window| {
        window.find("data-part", "title").is_some() || window.find("data-part", "back").is_some()
    });
    window
}

fn text(window: &Window, part: &str) -> String {
    window
        .find_all("data-part", part)
        .into_iter()
        .map(|node| window.text(node))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn play_button(window: &Window) -> blitz_dom::NodeId {
    let hero = window.get("data-cs", "hero");
    window.find_in(hero, "data-part", "play").expect("Play")
}

#[test]
fn an_installed_game_shows_what_it_is_and_its_release() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    // Fixture Game 0002: installed, Adventure, PC, released 2024.
    let window = open("/games/fixture-1");
    assert_eq!(
        window
            .attr(window.get("data-view", "game"), "data-status")
            .as_deref(),
        Some("installed")
    );
    assert_eq!(text(&window, "title"), "Fixture Game 0002");
    let meta = text(&window, "meta");
    assert!(meta.contains("Adventure · PC · 1.0 GB · 2024"), "{meta}");
    assert!(text(&window, "description").contains("Original development fixture."));
    assert!(text(&window, "tags").contains("Fixture"));
    // Its one release, from the fixture launcher, with nothing to launch:
    // Play stays, disabled.
    let entry = text(&window, "entry");
    assert!(entry.contains("Installed"), "{entry}");
    assert_eq!(
        window
            .attr(play_button(&window), "data-disabled")
            .as_deref(),
        Some("true")
    );
    // One release: no picker, no Default badge.
    assert!(window.find("data-part", "release").is_none());
    assert!(window.find("data-part", "default").is_none());
    assert!(text(&window, "sessions").is_empty());
}

#[test]
fn a_game_that_isnt_installed_says_its_release_is_partly_there() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    // Fixture Game 0001: not installed.
    let window = open("/games/fixture-0");
    assert_eq!(
        window
            .attr(window.get("data-view", "game"), "data-status")
            .as_deref(),
        Some("not-installed")
    );
    let entry = text(&window, "entry");
    assert!(entry.contains("Partly installed"), "{entry}");
    assert_eq!(
        window
            .attr(play_button(&window), "data-disabled")
            .as_deref(),
        Some("true")
    );
}

#[test]
fn an_emulated_game_plays_through_play() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/games/fixture-emulated-0-0");
    assert_eq!(
        window
            .attr(window.get("data-view", "game"), "data-status")
            .as_deref(),
        Some("emulated")
    );
    let _script = script(Script {
        outcomes: [started("fixture-emulated-0-0")].into(),
        ..Script::default()
    });
    let play = play_button(&window);
    assert_eq!(window.attr(play, "data-disabled"), None);
    window.click(play);
    window.settle_until("the launch", |_| launches() == 1);
}

#[test]
fn back_goes_where_the_game_was_opened_from() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    // Opened from a link to it: back to the Library, in its place.
    let mut window = open("/games/fixture-1");
    let back = window.get("data-part", "back");
    assert!(
        window.text(back).contains("Back to Library"),
        "{}",
        window.text(back)
    );
    window.click(back);
    window.settle_until("the Library", |window| {
        matches!(window.route(), Route::Library(_))
    });
    // Opened from another game's page: named after that game, and a step
    // back in history to it.
    window.go("/games/fixture-1");
    window.settle_until("the first game", |window| {
        window.find("data-part", "title").is_some()
    });
    window.go("/games/fixture-2?from=%2Fgames%2Ffixture-1");
    window.settle_until("the second game", |window| {
        window
            .find("data-part", "title")
            .is_some_and(|title| window.text(title) == "Fixture Game 0003")
    });
    window.settle_until("the other game's name", |window| {
        window
            .find("data-part", "back")
            .is_some_and(|back| window.text(back).contains("Back to Fixture Game 0002"))
    });
    let back = window.get("data-part", "back");
    window.click(back);
    window.settle_until(
        "the first game again",
        |window| matches!(window.route(), Route::Game { ref id, .. } if id == "fixture-1"),
    );
}

#[test]
fn a_game_that_isnt_there_says_so() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    window.go("/games/no-such-game");
    window.settle_until("not found", |window| {
        window
            .find("role", "status")
            .is_none_or(|status| !window.text(status).contains("Loading"))
            && window.find("data-part", "back").is_some()
    });
    assert!(window.find("data-view", "game").is_none());
}

#[test]
fn a_right_click_on_the_hero_opens_the_games_menu() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/games/fixture-1");
    let hero = window.get("data-cs", "hero");
    window.right_click(hero);
    window.settle_until("the menu", |window| window.find("role", "menu").is_some());
}

// The page's own data, scripted: updates, launch actions, separating.

use super::data::{PageData, UpdateChange};
use caskade_core::launch::ReleaseActions;
use caskade_core::model::{
    ActionKind, ActionTracking, EntryActionSettings, GameAction, LaunchSpec,
};
use caskade_core::updates::GameUpdate;
use std::rc::Rc;
use std::sync::Mutex;

/// What the page's data answers, and what it was asked.
#[derive(Default)]
pub(crate) struct PageScript {
    pub(crate) updates: Vec<GameUpdate>,
    pub(crate) actions: Vec<ReleaseActions>,
    pub(crate) changes: Vec<(String, UpdateChange)>,
    pub(crate) installs: Vec<String>,
    pub(crate) saved: Vec<(String, EntryActionSettings)>,
    pub(crate) separated: Vec<String>,
}

static PAGE: Mutex<Option<PageScript>> = Mutex::new(None);

fn page<T>(f: impl FnOnce(&mut PageScript) -> T) -> T {
    let mut script = PAGE.lock().unwrap_or_else(|e| e.into_inner());
    f(script.get_or_insert_with(PageScript::default))
}

pub(crate) struct PageScripted;

pub(crate) fn page_script(script: PageScript) -> PageScripted {
    *PAGE.lock().unwrap_or_else(|e| e.into_inner()) = Some(script);
    PageScripted
}

impl Drop for PageScripted {
    fn drop(&mut self) {
        *PAGE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn answer<T: 'static>(value: T) -> super::launch::Answer<T> {
    Box::pin(async move { Ok(value) })
}

/// The page data every test window reads (`frame::tests`' app provides it).
pub(crate) fn fake_data() -> PageData {
    PageData {
        updates: Rc::new(|game_id| {
            answer(page(|script| {
                script
                    .updates
                    .iter()
                    .filter(|update| update.game_id == game_id)
                    .cloned()
                    .collect()
            }))
        }),
        change_update: Rc::new(|entry_id, change| {
            let update = page(|script| {
                script.changes.push((entry_id.clone(), change));
                script
                    .updates
                    .iter()
                    .find(|update| update.entry_id == entry_id)
                    .cloned()
            });
            Box::pin(async move { update.ok_or_else(|| "no such update".to_owned().into()) })
        }),
        install_update: Rc::new(|entry_id| {
            page(|script| script.installs.push(entry_id));
            answer("job-1".to_owned())
        }),
        actions: Rc::new(|_| answer(page(|script| script.actions.clone()))),
        save_action_settings: Rc::new(|_, entry_id, settings| {
            page(|script| script.saved.push((entry_id, settings.clone())));
            answer(settings)
        }),
        separate: Rc::new(|entry_id| {
            page(|script| script.separated.push(entry_id));
            answer("separated".to_owned())
        }),
    }
}

fn update(entry_id: &str, game_id: &str) -> GameUpdate {
    GameUpdate {
        entry_id: entry_id.to_owned(),
        game_id: game_id.to_owned(),
        provider: "github".to_owned(),
        project: "tide/water".to_owned(),
        project_url: Some("https://github.com/tide/water".to_owned()),
        installed: Some("1.0".to_owned()),
        latest: Some("1.1".to_owned()),
        latest_url: None,
        available: true,
        skipped: None,
        prereleases: false,
        checked_at_ms: 1_700_000_000_000,
        error: None,
    }
}

fn action(id: &str, entry_id: &str, name: &str, kind: ActionKind) -> GameAction {
    GameAction {
        id: id.to_owned(),
        entry_id: entry_id.to_owned(),
        name: name.to_owned(),
        kind,
        target: LaunchSpec::Uri {
            uri: format!("fixture://{id}"),
        },
        tracking: ActionTracking::default(),
        proton: None,
    }
}

#[test]
fn a_followed_install_offers_its_newer_release_and_skips_it() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript {
        updates: vec![update("entry-1", "fixture-1")],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the update", |window| {
        window.find("data-part", "update").is_some()
    });
    let panel = window.get("data-part", "update");
    assert_eq!(
        window.attr(panel, "data-status").as_deref(),
        Some("available")
    );
    let said = window.text(panel);
    assert!(
        said.contains("v1.1 is out") && said.contains("you have v1.0"),
        "{said}"
    );
    let skip = window
        .find_in(panel, "data-part", "skip")
        .expect("Skip this version");
    window.click(skip);
    window.settle_until("the skip", |_| page(|script| script.changes.len()) == 1);
    assert_eq!(
        page(|script| script.changes[0].clone()),
        (
            "entry-1".to_owned(),
            UpdateChange::Skip(Some("1.1".to_owned()))
        )
    );
    let panel = window.get("data-part", "update");
    let install = window
        .find_in(panel, "data-part", "install-update")
        .expect("Update");
    window.click(install);
    window.settle_until("the install", |_| page(|script| script.installs.len()) == 1);
}

#[test]
fn a_release_with_several_launch_actions_picks_the_one_play_starts_and_lists_its_tools() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript {
        actions: vec![ReleaseActions {
            entry_id: "fixture-emulated-entry-0-0".to_owned(),
            editable_built_in: false,
            has_folder: false,
            proton: false,
            actions: vec![
                action(
                    "main",
                    "fixture-emulated-entry-0-0",
                    "Play",
                    ActionKind::Play,
                ),
                action(
                    "vr",
                    "fixture-emulated-entry-0-0",
                    "Play in VR",
                    ActionKind::Play,
                ),
                action(
                    "config",
                    "fixture-emulated-entry-0-0",
                    "Configure",
                    ActionKind::Utility,
                ),
            ],
            settings: EntryActionSettings::default(),
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-emulated-0-0");
    window.settle_until("the choice", |window| {
        window.find("data-cs", "play-choice").is_some()
    });
    let choice = window.get("data-cs", "play-choice");
    assert!(
        window.text(choice).contains("Configure"),
        "{}",
        window.text(choice)
    );
    // Play starts "Play in VR" from now on.
    let select = window
        .find_in(choice, "data-part", "play-starts")
        .expect("Play starts");
    window.focus(select);
    window.key(dioxus_native::prelude::keyboard_types::Key::Character(
        " ".into(),
    ));
    window.settle_until("its options", |window| {
        window.find("role", "listbox").is_some()
    });
    window.key(dioxus_native::prelude::keyboard_types::Key::Named(
        dioxus_native::prelude::keyboard_types::NamedKey::ArrowDown,
    ));
    window.key(dioxus_native::prelude::keyboard_types::Key::Named(
        dioxus_native::prelude::keyboard_types::NamedKey::Enter,
    ));
    window.settle_until("the save", |_| page(|script| script.saved.len()) == 1);
    let (entry, settings) = page(|script| script.saved[0].clone());
    assert_eq!(entry, "fixture-emulated-entry-0-0");
    assert_eq!(settings.default_action.as_deref(), Some("vr"));
    assert!(!settings.ask);
}
