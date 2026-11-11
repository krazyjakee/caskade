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
    pub(crate) saves: Option<caskade_core::saves::versions::SaveHistory>,
    pub(crate) save_paths: Vec<String>,
    pub(crate) restored: Vec<(String, String)>,
    pub(crate) save_chosen: Vec<(String, String)>,
    pub(crate) revealed: Vec<(String, u32)>,
    pub(crate) igdb: Option<caskade_core::metadata::GameIgdb>,
    pub(crate) igdb_changes: Vec<super::data::IgdbChange>,
    pub(crate) igdb_results: Vec<caskade_core::igdb::IgdbGame>,
    pub(crate) searches: Vec<String>,
    pub(crate) ratings: Vec<caskade_core::igdb::IgdbAgeRating>,
    pub(crate) videos_played: Vec<(String, String)>,
    pub(crate) emulations: Vec<caskade_core::emulation::GameEmulation>,
    pub(crate) emulator_set: Vec<(String, caskade_core::emulation::EmulatorTarget)>,
    pub(crate) owned: Vec<caskade_core::store_accounts::OwnedRelease>,
    pub(crate) owned_installs: Vec<String>,
    pub(crate) playtime: Option<caskade_core::db::PlaytimeBreakdown>,
    pub(crate) copies: Vec<caskade_core::sync::copies::PeerCopy>,
    pub(crate) copied: Vec<String>,
    pub(crate) played_on: Vec<String>,
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
        emulation: Rc::new(|entry_id| {
            answer(page(|script| {
                script
                    .emulations
                    .iter()
                    .find(|each| each.entry_id == entry_id)
                    .cloned()
                    .unwrap_or_else(|| ready_emulation(&entry_id))
            }))
        }),
        set_emulation: Rc::new(|entry_id, target, _| {
            let emulation = page(|script| {
                script.emulator_set.push((entry_id.clone(), target.clone()));
                ready_emulation(&entry_id)
            });
            answer(emulation)
        }),
        split_args: Rc::new(|text| answer(text.split_whitespace().map(str::to_owned).collect())),
        owned: Rc::new(|_| answer(page(|script| script.owned.clone()))),
        owned_install: Rc::new(|entry_id| {
            page(|script| script.owned_installs.push(entry_id));
            answer(())
        }),
        owned_cancel: Rc::new(|_| answer(())),
        playtime: Rc::new(|_| answer(page(|script| script.playtime.clone().unwrap_or_default()))),
        copies: Rc::new(|_| answer(page(|script| script.copies.clone()))),
        copy_here: Rc::new(|_, device| {
            page(|script| script.copied.push(device));
            answer(())
        }),
        play_on: Rc::new(|_, device| {
            page(|script| script.played_on.push(device));
            answer(())
        }),
        igdb: Rc::new(|_| answer(page(|script| script.igdb.clone()))),
        igdb_change: Rc::new(|_, what| {
            answer(page(|script| {
                script.igdb_changes.push(what);
                script.igdb.clone()
            }))
        }),
        igdb_search: Rc::new(|query| {
            answer(page(|script| {
                script.searches.push(query);
                script.igdb_results.clone()
            }))
        }),
        age_ratings: Rc::new(|_| answer(page(|script| script.ratings.clone()))),
        play_video: Rc::new(|video, title| {
            page(|script| script.videos_played.push((video, title)));
            Ok(())
        }),
        save_history: Rc::new(|_| answer(page(|script| script.saves.clone()))),
        save_locations: Rc::new(|_| answer(page(|script| script.save_paths.clone()))),
        save_restore: Rc::new(|entry, version| {
            page(|script| script.restored.push((entry, version)));
            answer(())
        }),
        save_choose: Rc::new(|entry, version| {
            page(|script| script.save_chosen.push((entry, version)));
            answer(())
        }),
        save_reveal: Rc::new(|entry, location| {
            page(|script| script.revealed.push((entry, location)));
            answer(())
        }),
        accounts: Rc::new(|| answer(vec![("acct-1".to_owned(), "Robin".to_owned())])),
    }
}

