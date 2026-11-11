//! The editor's Actions tab (the webview's `gameEditor/ActionsTab.tsx`):
//! each release's launch actions. Its own launch first, then the play
//! actions and tools the user added. Add, duplicate, reorder, rename and
//! delete them, choose what Play starts or have it ask, and save one action
//! at a time.

use caskade_core::launch::ReleaseActions;
use caskade_core::model::{
    ActionKind, ActionTracking, GameAction, GameDetails, LaunchSpec, built_in_action_id,
};
use caskade_ui_data::Key;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::action_form::ActionForm;
use super::data::EditData;
use super::{flag, release_label};
use crate::game::data::use_answer;
use crate::ui::icons::{ARROW_DOWN, ARROW_UP, COPY, GAMEPAD_2, PLUS, TRASH, WRENCH};
use crate::ui::{Button, ButtonVariant, Select, SelectOption, Svg};

const ASK: &str = "ask";

fn is_built_in(action: &GameAction) -> bool {
    action.id == built_in_action_id(&action.entry_id)
}

/// Whether the action runs a Windows program: its release's own launch
/// (which Proton runs where the release has it), or a program file.
fn runs_windows_program(action: &GameAction) -> bool {
    if is_built_in(action) {
        return true;
    }
    match &action.target {
        LaunchSpec::Executable { path, .. } => path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ["exe", "bat", "cmd", "com", "lnk", "msi"]
                    .iter()
                    .any(|known| ext.eq_ignore_ascii_case(known))
            }),
        _ => false,
    }
}

