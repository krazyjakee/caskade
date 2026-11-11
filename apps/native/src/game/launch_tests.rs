//! What Play asks before a game starts (#100), headless in the frame over
//! the fixture library with the app's answers scripted, as `launch.test.tsx`
//! has them: the update offer, the game running on another device, the
//! saves (a conflict, one on its way), and stopping a streaming device's
//! game.

use super::launch::{Answer, LaunchSteps};
use crate::frame::tests::{ONE, Window};
use caskade_app::commands::launch::LaunchOutcome;
use caskade_app::commands::presence::PlayingElsewhere;
use caskade_core::error::ErrorReport;
use caskade_core::launch::{HostBusy, LaunchChoice, LaunchTicket};
use caskade_core::saves::versions::{BeforePlay, SaveCheck, SaveVersion, VersionReason};
use caskade_core::updates::GameUpdate;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Mutex;

/// What the app answers, and what it was asked.
#[derive(Default)]
struct Script {
    update: Option<GameUpdate>,
    playing: Vec<PlayingElsewhere>,
    /// The saves' answers in turn; then no saves.
    saves: VecDeque<BeforePlay>,
    /// The launch's answers in turn; then it isn't available (fixtures
    /// mode's answer, which other tests expect).
    outcomes: VecDeque<LaunchOutcome>,
    /// Looking again for a save never comes back (Skip's test).
    pause_forever: bool,
    skipped: Vec<(String, String)>,
    chosen: Vec<(String, String)>,
    launches: Vec<(String, LaunchChoice)>,
    save_checks: usize,
}

static SCRIPT: Mutex<Option<Script>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Script) -> T) -> T {
    let mut script = SCRIPT.lock().unwrap_or_else(|e| e.into_inner());
    f(script.get_or_insert_with(Script::default))
}

/// Sets the script for one test; the default comes back after it.
struct Scripted;

fn script(script: Script) -> Scripted {
    *SCRIPT.lock().unwrap_or_else(|e| e.into_inner()) = Some(script);
    Scripted
}