/// An emulated release that can start, with one profile to choose.
pub(crate) fn ready_emulation(entry_id: &str) -> caskade_core::emulation::GameEmulation {
    use caskade_core::emulation::{
        ArgsOverride, CompatibleProfile, EmulatorTarget, GameEmulation, Readiness,
    };
    let profile = CompatibleProfile {
        emulator_id: "retroarch".to_owned(),
        profile_id: "mupen".to_owned(),
        emulator_name: "RetroArch".to_owned(),
        profile_name: "Mupen64Plus".to_owned(),
        available: true,
    };
    GameEmulation {
        entry_id: entry_id.to_owned(),
        platform_id: "nintendo_n64".to_owned(),
        platform: None,
        target: EmulatorTarget::PlatformDefault,
        args: ArgsOverride::Default,
        discs: Vec::new(),
        profile: Some(profile.clone()),
        compatible: vec![profile],
        bios: Vec::new(),
        readiness: Readiness::Ready,
        message: None,
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

#[test]
fn an_emulated_release_shows_its_emulator_and_picks_another() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript::default());
    let mut window = open("/games/fixture-emulated-0-0");
    window.settle_until("the emulator", |window| {
        window.find("data-cs", "emulation").is_some()
    });
    let panel = window.get("data-cs", "emulation");
    assert_eq!(window.attr(panel, "data-status").as_deref(), Some("ready"));
    assert!(
        window.text(panel).contains("Ready"),
        "{}",
        window.text(panel)
    );
    let select = window
        .find_in(panel, "data-part", "launch-with")
        .expect("Launch with");
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
    window.settle_until("the emulator set", |_| {
        page(|script| script.emulator_set.len()) == 1
    });
    let (_, target) = page(|script| script.emulator_set[0].clone());
    assert_eq!(
        target,
        caskade_core::emulation::EmulatorTarget::Profile {
            emulator_id: "retroarch".to_owned(),
            profile_id: "mupen".to_owned(),
        }
    );
}

