//! Starting a game (#99, #100; the webview's `views/game/launch.tsx`,
//! `GameMenuHost.tsx`'s `PlayRequest`, `components/LaunchChoiceDialog.tsx`):
//! the entry Play starts, then what Play asks before it starts
//! (`crate::game::launch`: a newer release's offer, the game running on
//! another device, the saves), the launch, a choice when the launch asks
//! for one (a launch action, an emulator, a disc), stopping the game a
//! streaming device plays, and why it didn't start when it didn't. A
//! release streamed from another device skips the offer, the warning and
//! the saves, which are that device's.

use crate::data::use_game;
use crate::flows::streaming::{HostBusy, StopHostDialog};
use crate::game::launch::{
    PlayingElsewhereDialog, SaveConflictDialog, UpdateOffer, WaitForSave, save_note,
    use_launch_steps, use_save_device_name,
};
use crate::ui::{Button, ButtonVariant, Dialog, RadioGroup, RadioOption};
use caskade_app::commands::launch::LaunchOutcome;
use caskade_app::commands::presence::PlayingElsewhere;
use caskade_core::emulation::ProfileRef;
use caskade_core::launch::{LaunchChoice, LaunchPrompt};
use caskade_core::saves::versions::{BeforePlay, SaveCheck, SaveVersion};
use caskade_core::updates::GameUpdate;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use std::collections::HashSet;
use std::time::Duration;

use super::entries::{entries, playable};

/// While another device's save is on its way: how often to look, and for
/// how long before the game starts with the save here.
const ARRIVING_POLL: Duration = Duration::from_secs(2);
const ARRIVING_LOOKS: u32 = 30;

/// Where a play request is.
#[derive(Clone, PartialEq)]
enum Stage {
    /// Waiting for the game's details, to pick the entry.
    Waiting,
    /// A step is asking the app.
    Checking,
    /// Update before playing?
    Update(Box<GameUpdate>),
    /// It's running on another device.
    Elsewhere(Vec<PlayingElsewhere>),
    /// Both devices played: which save?
    Conflict(Vec<SaveVersion>),
    /// Another device's newer save is on its way.
    Arriving(Box<SaveVersion>),
    Launching,
    Choose(Box<LaunchPrompt>),
    /// The streaming device plays another game: stop it?
    HostBusy(HostBusy, LaunchChoice),
    Failed(String),
}

