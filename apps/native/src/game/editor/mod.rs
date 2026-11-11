//! A game's editor (the webview's `gameEditor/GameEditor.tsx`): its details
//! in one draft saved as a patch, and tabs that save on their own (artwork,
//! launch actions, scripts, save locations). Closing with unsaved work
//! anywhere asks first. Opened by the game page's Edit details and the game
//! menu's Edit….

mod action_form;
mod actions;
mod artwork;
pub mod data;
mod details;
mod draft;
mod saves;
#[cfg(test)]
pub(crate) mod tests;

use blitz_dom::NodeId;
use caskade_core::model::{FieldError, Game, GameEditOutcome, GameEditState, GameEntry};
use caskade_ui_data::Key;
use caskade_ui_data::scripts::ScriptScope;
use caskade_ui_i18n::t;
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use self::data::{EditData, use_edit_data};
use self::draft::{Draft, prune, take, to_edit};
use crate::data::use_game;
use crate::game::data::use_answer;
use crate::input::{Bind, use_input_action};
use crate::ui::icons::Icon;
use crate::ui::interact::flag;
use crate::ui::{Button, ButtonVariant, Dialog, Tab, Tabs};

/// Lucide's `user-pen`: a value the user set.
const USER_PEN: Icon = Icon(&[
    "M11.5 15H7a4 4 0 0 0-4 4v2",
    "M21.378 16.626a1 1 0 0 0-3.004-3.004l-4.01 4.012a2 2 0 0 0-.506.854l-.837 2.87a.5.5 0 0 0 .62.62l2.87-.837a2 2 0 0 0 .854-.506z",
    "M6 7a4 4 0 1 0 8 0a4 4 0 1 0-8 0",
]);

/// Lucide's `image-plus`: Import image….
const IMAGE_PLUS: Icon = Icon(&[
    "M16 5h6",
    "M19 2v6",
    "M21 11.5V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7.5",
    "m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21",
    "M7 9a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
]);

/// Lucide's `file-plus`: Add file.
const FILE_PLUS: Icon = Icon(&[
    "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z",
    "M14 2v4a2 2 0 0 0 2 2h4",
    "M9 15h6",
    "M12 18v-6",
]);

/// A release in a picker: its folder's name and its source, else its
/// source.
fn release_label(entry: &GameEntry) -> String {
    use caskade_core::model::InstallLocation;
    let source = super::present::source(entry.source_id.as_str());
    let folder = entry.location.as_ref().and_then(|location| {
        let path = match location {
            InstallLocation::External { path } => path,
            InstallLocation::Managed { rel_dir, .. } => rel_dir,
        };
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    });
    match folder {
        Some(folder) => format!("{folder} · {source}"),
        None => source,
    }
}

const TABS: [&str; 5] = ["details", "artwork", "actions", "scripts", "saves"];

/// `game_id`'s editor, titled by `title`; nothing where the app has no
/// editor (fixtures mode).
#[component]
pub fn GameEditor(game_id: String, title: String, on_close: EventHandler<()>) -> Element {
    match use_edit_data() {
        Some(data) => rsx! {
            Editor { game_id, title, data, on_close }
        },
        None => rsx! {},
    }
}

/// Whether games can be edited here (Edit details, the menu's Edit…).
pub fn use_edits() -> bool {
    use_edit_data().is_some()
}