#[test]
fn an_emulated_release_that_needs_setup_says_how_and_play_waits() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut emulation = ready_emulation("fixture-emulated-entry-0-0");
    emulation.readiness = caskade_core::emulation::Readiness::NoEmulator;
    emulation.message = Some("No emulator for N64".to_owned());
    let _script = page_script(PageScript {
        emulations: vec![emulation],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-emulated-0-0");
    window.settle_until("the setup note", |window| {
        window.find("data-part", "readiness").is_some()
    });
    let note = window.text(window.get("data-part", "readiness"));
    assert!(
        note.contains("No emulator for N64") && note.contains("Configure"),
        "{note}"
    );
    window.settle_until("Play waiting", |window| {
        window.attr(play_button(window), "data-disabled").as_deref() == Some("true")
    });
}

#[test]
fn an_owned_release_installs_through_its_store() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::store_accounts::{OwnedInstallState, OwnedRelease};
    let _script = page_script(PageScript {
        owned: vec![OwnedRelease {
            entry_id: "owned-1".to_owned(),
            store_account_id: "steam-me".to_owned(),
            provider: "steam".to_owned(),
            provider_name: "Steam".to_owned(),
            account_name: "me".to_owned(),
            product_id: "1".to_owned(),
            title: "Fixture Game 0001".to_owned(),
            state: OwnedInstallState::Available {
                launcher: "Steam".to_owned(),
            },
            refreshed_at_ms: None,
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-0");
    window.settle_until("the owned release", |window| {
        window.find("data-part", "owned-release").is_some()
    });
    let owned = window.get("data-part", "owned-release");
    assert!(
        window.text(owned).contains("Owned on Steam (me)"),
        "{}",
        window.text(owned)
    );
    let install = window
        .find_in(owned, "data-part", "install")
        .expect("Install with Steam");
    assert!(window.text(install).contains("Install with Steam"));
    window.click(install);
    window.settle_until("the install", |_| {
        page(|script| script.owned_installs.len()) == 1
    });
}

#[test]
fn a_copy_on_another_device_copies_here_or_plays_there() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::db::CopyLocation;
    use caskade_core::devices::{DeviceKind, DeviceLink, DeviceOs};
    use caskade_core::sync::copies::PeerCopy;
    let _script = page_script(PageScript {
        copies: vec![PeerCopy {
            device_id: "den".to_owned(),
            device_name: "Den PC".to_owned(),
            kind: DeviceKind::Desktop,
            os: DeviceOs::Linux,
            link: DeviceLink::Lan,
            size_bytes: Some(2 * 1024 * 1024 * 1024),
            location: CopyLocation::Managed,
            blocked: None,
            through_proton: false,
            playing: false,
            wakeable: false,
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-0");
    window.settle_until("the copy", |window| {
        window.find("data-part", "peer-copy").is_some()
    });
    let row = window.get("data-part", "peer-copy");
    assert!(
        window.text(row).contains("On Den PC"),
        "{}",
        window.text(row)
    );
    let copy = window
        .find_in(row, "data-part", "copy-here")
        .expect("Copy to this computer");
    window.click(copy);
    window.settle_until("the copy", |_| page(|script| script.copied.len()) == 1);
    let row = window.get("data-part", "peer-copy");
    let play = window
        .find_in(row, "data-part", "play-there")
        .expect("Play on Den PC");
    window.click(play);
    window.settle_until("played there", |_| {
        page(|script| script.played_on.len()) == 1
    });
}

#[test]
fn play_time_reported_by_stores_says_where_it_comes_from() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::db::{PlaytimeBreakdown, ReportedPlaytime};
    let _script = page_script(PageScript {
        playtime: Some(PlaytimeBreakdown {
            total_ms: 3 * 3_600_000,
            tracked_ms: 3_600_000,
            reported_ms: 2 * 3_600_000,
            last_played_ms: None,
            sources: vec![ReportedPlaytime {
                provider: "steam".to_owned(),
                account_name: Some("me".to_owned()),
                total_ms: Some(2 * 3_600_000),
                counted_ms: 2 * 3_600_000,
                last_played_ms: None,
                observed_at_ms: 1_700_000_000_000,
            }],
        }),
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the sources", |window| {
        window.find("data-part", "playtime-sources").is_some()
    });
    let said = text(&window, "playtime-sources");
    assert!(
        said.contains("in Caskade") && said.contains("more reported by Steam (me)"),
        "{said}"
    );
}

fn matched(game_id: &str) -> caskade_core::metadata::GameIgdb {
    caskade_core::metadata::GameIgdb {
        game_id: game_id.to_owned(),
        igdb_id: Some(1942),
        source: caskade_core::metadata::MatchSource::Store,
        pending: false,
        game: Some(caskade_core::igdb::IgdbGame {
            id: 1942,
            title: "Tidewater".to_owned(),
            release_date: Some("2024-02-29".to_owned()),
            total_rating: Some(86.4),
            genres: vec!["Adventure".to_owned()],
            videos: vec![caskade_core::igdb::IgdbVideo {
                video_id: "dQw4w9WgXcQ".to_owned(),
                name: Some("Trailer".to_owned()),
            }],
            ..caskade_core::igdb::IgdbGame::default()
        }),
    }
}

#[test]
fn a_matched_game_shows_igdbs_facts_and_changes_its_match() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript {
        igdb: Some(matched("fixture-1")),
        igdb_results: vec![caskade_core::igdb::IgdbGame {
            id: 77,
            title: "Tidewater Remastered".to_owned(),
            ..caskade_core::igdb::IgdbGame::default()
        }],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the facts", |window| {
        window.find("data-part", "igdb-facts").is_some()
    });
    let facts = text(&window, "igdb-facts");
    assert!(
        facts.contains("Tidewater") && facts.contains("86 / 100"),
        "{facts}"
    );
    let source = text(&window, "igdb-source");
    assert!(
        source.contains("Matched by store id") && source.contains("IGDB ID 1942"),
        "{source}"
    );
    // Not on IGDB.
    let none = window.get("data-part", "igdb-none");
    window.click(none);
    window.settle_until("the change", |_| {
        page(|script| script.igdb_changes.len()) == 1
    });
    assert_eq!(
        page(|script| script.igdb_changes[0].clone()),
        super::data::IgdbChange::Set(None)
    );
    // Change IGDB game: searches the title, then uses a result.
    let change = window.get("data-part", "igdb-change");
    window.click(change);
    window.settle_until("the results", |window| {
        window.find("data-part", "igdb-result").is_some()
    });
    assert_eq!(
        page(|script| script.searches.clone()),
        ["Fixture Game 0002"]
    );
    let result = window.get("data-part", "igdb-result");
    let use_it = window.find_in(result, "data-part", "use").expect("Use");
    window.click(use_it);
    window.settle_until("the new match", |_| {
        page(|script| script.igdb_changes.len()) == 2
    });
    assert_eq!(
        page(|script| script.igdb_changes[1].clone()),
        super::data::IgdbChange::Set(Some("77".to_owned()))
    );
}

#[test]
fn igdbs_videos_play_in_the_video_window_and_the_age_rating_shows() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = page_script(PageScript {
        igdb: Some(matched("fixture-1")),
        ratings: vec![
            caskade_core::igdb::IgdbAgeRating {
                board: "esrb".to_owned(),
                rating: "T".to_owned(),
            },
            caskade_core::igdb::IgdbAgeRating {
                board: "pegi".to_owned(),
                rating: "12".to_owned(),
            },
        ],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-1");
    window.settle_until("the rating", |window| {
        text(window, "meta").contains("PEGI 12")
    });
    window.settle_until("the videos", |window| {
        window.find("data-cs", "videos").is_some()
    });
    let video = window.get("data-part", "video");
    assert!(window.text(video).contains("Trailer"));
    window.click(video);
    window.settle_until("the video", |_| {
        page(|script| script.videos_played.len()) == 1
    });
    assert_eq!(
        page(|script| script.videos_played[0].clone()),
        (
            "dQw4w9WgXcQ".to_owned(),
            "Fixture Game 0002 · Trailer".to_owned()
        )
    );
    // No screenshots in the fixtures: the empty grid says so.
    assert!(window.find("data-part", "screenshots-empty").is_some());
}

#[test]
fn the_saves_panel_restores_shows_where_they_are_and_settles_a_conflict() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    use caskade_core::saves::versions::{SaveHistory, VersionState};
    let ours = super::launch_tests::version("ours", true);
    let theirs = super::launch_tests::version("theirs", false);
    let _script = page_script(PageScript {
        saves: Some(SaveHistory {
            account_id: String::new(),
            versions: vec![
                VersionState {
                    version: ours.clone(),
                    current: true,
                    complete: true,
                },
                VersionState {
                    version: theirs.clone(),
                    current: false,
                    complete: true,
                },
            ],
            conflict: vec!["ours".to_owned(), "theirs".to_owned()],
            syncing: true,
        }),
        save_paths: vec!["/home/me/.local/share/Tidewater/saves".to_owned()],
        ..PageScript::default()
    });
    let mut window = open("/games/fixture-emulated-0-0");
    window.settle_until("the saves", |window| {
        window.find("data-cs", "saves-panel").is_some()
    });
    let panel = window.get("data-cs", "saves-panel");
    let said = window.text(panel);
    assert!(
        said.contains("In use") && said.contains("After playing"),
        "{said}"
    );
    assert!(
        said.contains("/home/me/.local/share/Tidewater/saves"),
        "{said}"
    );
    // Show in folder.
    let reveal = window
        .find_in(panel, "data-part", "reveal")
        .expect("Show in folder");
    window.click(reveal);
    window.settle_until("revealed", |_| page(|script| script.revealed.len()) == 1);
    // Restore the other device's.
    let versions = window.find_all("data-part", "version");
    let theirs_row = versions
        .into_iter()
        .find(|&row| window.attr(row, "data-status").as_deref() == Some("kept"))
        .expect("the kept version");
    let restore = window
        .find_in(theirs_row, "data-part", "restore")
        .expect("Restore");
    window.click(restore);
    window.settle_until("restored", |_| page(|script| script.restored.len()) == 1);
    assert_eq!(page(|script| script.restored[0].1.clone()), "theirs");
    // Both played: choose one.
    let panel = window.get("data-cs", "saves-panel");
    let conflict = window
        .find_in(panel, "data-part", "conflict")
        .expect("the conflict");
    let choose = window
        .find_in(conflict, "data-part", "choose")
        .expect("Choose a save…");
    window.click(choose);
    window.settle_until("the question", |window| {
        window.find("data-cs", "save-conflict").is_some()
    });
    let dialog = window.get("data-cs", "save-conflict");
    let confirm = window
        .find_in(dialog, "data-part", "choose")
        .expect("Use this save");
    assert!(window.text(confirm).contains("Use this save"));
    window.click(confirm);
    window.settle_until("chosen", |_| page(|script| script.save_chosen.len()) == 1);
    assert_eq!(page(|script| script.save_chosen[0].1.clone()), "ours");
}