/// Starts `game_id`'s entry `entry_id` (else the one Play starts), with
/// `action` when one was picked: asks what Play asks first, then when the
/// launch asks, and says why when it fails. `starting` is the game shown as
/// starting meanwhile.
#[component]
pub fn PlayRequest(
    game_id: String,
    title: String,
    entry_id: Option<String>,
    action: Option<String>,
    starting: Signal<Option<String>>,
    on_close: EventHandler<()>,
    /// How it went, once (another device's request, #108): started; or why
    /// not, `None` when the choice was closed unanswered. With it, a
    /// failure goes there instead of into a dialog here.
    #[props(default)]
    on_outcome: Option<EventHandler<Result<(), Option<String>>>>,
) -> Element {
    let steps = use_launch_steps();
    let mut stage = use_signal(|| Stage::Waiting);
    // Settling a save conflict.
    let mut choosing = use_signal(|| false);
    let mut save_error = use_signal(|| None::<String>);
    let toasts = try_use_context::<crate::ui::Toasts>();
    let device_name = use_save_device_name();
    let tell = move |result: Result<(), Option<String>>| {
        if let Some(on_outcome) = on_outcome {
            on_outcome.call(result);
        }
    };
    let mut fail = move |reason: String| {
        if on_outcome.is_some() {
            tell(Err(Some(reason)));
        } else {
            stage.set(Stage::Failed(reason));
        }
    };
    // Closed before it started: the asking device hears it wasn't.
    let give_up = move || {
        tell(Err(None));
        on_close.call(());
    };
    let game = use_game(game_id.clone());
    let mut started = use_signal(|| false);
    let details = game
        .read()
        .as_ref()
        .and_then(|game| game.as_ref().ok().cloned().flatten());
    // The entry, once known.
    let entry = entry_id.clone().or_else(|| {
        let game = details.as_ref()?;
        let list = entries(game);
        playable(&list, &HashSet::new(), game.game.default_entry.as_deref())
            .map(|entry| entry.id.clone())
    });
    let streamed = entry.as_ref().is_some_and(|id| {
        details.as_ref().is_some_and(|game| {
            game.entries
                .iter()
                .any(|each| &each.id == id && each.source_id.as_str().starts_with("stream:"))
        })
    });
    let loaded = game.read().is_some();

    // The launch itself, with every answer so far.
    let launch = {
        let game_id = game_id.clone();
        let steps = steps.clone();
        use_callback(move |(entry, choice): (String, LaunchChoice)| {
            stage.set(Stage::Launching);
            starting.set(Some(game_id.clone()));
            let game_id = game_id.clone();
            let steps = steps.clone();
            spawn(async move {
                let outcome = (steps.launch)(entry, choice.clone()).await;
                if starting.peek().as_deref() == Some(game_id.as_str()) {
                    starting.set(None);
                }
                match outcome {
                    Ok(LaunchOutcome::Started { .. }) => {
                        tell(Ok(()));
                        on_close.call(());
                    }
                    Ok(LaunchOutcome::Choose { prompt }) => {
                        stage.set(Stage::Choose(Box::new(prompt)))
                    }
                    // The admin's PIN is #106's.
                    Ok(LaunchOutcome::AgeRestricted { .. }) => fail(t!("Admin PIN needed")),
                    Ok(LaunchOutcome::HostBusy { busy }) => stage.set(Stage::HostBusy(
                        HostBusy {
                            device: busy.device,
                            playing: busy.playing,
                        },
                        choice,
                    )),
                    Err(report) => fail(report.summary),
                }
            });
        })
    };
    let first_choice = LaunchChoice {
        action: action.clone(),
        ..LaunchChoice::default()
    };
    // The saves: a newer one put in place (said in a note), which to use
    // when both devices played, or waiting for one on its way. A save
    // check that fails never stops the launch.
    let settle = {
        let first_choice = first_choice.clone();
        let device_name = device_name.clone();
        use_callback(move |(entry, result, waited): (String, BeforePlay, bool)| {
            match &result.check {
                SaveCheck::Conflict { versions } => {
                    stage.set(Stage::Conflict(versions.clone()));
                    return;
                }
                SaveCheck::NotArrived { version } if !waited => {
                    stage.set(Stage::Arriving(Box::new(version.clone())));
                    return;
                }
                _ => {}
            }
            let note = save_note(&result, &device_name);
            if !note.is_empty()
                && let Some(toasts) = toasts
            {
                toasts.show(note.join(" "), false, None);
            }
            launch((entry, first_choice.clone()));
        })
    };
    let check_saves = {
        let steps = steps.clone();
        let first_choice = first_choice.clone();
        use_callback(move |entry: String| {
            stage.set(Stage::Checking);
            let steps = steps.clone();
            let first_choice = first_choice.clone();
            spawn(async move {
                match (steps.saves_before_play)(entry.clone()).await {
                    Ok(result) => settle((entry, result, false)),
                    Err(_) => launch((entry, first_choice)),
                }
            });
        })
    };
    // Running on another device: each keeps its own save if played here.
    // Presence never stops a launch.
    let check_elsewhere = {
        let steps = steps.clone();
        let game_id = game_id.clone();
        use_callback(move |entry: String| {
            stage.set(Stage::Checking);
            let steps = steps.clone();
            let game_id = game_id.clone();
            spawn(async move {
                let playing: Vec<PlayingElsewhere> = (steps.playing_elsewhere)()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|each| each.game_id == game_id)
                    .collect();
                if playing.is_empty() {
                    check_saves(entry);
                } else {
                    stage.set(Stage::Elsewhere(playing));
                }
            });
        })
    };
    // A newer release of a followed install: offered first. A check that
    // can't be made plays.
    let check_update = {
        let steps = steps.clone();
        use_callback(move |entry: String| {
            stage.set(Stage::Checking);
            let steps = steps.clone();
            spawn(async move {
                match (steps.update_before_play)(entry.clone()).await {
                    Ok(Some(update)) => stage.set(Stage::Update(Box::new(update))),
                    _ => check_elsewhere(entry),
                }
            });
        })
    };
    // Looking again for a save on its way, while its dialog is up.
    {
        let steps = steps.clone();
        let entry = entry.clone();
        use_effect(move || {
            let Stage::Arriving(version) = stage() else {
                return;
            };
            let Some(entry) = entry.clone() else { return };
            let steps = steps.clone();
            spawn(async move {
                let mut latest = BeforePlay {
                    check: SaveCheck::NotArrived {
                        version: (*version).clone(),
                    },
                    checked: true,
                    warnings: Vec::new(),
                };
                for _ in 0..ARRIVING_LOOKS {
                    let _ = (steps.pause)(ARRIVING_POLL).await;
                    // Skipped or cancelled meanwhile.
                    if !matches!(*stage.peek(), Stage::Arriving(_)) {
                        return;
                    }
                    if let Ok(result) = (steps.saves_before_play)(entry.clone()).await {
                        let arrived = !matches!(result.check, SaveCheck::NotArrived { .. });
                        latest = result;
                        if arrived {
                            break;
                        }
                    }
                }
                if matches!(*stage.peek(), Stage::Arriving(_)) {
                    settle((entry, latest, true));
                }
            });
        });
    }

    // Once, when the entry is known (or it's clear there's none).
    if !*started.peek() && *stage.peek() == Stage::Waiting {
        if let Some(entry) = entry.clone() {
            started.set(true);
            if streamed {
                launch((entry, first_choice.clone()));
            } else {
                check_update(entry);
            }
        } else if loaded {
            started.set(true);
            fail(t!("It has nothing installed that can launch."));
        }
    }

    let entry_now = entry.clone().unwrap_or_default();
    match stage() {
        Stage::Waiting | Stage::Checking | Stage::Launching => rsx! {},
        Stage::Update(update) => {
            let entry = entry_now.clone();
            rsx! {
                UpdateOffer {
                    update: *update,
                    on_close: move |_| give_up(),
                    on_play: move |_| check_elsewhere(entry.clone()),
                }
            }
        }
        Stage::Elsewhere(playing) => {
            let entry = entry_now.clone();
            rsx! {
                PlayingElsewhereDialog {
                    playing,
                    on_close: move |_| give_up(),
                    on_play: move |_| check_saves(entry.clone()),
                }
            }
        }
        Stage::Conflict(versions) => {
            let entry = entry_now.clone();
            let steps = steps.clone();
            let first_choice = first_choice.clone();
            rsx! {
                SaveConflictDialog {
                    versions,
                    busy: choosing(),
                    error: save_error(),
                    on_close: move |_| give_up(),
                    on_choose: move |version: String| {
                        let (steps, entry, first_choice) = (steps.clone(), entry.clone(), first_choice.clone());
                        choosing.set(true);
                        save_error.set(None);
                        spawn(async move {
                            match (steps.choose_save)(entry.clone(), version).await {
                                Ok(()) => launch((entry, first_choice)),
                                Err(report) => save_error.set(Some(report.summary)),
                            }
                            choosing.set(false);
                        });
                    },
                }
            }
        }
        Stage::Arriving(version) => {
            let entry = entry_now.clone();
            let skipped = BeforePlay {
                check: SaveCheck::NotArrived {
                    version: (*version).clone(),
                },
                checked: true,
                warnings: Vec::new(),
            };
            rsx! {
                WaitForSave {
                    version: *version,
                    on_close: move |_| give_up(),
                    on_skip: move |_| settle((entry.clone(), skipped.clone(), true)),
                }
            }
        }
        Stage::Choose(prompt) => {
            let entry = entry.clone();
            rsx! {
                LaunchChoiceDialog {
                    prompt: *prompt,
                    action: action.clone(),
                    on_close: move |_| give_up(),
                    on_choose: move |choice: LaunchChoice| {
                        if let Some(entry) = entry.clone() {
                            launch((entry, choice));
                        }
                    },
                }
            }
        }
        Stage::HostBusy(busy, choice) => {
            let entry = entry_now.clone();
            rsx! {
                StopHostDialog {
                    busy,
                    title: title.clone(),
                    launching: false,
                    on_close: move |_| give_up(),
                    on_confirm: move |_| {
                        launch((entry.clone(), LaunchChoice { stop_host: true, ..choice.clone() }))
                    },
                }
            }
        }
        Stage::Failed(reason) => rsx! {
            Dialog {
                title: t!("{title} didn't start", title = title),
                alert: true,
                on_close: move |_| on_close.call(()),
                p { class: "text-sm text-fg-muted", "{reason}" }
                div { class: "mt-5 flex justify-end",
                    Button {
                        variant: ButtonVariant::Primary,
                        "data-focus-default": "true",
                        on_press: move |_| on_close.call(()),
                        {t!("OK")}
                    }
                }
            }
        },
    }
}