impl Drop for Scripted {
    fn drop(&mut self) {
        *SCRIPT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn ready<T: 'static>(value: Result<T, ErrorReport>) -> Answer<T> {
    Box::pin(async move { value })
}

/// The steps every test window's Play asks (`frame::tests`' app provides
/// them).
pub(crate) fn fake_steps() -> LaunchSteps {
    LaunchSteps {
        update_before_play: Rc::new(|_| ready(Ok(with(|script| script.update.clone())))),
        skip_update: Rc::new(|entry, version| {
            with(|script| script.skipped.push((entry, version)));
            ready(Ok(()))
        }),
        install_update: Rc::new(|_| ready(Ok("job-1".to_owned()))),
        playing_elsewhere: Rc::new(|| ready(Ok(with(|script| script.playing.clone())))),
        saves_before_play: Rc::new(|_| {
            let answer = with(|script| {
                script.save_checks += 1;
                script.saves.pop_front()
            });
            ready(Ok(answer.unwrap_or(BeforePlay {
                check: SaveCheck::NoSaves,
                checked: true,
                warnings: Vec::new(),
            })))
        }),
        choose_save: Rc::new(|entry, version| {
            with(|script| script.chosen.push((entry, version)));
            ready(Ok(()))
        }),
        launch: Rc::new(|entry, choice| {
            let outcome = with(|script| {
                script.launches.push((entry, choice));
                script.outcomes.pop_front()
            });
            ready(outcome.ok_or_else(|| {
                ErrorReport::from("That isn't available in this copy of Caskade.".to_owned())
            }))
        }),
        pause: Rc::new(|_| {
            if with(|script| script.pause_forever) {
                Box::pin(std::future::pending())
            } else {
                ready(Ok(()))
            }
        }),
    }
}

fn started(game_id: &str) -> LaunchOutcome {
    LaunchOutcome::Started {
        ticket: LaunchTicket {
            game_id: game_id.to_owned(),
            entry_id: String::new(),
            session_id: "session".to_owned(),
        },
    }
}

fn version(id: &str, local: bool) -> SaveVersion {
    SaveVersion {
        id: id.to_owned(),
        game_id: String::new(),
        account_id: String::new(),
        device_id: if local {
            "local".to_owned()
        } else {
            "den".to_owned()
        },
        session_id: None,
        created_at_ms: 1_700_000_000_000,
        content_hash: format!("sha256:{id}"),
        size_bytes: 2048,
        file_count: 1,
        parents: Vec::new(),
        reason: VersionReason::Session,
        local,
    }
}

/// The library, with its first game's id.
fn library() -> (Window, String) {
    let mut window = Window::open();
    window.go("/library?filter=all&sort=title&layout=grid");
    window.settle_until("a card", |window| {
        !window.find_all("data-index", "0").is_empty()
    });
    let card = window.get("data-index", "0");
    let id = window
        .attr(card, "data-focus-key")
        .and_then(|key| key.strip_prefix("game-").map(str::to_owned))
        .expect("the card's game");
    (window, id)
}

fn play(window: &mut Window) {
    let card = window.get("data-index", "0");
    window.double_click(card);
}

fn launched(window: &mut Window, times: usize) {
    window.settle_until("the launch", |_| {
        with(|script| script.launches.len()) >= times
    });
}

fn press(window: &mut Window, dialog: &str, part: &str) {
    window.settle_until(dialog, |window| window.find("data-cs", dialog).is_some());
    let root = window.get("data-cs", dialog);
    let button = window
        .find_in(root, "data-part", part)
        .unwrap_or_else(|| panic!("no {part} in {dialog}"));
    window.click(button);
}

#[test]
fn a_newer_release_is_offered_before_play_and_play_now_starts_it() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let (mut window, game) = library();
    let _script = script(Script {
        update: Some(GameUpdate {
            entry_id: "e".to_owned(),
            game_id: game.clone(),
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
        outcomes: [started(&game)].into(),
        ..Script::default()
    });
    play(&mut window);
    window.settle_until("the offer", |window| {
        window.find("data-cs", "update-offer").is_some()
    });
    let offer = window.text(window.get("data-cs", "update-offer"));
    assert!(offer.contains("v1.2 is out, and you have v1.1."), "{offer}");
    assert_eq!(with(|script| script.launches.len()), 0);
    press(&mut window, "update-offer", "play-now");
    launched(&mut window, 1);
    window.settle_until("the offer gone", |window| {
        window.find("data-cs", "update-offer").is_none()
    });
}

#[test]
fn skipping_a_release_stops_its_offer_and_plays() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let (mut window, game) = library();
    let _script = script(Script {
        update: Some(GameUpdate {
            entry_id: "e".to_owned(),
            game_id: game.clone(),
            provider: "github".to_owned(),
            project: "tide/water".to_owned(),
            project_url: None,
            installed: None,
            latest: Some("2.0".to_owned()),
            latest_url: None,
            available: true,
            skipped: None,
            prereleases: false,
            checked_at_ms: 0,
            error: None,
        }),
        outcomes: [started(&game)].into(),
        ..Script::default()
    });
    play(&mut window);
    press(&mut window, "update-offer", "skip");
    launched(&mut window, 1);
    let skipped = with(|script| script.skipped.clone());
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].1, "2.0");
}

#[test]
fn a_game_running_on_another_device_warns_and_plays_anyway() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let (mut window, game) = library();
    let _script = script(Script {
        playing: vec![PlayingElsewhere {
            device_id: "den".to_owned(),
            device_name: "Den PC".to_owned(),
            game_id: game.clone(),
            title: "Tidewater".to_owned(),
            account_id: String::new(),
            started_at_ms: 1_700_000_000_000,
        }],
        outcomes: [started(&game)].into(),
        ..Script::default()
    });
    play(&mut window);
    window.settle_until("the warning", |window| {
        window.find("data-cs", "playing-elsewhere").is_some()
    });
    let warning = window.text(window.get("data-cs", "playing-elsewhere"));
    assert!(
        warning.contains("Tidewater is running on Den PC"),
        "{warning}"
    );
    assert_eq!(with(|script| script.launches.len()), 0);
    press(&mut window, "playing-elsewhere", "play-anyway");
    launched(&mut window, 1);
}

