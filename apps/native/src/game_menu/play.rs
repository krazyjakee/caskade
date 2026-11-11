//! Starting a game from its menu, card or quick look (#99,
//! `GameMenuHost.tsx`'s `PlayRequest`, `components/LaunchChoiceDialog.tsx`):
//! the entry Play starts, a choice when the launch asks for one (a launch
//! action, an emulator, a disc), and why it didn't start when it didn't.

use super::actions;
use super::entries::{entries, playable};
use crate::data::use_game;
use crate::ui::{Button, ButtonVariant, Dialog, RadioGroup, RadioOption};
use caskade_app::commands::launch::LaunchOutcome;
use caskade_core::emulation::ProfileRef;
use caskade_core::launch::{LaunchChoice, LaunchPrompt};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use std::collections::HashSet;

/// Where a play request is.
#[derive(Clone, PartialEq)]
enum Stage {
    /// Waiting for the game's details, to pick the entry.
    Waiting,
    Launching,
    Choose(Box<LaunchPrompt>),
    Failed(String),
}

/// Starts `game_id`'s entry `entry_id` (else the one Play starts), with
/// `action` when one was picked; asks when the launch asks, says why when it
/// fails. `starting` is the game shown as starting meanwhile.
#[component]
pub fn PlayRequest(
    game_id: String,
    title: String,
    entry_id: Option<String>,
    action: Option<String>,
    starting: Signal<Option<String>>,
    on_close: EventHandler<()>,
) -> Element {
    let game = use_game(game_id.clone());
    let mut stage = use_signal(|| Stage::Waiting);
    let mut started = use_signal(|| false);
    // The entry, once known.
    let entry = entry_id.clone().or_else(|| {
        game.read().as_ref().and_then(|game| {
            let game = game.as_ref().ok()?.as_ref()?;
            let list = entries(game);
            playable(&list, &HashSet::new(), game.game.default_entry.as_deref())
                .map(|entry| entry.id.clone())
        })
    });
    let loaded = game.read().is_some();
    let launch = {
        let game_id = game_id.clone();
        use_callback(move |(entry, choice): (String, LaunchChoice)| {
            stage.set(Stage::Launching);
            starting.set(Some(game_id.clone()));
            let game_id = game_id.clone();
            spawn(async move {
                let outcome = actions::launch(entry, Some(choice)).await;
                if starting.peek().as_deref() == Some(game_id.as_str()) {
                    starting.set(None);
                }
                match outcome {
                    Ok(LaunchOutcome::Started { .. }) => on_close.call(()),
                    Ok(LaunchOutcome::Choose { prompt }) => {
                        stage.set(Stage::Choose(Box::new(prompt)))
                    }
                    Ok(LaunchOutcome::AgeRestricted { .. }) => {
                        stage.set(Stage::Failed(t!("Admin PIN needed")))
                    }
                    Ok(LaunchOutcome::HostBusy { .. }) => stage.set(Stage::Failed(t!(
                        "This device is streaming a game to another one."
                    ))),
                    Err(report) => stage.set(Stage::Failed(report.summary)),
                }
            });
        })
    };
    // Once, when the entry is known (or it's clear there's none).
    if !*started.peek() && *stage.peek() == Stage::Waiting {
        if let Some(entry) = entry.clone() {
            started.set(true);
            launch((
                entry,
                LaunchChoice {
                    action: action.clone(),
                    ..LaunchChoice::default()
                },
            ));
        } else if loaded {
            started.set(true);
            stage.set(Stage::Failed(t!(
                "It has nothing installed that can launch."
            )));
        }
    }

    match stage() {
        Stage::Waiting | Stage::Launching => rsx! {},
        Stage::Choose(prompt) => rsx! {
            LaunchChoiceDialog {
                prompt: *prompt,
                action: action.clone(),
                on_close: move |_| on_close.call(()),
                on_choose: move |choice: LaunchChoice| {
                    if let Some(entry) = entry.clone() {
                        launch((entry, choice));
                    }
                },
            }
        },
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