#[component]
fn Editor(game_id: String, title: String, data: EditData, on_close: EventHandler<()>) -> Element {
    let loaded = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(Key::Game(game_id.clone()), move || {
            (data.state)(game_id.clone())
        })
    };
    let details = use_game(game_id.clone());
    let mut tab = use_signal(|| TABS[0].to_owned());
    let mut draft = use_signal(Draft::default);
    let mut errors = use_signal(Vec::<FieldError>::new);
    // The game as the editor opened it: a field saved differently since is
    // a conflict, not overwritten.
    let mut base = use_signal(|| None::<Game>);
    // The state a save answered, until the next load replaces the one it
    // was saved over.
    let mut saved = use_signal(|| None::<(GameEditState, Option<GameEditState>)>);
    let mut actions_dirty = use_signal(|| false);
    let mut scripts_dirty = use_signal(|| false);
    let mut saves_dirty = use_signal(|| false);
    let mut confirming = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut failure = use_signal(|| None::<String>);
    let mut body = use_signal(|| None::<NodeId>);

    let fresh: Option<Option<GameEditState>> = match &*loaded.read() {
        Some(Ok(state)) => Some(state.clone()),
        _ => None,
    };
    let load_error = match &*loaded.read() {
        Some(Err(report)) => Some(report.summary.clone()),
        _ => None,
    };
    let state: Option<GameEditState> = match &*saved.read() {
        Some((state, over)) if fresh.as_ref() == Some(over) => Some(state.clone()),
        _ => fresh.clone().flatten(),
    };
    use_effect(move || {
        if base.peek().is_none()
            && let Some(Ok(Some(state))) = &*loaded.read()
        {
            base.set(Some(state.game.clone()));
        }
    });
    let details_dirty = draft.read().is_dirty();
    let other_dirty = actions_dirty() || scripts_dirty() || saves_dirty();
    let mut request_close = move || {
        if draft.peek().is_dirty()
            || *actions_dirty.peek()
            || *scripts_dirty.peek()
            || *saves_dirty.peek()
        {
            confirming.set(true);
        } else {
            on_close.call(());
        }
    };
    let submit = {
        let (data, game_id, state, fresh) =
            (data.clone(), game_id.clone(), state.clone(), fresh.clone());
        move || {
            if state.is_none() || !draft.peek().is_dirty() || *saving.peek() {
                return;
            }
            let current = draft.peek().clone();
            let work = (data.save)(game_id.clone(), to_edit(&current), base.peek().clone());
            let over = fresh.clone().flatten();
            saving.set(true);
            failure.set(None);
            spawn(async move {
                let result = work.await;
                saving.set(false);
                match result {
                    // The game as it is now: a conflict shows the value
                    // saved meanwhile, which Keep mine saves over.
                    Ok(GameEditOutcome::Rejected {
                        state,
                        errors: refused,
                    }) => {
                        errors.set(refused);
                        saved.set(Some((state, over)));
                    }
                    Ok(GameEditOutcome::Saved { state, .. }) => {
                        errors.set(Vec::new());
                        base.set(Some(state.game.clone()));
                        draft.set(prune(&state, &current));
                        saved.set(Some((state, over)));
                        if !(*actions_dirty.peek() || *scripts_dirty.peek() || *saves_dirty.peek())
                        {
                            on_close.call(());
                        }
                    }
                    Err(report) => failure.set(Some(report.summary)),
                }
            });
        }
    };
    let on_details = tab() == "details";
    use_input_action(
        Action::Menu,
        Bind {
            label: (on_details && details_dirty).then(|| t!("Save")),
            owner: body(),
            ..Bind::default()
        },
        {
            let mut submit = submit.clone();
            move || {
                if *tab.peek() == "details" && draft.peek().is_dirty() {
                    submit();
                    true
                } else {
                    false
                }
            }
        },
    );
    let rebase = {
        let state = state.clone();
        move |field| {
            if let Some(state) = &state {
                let mut next = base.peek().clone().unwrap_or_else(|| state.game.clone());
                take(&mut next, &state.game, field);
                base.set(Some(next));
            }
            errors.write().retain(|error| error.field != field);
        }
    };
    let dirty_of = |id: &str| match id {
        "details" => details_dirty,
        "actions" => actions_dirty(),
        "scripts" => scripts_dirty(),
        "saves" => saves_dirty(),
        _ => false,
    };
    let label_of = |id: &str| match id {
        "details" => t!("Details"),
        "artwork" => t!("Artwork"),
        "actions" => t!("Actions"),
        "scripts" => t!("Scripts"),
        _ => t!("Saves"),
    };
    let tabs: Vec<Tab> = TABS
        .iter()
        .map(|id| Tab::new(*id, label_of(id)).dirty(dirty_of(id)))
        .collect();
    let game: Option<caskade_core::model::GameDetails> = details
        .read()
        .as_ref()
        .and_then(|game| game.as_ref().ok().cloned().flatten());
    let status = if let Some(failure) = failure().or(load_error) {
        Some(rsx! {
            span { class: "text-danger", "{failure}" }
        })
    } else if !errors.read().is_empty() {
        Some(rsx! {
            span { class: "text-danger", {t!("Nothing was saved. Check the marked fields.")} }
        })
    } else if saving() {
        Some(rsx! {
            span { class: "text-fg-muted", {t!("Saving…")} }
        })
    } else {
        None
    };
    let panel = |id: &str| if tab() == id { "" } else { "hidden" };
    rsx! {
        Dialog {
            title: t!("Edit {title}", title = title.clone()),
            on_close: move |_| request_close(),
            "data-cs": "game-editor",
            class: "max-w-3xl! flex h-[88vh] flex-col",
            div {
                class: "flex min-h-0 flex-1 flex-col",
                onmounted: move |event| {
                    if let Some(handle) = event.data().downcast::<NodeHandle>() {
                        body.set(Some(handle.node_id()));
                    }
                },
                Tabs {
                    tabs,
                    selected: tab(),
                    label: t!("Editor sections"),
                    class: "min-h-0 flex-1",
                    on_change: move |id: String| tab.set(id),
                    div { class: "min-h-0 flex-1 overflow-y-auto py-2",
                        // Every tab stays mounted, so switching keeps unsaved
                        // work.
                        div { class: panel("details"),
                            match (&state, &fresh) {
                                (Some(state), _) => rsx! {
                                    details::DetailsTab {
                                        state: state.clone(),
                                        draft: draft(),
                                        errors: errors(),
                                        on_draft: move |next: Draft| {
                                            // A field changed again is checked again on the
                                            // next save.
                                            errors.write().retain(|error| next.has(error.field));
                                            draft.set(next);
                                        },
                                        on_take_theirs: {
                                            let mut rebase = rebase.clone();
                                            move |field| {
                                                rebase(field);
                                                let next = draft.peek().without(field);
                                                draft.set(next);
                                            }
                                        },
                                        on_keep_mine: {
                                            let mut rebase = rebase.clone();
                                            move |field| rebase(field)
                                        },
                                    }
                                },
                                (None, None) => rsx! {
                                    p { class: "text-sm text-fg-muted", {t!("Loading…")} }
                                },
                                (None, Some(_)) => rsx! {
                                    p { class: "text-sm text-fg-muted", {t!("This game is no longer in the library.")} }
                                },
                            }
                        }
                        div { class: panel("artwork"),
                            artwork::ArtworkTab { game_id: game_id.clone(), data: data.clone() }
                        }
                        div { class: panel("actions"),
                            if let Some(game) = game.clone() {
                                actions::ActionsTab {
                                    game_id: game_id.clone(),
                                    details: game,
                                    data: data.clone(),
                                    on_dirty_change: move |dirty| actions_dirty.set(dirty),
                                }
                            }
                        }
                        div { class: panel("scripts"),
                            crate::flows::scripts::ScriptsPanel {
                                scope: ScriptScope::Game { game_id: game_id.clone() },
                                game_id: Some(game_id.clone()),
                                on_dirty_change: Some(EventHandler::new(move |dirty| scripts_dirty.set(dirty))),
                            }
                        }
                        div { class: panel("saves"),
                            if let Some(game) = game.clone() {
                                saves::SavesTab {
                                    details: game,
                                    data: data.clone(),
                                    on_dirty_change: move |dirty| saves_dirty.set(dirty),
                                }
                            }
                        }
                    }
                }
                div {
                    "data-part": "footer",
                    class: "flex flex-wrap items-center gap-3 border-t border-border pt-4",
                    p { role: "status", "data-part": "status", class: "mr-auto text-sm",
                        {status}
                    }
                    Button {
                        "data-part": "cancel",
                        "data-spatial": "focusable",
                        on_press: move |_| request_close(),
                        if details_dirty || other_dirty { {t!("Cancel")} } else { {t!("Close")} }
                    }
                    if on_details {
                        Button {
                            "data-part": "save",
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Primary,
                            disabled: !details_dirty || saving(),
                            on_press: {
                                let mut submit = submit.clone();
                                move |_| submit()
                            },
                            if failure().is_some() { {t!("Try again")} } else { {t!("Save")} }
                        }
                    }
                }
            }
        }
        if confirming() {
            Dialog {
                title: t!("Discard your changes?"),
                alert: true,
                on_close: move |_| confirming.set(false),
                "data-cs": "game-editor-discard",
                p { class: "text-sm text-fg-muted",
                    {t!("Your unsaved changes to {title} will be lost.", title = title.clone())}
                }
                div { class: "flex justify-end gap-2",
                    Button {
                        "data-spatial": "focusable",
                        "data-focus-default": "true",
                        on_press: move |_| confirming.set(false),
                        {t!("Keep editing")}
                    }
                    Button {
                        "data-spatial": "focusable",
                        variant: ButtonVariant::Danger,
                        on_press: move |_| on_close.call(()),
                        {t!("Discard")}
                    }
                }
            }
        }
    }
}