#[test]
fn when_both_devices_played_play_asks_which_save_and_plays_with_it() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let (mut window, game) = library();
    let _script = script(Script {
        saves: [BeforePlay {
            check: SaveCheck::Conflict {
                versions: vec![version("theirs", false), version("ours", true)],
            },
            checked: true,
            warnings: Vec::new(),
        }]
        .into(),
        outcomes: [started(&game)].into(),
        ..Script::default()
    });
    play(&mut window);
    window.settle_until("the question", |window| {
        window.find("data-cs", "save-conflict").is_some()
    });
    let question = window.text(window.get("data-cs", "save-conflict"));
    assert!(question.contains("This device"), "{question}");
    assert!(question.contains("Another device"), "{question}");
    // This device's save is chosen to begin with.
    press(&mut window, "save-conflict", "choose");
    launched(&mut window, 1);
    let chosen = with(|script| script.chosen.clone());
    assert_eq!(chosen.len(), 1);
    assert_eq!(chosen[0].1, "ours");
}

#[test]
fn a_save_on_its_way_is_waited_for_then_the_game_starts() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let (mut window, game) = library();
    let _script = script(Script {
        saves: [
            BeforePlay {
                check: SaveCheck::NotArrived {
                    version: version("theirs", false),
                },
                checked: true,
                warnings: Vec::new(),
            },
            BeforePlay {
                check: SaveCheck::Restored {
                    version: version("theirs", false),
                },
                checked: true,
                warnings: Vec::new(),
            },
        ]
        .into(),
        outcomes: [started(&game)].into(),
        ..Script::default()
    });
    play(&mut window);
    launched(&mut window, 1);
    // It looked again, found the save in place, and says so.
    assert_eq!(with(|script| script.save_checks), 2);
    window.settle_until("the note", |window| {
        window.find_all("role", "status").into_iter().any(|node| {
            window
                .text(node)
                .contains("Started with the save from Another device.")
        })
    });
}

#[test]
fn skip_plays_with_the_save_here_while_another_is_on_its_way() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let (mut window, game) = library();
    let _script = script(Script {
        saves: [BeforePlay {
            check: SaveCheck::NotArrived {
                version: version("theirs", false),
            },
            checked: true,
            warnings: Vec::new(),
        }]
        .into(),
        outcomes: [started(&game)].into(),
        pause_forever: true,
        ..Script::default()
    });
    play(&mut window);
    press(&mut window, "save-arriving", "skip");
    launched(&mut window, 1);
    window.settle_until("the note", |window| {
        window
            .find_all("role", "status")
            .into_iter()
            .any(|node| window.text(node).contains("hasn't arrived yet"))
    });
}

#[test]
fn a_busy_streaming_device_is_asked_to_stop_and_the_game_launches_again() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let (mut window, game) = library();
    let _script = script(Script {
        outcomes: [
            LaunchOutcome::HostBusy {
                busy: HostBusy {
                    entry_id: "e".to_owned(),
                    device: "Den PC".to_owned(),
                    playing: Some("Celeste".to_owned()),
                },
            },
            started(&game),
        ]
        .into(),
        ..Script::default()
    });
    play(&mut window);
    launched(&mut window, 1);
    window.settle_until("the question", |window| {
        window
            .find("data-part", "question")
            .is_some_and(|question| window.text(question).contains("Den PC is playing Celeste."))
    });
    let stop = window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| window.text(button).contains("Stop"))
        .expect("Stop and stream");
    window.click(stop);
    launched(&mut window, 2);
    let second = with(|script| script.launches[1].1.clone());
    assert!(second.stop_host);
}