fn blank(entry_id: &str, kind: ActionKind, name: String) -> GameAction {
    GameAction {
        id: String::new(),
        entry_id: entry_id.to_owned(),
        name,
        kind,
        target: LaunchSpec::Executable {
            path: PathBuf::new(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
        },
        tracking: ActionTracking::default(),
        proton: None,
    }
}

#[component]
pub fn ActionsTab(
    game_id: String,
    details: GameDetails,
    data: EditData,
    on_dirty_change: EventHandler<bool>,
) -> Element {
    let query = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(Key::Game(game_id.clone()), move || {
            (data.actions)(game_id.clone())
        })
    };
    let proton = {
        let data = data.clone();
        use_resource(move || (data.proton)())
    };
    let mut entry_id = use_signal(|| None::<String>);
    // The action being edited: a saved one's id, or a new one's draft.
    let mut selected = use_signal(|| None::<String>);
    let mut draft = use_signal(|| None::<GameAction>);
    let mut confirm_delete = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut failure = use_signal(|| None::<String>);

    let loaded = query.read().clone();
    let releases: Vec<ReleaseActions> = match &loaded {
        Some(Ok(releases)) => releases.clone(),
        _ => Vec::new(),
    };
    let release = releases
        .iter()
        .find(|each| Some(&each.entry_id) == entry_id.read().as_ref())
        .or_else(|| {
            releases
                .iter()
                .find(|each| Some(&each.entry_id) == details.game.default_entry.as_ref())
        })
        .or_else(|| releases.first())
        .cloned();
    let actions = release
        .as_ref()
        .map(|release| release.actions.clone())
        .unwrap_or_default();
    // The saved action shown: the one chosen, else the first.
    let chosen = selected
        .read()
        .clone()
        .or_else(|| actions.first().map(|action| action.id.clone()));
    let shown = actions
        .iter()
        .find(|action| Some(&action.id) == chosen.as_ref())
        .cloned();
    let editing = draft.read().clone().or_else(|| shown.clone());
    // The saved version of what is being edited; none for a new action.
    let saved = match &*draft.read() {
        Some(draft) if Some(&draft.id) != shown.as_ref().map(|shown| &shown.id) => None,
        _ => shown.clone(),
    };
    let dirty = draft
        .read()
        .as_ref()
        .is_some_and(|draft| Some(draft) != saved.as_ref());
    use_effect(use_reactive!(|dirty| on_dirty_change.call(dirty)));

    let mut choose = move |id: Option<String>| {
        selected.set(id);
        draft.set(None);
        confirm_delete.set(false);
        failure.set(None);
    };
    // Runs a change; an id it answers is chosen next.
    let run = {
        move |work: crate::game::launch::Answer<Option<String>>| {
            pending.set(true);
            failure.set(None);
            spawn(async move {
                let result = work.await;
                pending.set(false);
                match result {
                    Ok(Some(id)) => {
                        selected.set(Some(id));
                        draft.set(None);
                        confirm_delete.set(false);
                    }
                    Ok(None) => {}
                    Err(report) => failure.set(Some(report.summary)),
                }
            });
        }
    };
    let added: Vec<GameAction> = actions
        .iter()
        .filter(|action| !is_built_in(action))
        .cloned()
        .collect();
    let play: Vec<GameAction> = actions
        .iter()
        .filter(|action| action.kind == ActionKind::Play)
        .cloned()
        .collect();
    let settings = release.as_ref().map(|release| release.settings.clone());
    let current = match &settings {
        Some(settings) if settings.ask => Some(ASK.to_owned()),
        _ => play
            .iter()
            .find(|action| {
                Some(&action.id)
                    == settings
                        .as_ref()
                        .and_then(|settings| settings.default_action.as_ref())
            })
            .or_else(|| play.first())
            .map(|action| action.id.clone()),
    };
    let built_in = editing
        .as_ref()
        .is_some_and(|editing| is_built_in(editing) && !editing.id.is_empty());
    let busy = pending();
    let proton = match (&release, &editing, &*proton.read()) {
        (Some(release), Some(editing), Some(Ok(builds)))
            if release.proton && runs_windows_program(editing) =>
        {
            Some(builds.clone())
        }
        _ => None,
    };

    match loaded {
        None => {
            return rsx! {
                p { role: "status", class: "text-sm text-fg-muted", {t!("Loading launch actions…")} }
            };
        }
        Some(Err(report)) => {
            return rsx! {
                p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
            };
        }
        Some(Ok(_)) => {}
    }
    let entries = details.entries.clone();
    rsx! {
        div { "data-cs": "game-actions", class: "flex flex-col gap-5",
            if releases.len() > 1 && let Some(release) = release.clone() {
                Select {
                    label: tp!("Picks which release's actions to edit", "Release"),
                    options: releases
                        .iter()
                        .map(|each| {
                            let entry = entries.iter().find(|entry| entry.id == each.entry_id);
                            SelectOption::new(
                                each.entry_id.clone(),
                                entry.map(release_label).unwrap_or_else(|| each.entry_id.clone()),
                            )
                        })
                        .collect::<Vec<_>>(),
                    value: Some(release.entry_id.clone()),
                    disabled: dirty,
                    on_change: move |next: String| {
                        entry_id.set(Some(next));
                        choose(None);
                    },
                }
            }
            if let (Some(release), Some(current), true) = (release.clone(), current.clone(), play.len() > 1) {
                Select {
                    "data-part": "default",
                    label: t!("Play starts"),
                    options: play
                        .iter()
                        .map(|action| SelectOption::new(action.id.clone(), action.name.clone()))
                        .chain([SelectOption::new(ASK, t!("Ask each time"))])
                        .collect::<Vec<_>>(),
                    value: Some(current),
                    disabled: busy,
                    on_change: {
                        let (data, game_id, mut run) = (data.clone(), game_id.clone(), run);
                        move |value: String| {
                            let mut settings = release.settings.clone();
                            if value == ASK {
                                settings.ask = true;
                            } else {
                                settings.ask = false;
                                settings.default_action = Some(value);
                            }
                            let work = (data.action_settings)(game_id.clone(), release.entry_id.clone(), settings);
                            run(Box::pin(async move { work.await.map(|_| None) }));
                        }
                    },
                }
            }
            div { class: "grid gap-5 md:grid-cols-[minmax(12rem,16rem)_1fr]",
                div { class: "flex flex-col gap-3",
                    if actions.is_empty() {
                        p { class: "text-sm text-fg-muted",
                            {t!("Nothing launches this game yet. Add a play action to start it from Caskade, or a tool to run alongside it.")}
                        }
                    } else {
                        ul {
                            "data-part": "actions",
                            role: "listbox",
                            "aria-label": t!("Launch actions"),
                            class: "flex flex-col gap-1",
                            for action in actions.iter().cloned() {
                                {
                                    let is_selected = editing.as_ref().is_some_and(|editing| editing.id == action.id);
                                    let id = action.id.clone();
                                    let is_default = !settings.as_ref().is_some_and(|settings| settings.ask)
                                        && play.len() > 1
                                        && current.as_deref() == Some(action.id.as_str());
                                    rsx! {
                                        li { key: "{action.id}", role: "option",
                                            "aria-selected": if is_selected { "true" } else { "false" },
                                            Button {
                                                "data-part": "action",
                                                "data-kind": if action.kind == ActionKind::Play { "play" } else { "utility" },
                                                "data-selected": flag(is_selected),
                                                "data-spatial": "focusable",
                                                variant: ButtonVariant::Ghost,
                                                disabled: dirty && !is_selected,
                                                class: if is_selected { "w-full justify-start border border-accent-alt bg-accent/10" } else { "w-full justify-start border border-border" },
                                                on_press: move |_| {
                                                    if !dirty {
                                                        choose(Some(id.clone()));
                                                    }
                                                },
                                                Svg {
                                                    icon: if action.kind == ActionKind::Play { GAMEPAD_2 } else { WRENCH },
                                                    class: "size-4 shrink-0",
                                                }
                                                span { class: "min-w-0 flex-1 truncate text-left", "{action.name}" }
                                                if is_default {
                                                    span { class: "font-mono text-[10px] tracking-widest text-accent-alt uppercase",
                                                        {tp!("launch action", "Default")}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "flex flex-wrap gap-2",
                        Button {
                            "data-spatial": "focusable",
                            icon: PLUS,
                            disabled: dirty,
                            on_press: {
                                let entry = release.as_ref().map(|release| release.entry_id.clone()).unwrap_or_default();
                                move |_| {
                                    choose(None);
                                    draft.set(Some(blank(&entry, ActionKind::Play, t!("Play (modded)"))));
                                }
                            },
                            {t!("Add play action")}
                        }
                        Button {
                            "data-spatial": "focusable",
                            icon: PLUS,
                            disabled: dirty,
                            on_press: {
                                let entry = release.as_ref().map(|release| release.entry_id.clone()).unwrap_or_default();
                                move |_| {
                                    choose(None);
                                    draft.set(Some(blank(&entry, ActionKind::Utility, t!("New tool"))));
                                }
                            },
                            {t!("Add tool")}
                        }
                    }
                }
                if let Some(editing) = editing.clone() {
                    div { class: "flex flex-col gap-4",
                        ActionForm {
                            action: editing.clone(),
                            on_change: move |next: GameAction| draft.set(Some(next)),
                            target_locked: built_in && !release.as_ref().is_some_and(|release| release.editable_built_in),
                            proton,
                            disabled: busy,
                        }
                        if let Some(failure) = failure() {
                            p { role: "alert", "data-part": "error", class: "text-sm text-danger", "{failure}" }
                        }
                        div { class: "flex flex-wrap items-center gap-2",
                            Button {
                                "data-spatial": "focusable",
                                variant: ButtonVariant::Primary,
                                disabled: !dirty || busy,
                                on_press: {
                                    let (data, game_id, mut run, editing) = (data.clone(), game_id.clone(), run, editing.clone());
                                    move |_| {
                                        let work = (data.save_action)(game_id.clone(), editing.clone());
                                        run(Box::pin(async move { work.await.map(|action| Some(action.id)) }));
                                    }
                                },
                                if busy { {t!("Saving…")} } else { {t!("Save action")} }
                            }
                            Button {
                                "data-spatial": "focusable",
                                disabled: !dirty || busy,
                                on_press: move |_| {
                                    draft.set(None);
                                    failure.set(None);
                                },
                                {t!("Revert")}
                            }
                            if let Some(saved) = saved.clone() {
                                Button {
                                    "data-spatial": "focusable",
                                    icon: COPY,
                                    disabled: dirty,
                                    on_press: {
                                        let saved = saved.clone();
                                        move |_| {
                                            selected.set(None);
                                            failure.set(None);
                                            let mut copy = saved.clone();
                                            copy.id = String::new();
                                            copy.name = t!("{name} (copy)", name = saved.name.clone());
                                            draft.set(Some(copy));
                                        }
                                    },
                                    {t!("Duplicate")}
                                }
                                if !is_built_in(&saved) {
                                    {
                                        let at = added.iter().position(|action| action.id == saved.id);
                                        let ids: Vec<String> = added.iter().map(|action| action.id.clone()).collect();
                                        let last = added.len().saturating_sub(1);
                                        let mover = |by: isize| {
                                            let (data, game_id, mut run, ids) = (data.clone(), game_id.clone(), run, ids.clone());
                                            let entry = release.as_ref().map(|release| release.entry_id.clone()).unwrap_or_default();
                                            move |_| {
                                                let Some(at) = at else { return };
                                                let to = at as isize + by;
                                                if to < 0 || to as usize >= ids.len() {
                                                    return;
                                                }
                                                let mut ids = ids.clone();
                                                let id = ids.remove(at);
                                                ids.insert(to as usize, id);
                                                let work = (data.order_actions)(game_id.clone(), entry.clone(), ids);
                                                run(Box::pin(async move { work.await.map(|_| None) }));
                                            }
                                        };
                                        let remove = {
                                            let (data, game_id, mut run, id) = (data.clone(), game_id.clone(), run, saved.id.clone());
                                            move |_| {
                                                if !confirm_delete() {
                                                    confirm_delete.set(true);
                                                    return;
                                                }
                                                let work = (data.delete_action)(game_id.clone(), id.clone());
                                                run(Box::pin(async move {
                                                    work.await.map(|_| {
                                                        selected.set(None);
                                                        None
                                                    })
                                                }));
                                            }
                                        };
                                        rsx! {
                                            Button {
                                                "data-spatial": "focusable",
                                                "aria-label": t!("Move up"),
                                                icon: ARROW_UP,
                                                disabled: dirty || busy || at == Some(0),
                                                on_press: mover(-1),
                                            }
                                            Button {
                                                "data-spatial": "focusable",
                                                "aria-label": t!("Move down"),
                                                icon: ARROW_DOWN,
                                                disabled: dirty || busy || at == Some(last),
                                                on_press: mover(1),
                                            }
                                            Button {
                                                "data-spatial": "focusable",
                                                "data-part": "delete",
                                                variant: ButtonVariant::Danger,
                                                icon: TRASH,
                                                disabled: busy,
                                                on_press: remove,
                                                if confirm_delete() {
                                                    {t!("Delete “{name}”?", name = saved.name.clone())}
                                                } else {
                                                    {t!("Delete…")}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_programs_are_their_releases_launch_or_a_program_file() {
        let mut action = blank("e", ActionKind::Play, "x".into());
        assert!(!runs_windows_program(&action));
        action.target = LaunchSpec::Executable {
            path: "Game.EXE".into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
        };
        assert!(runs_windows_program(&action));
        action.id = built_in_action_id("e");
        action.target = LaunchSpec::Uri { uri: "x".into() };
        assert!(runs_windows_program(&action));
    }
}