/// How to play, when the launch asks: which launch action, which emulator
/// and which disc.
#[component]
fn LaunchChoiceDialog(
    prompt: LaunchPrompt,
    action: Option<String>,
    on_choose: EventHandler<LaunchChoice>,
    on_close: EventHandler<()>,
) -> Element {
    let default_action = action.clone().or_else(|| {
        prompt
            .actions
            .iter()
            .find(|option| option.default)
            .map(|option| option.id.clone())
    });
    let mut chosen_action = use_signal(|| default_action);
    let mut chosen_profile = use_signal(|| None::<String>);
    let mut chosen_disc = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let title = if !prompt.actions.is_empty() || !prompt.profiles.is_empty() {
        t!("Choose how to play")
    } else {
        t!("Choose a disc")
    };
    let complete = (prompt.actions.is_empty() || chosen_action().is_some())
        && (prompt.profiles.is_empty() || chosen_profile().is_some())
        && (prompt.discs.is_empty() || chosen_disc().is_some());
    let actions: Vec<RadioOption> = prompt
        .actions
        .iter()
        .map(|option| RadioOption {
            description: option.default.then(|| tp!("launch action", "Default")),
            ..RadioOption::new(option.id.clone(), option.name.clone())
        })
        .collect();
    let profiles: Vec<RadioOption> = prompt
        .profiles
        .iter()
        .map(|option| RadioOption {
            disabled: !option.installed,
            description: (!option.installed).then(|| t!("Not installed")),
            ..RadioOption::new(
                format!("{}/{}", option.emulator_id, option.profile_id),
                format!("{} · {}", option.emulator_name, option.profile_name),
            )
        })
        .collect();
    let discs: Vec<RadioOption> = prompt
        .discs
        .iter()
        .map(|option| RadioOption::new(option.index.to_string(), option.name.clone()))
        .collect();
    let play = move |_| {
        busy.set(true);
        let profile = chosen_profile().and_then(|value| {
            let (emulator_id, profile_id) = value.split_once('/')?;
            Some(ProfileRef {
                emulator_id: emulator_id.to_owned(),
                profile_id: profile_id.to_owned(),
            })
        });
        on_choose.call(LaunchChoice {
            profile,
            disc: chosen_disc().and_then(|disc| disc.parse().ok()),
            action: chosen_action(),
            ..LaunchChoice::default()
        });
    };
    rsx! {
        Dialog {
            title,
            on_close: move |_| on_close.call(()),
            div { class: "flex flex-col gap-4",
                if !actions.is_empty() {
                    RadioGroup {
                        label: tp!("launch choice", "Start with"),
                        options: actions,
                        value: chosen_action(),
                        on_change: move |value: String| chosen_action.set(Some(value)),
                    }
                }
                if !profiles.is_empty() {
                    RadioGroup {
                        label: tp!("launch choice", "Emulator"),
                        options: profiles,
                        value: chosen_profile(),
                        on_change: move |value: String| chosen_profile.set(Some(value)),
                    }
                }
                if !discs.is_empty() {
                    RadioGroup {
                        label: tp!("launch choice", "Disc"),
                        options: discs,
                        value: chosen_disc(),
                        on_change: move |value: String| chosen_disc.set(Some(value)),
                    }
                }
                div { class: "flex justify-end gap-2",
                    Button { on_press: move |_| on_close.call(()), {t!("Cancel")} }
                    Button {
                        variant: ButtonVariant::Primary,
                        disabled: !complete || busy(),
                        on_press: play,
                        if busy() {
                            {t!(note = "The Play button while the game starts", "Starting…")}
                        } else {
                            {tp!("game action", "Play")}
                        }
                    }
                }
            }
        }
    }
}
