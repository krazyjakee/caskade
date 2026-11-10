//! Adds a game by hand (#174), as the webview's `ManualGameDialog`: a game
//! already on disk or behind a URI, started where it is (nothing is copied,
//! installed or run), or a title-only record of a game Caskade can't start
//! (a cartridge, a disc on a shelf). A game that may already be in the
//! library offers to add the target to it, or to keep the new one apart.

use caskade_core::manual::{DuplicateGame, DuplicateReason, ManualGame, ManualTarget};
use caskade_core::model::LaunchSpec;
use caskade_ui_i18n::t;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use crate::data::{change, use_library};
use crate::ui::icons::FILE_SEARCH;
use crate::ui::{Button, ButtonSize, ButtonVariant, Dialog, TextField};

use super::paths::{PathRequest, PickKind, format_path, use_path_picker};

pub const ANATOMY: Entry = Entry {
    parts: &[
        (
            "target",
            "What it starts: the path or URI field, Browse and the drop area (`data-drop-active` while a file is dragged over).",
        ),
        (
            "summary",
            "The program, arguments, folder or URI it will start.",
        ),
        ("duplicates", "Games that may be the same one."),
        ("duplicate", "One of them, and why."),
        ("link", "Adds the target to that game instead (a `button`)."),
        ("error", "Why the target can't be added, or adding failed."),
        (
            "add",
            "Adds the game, or keeps it separate from the duplicates (a `button`).",
        ),
    ],
    states: &[("data-mode", &["target", "title"])],
    contains: &["button", "text-field"],
    ..Entry::new(
        "manual-game",
        "ManualGameDialog (apps/native/src/flows/manual_game.rs)",
        Source::App,
        "Adds a game by hand from Add game: one on this computer or behind a URI, started where it is, or a title-only record; games that may be the same offer to join or keep apart.",
    )
};

/// Which game the dialog adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualMode {
    /// A program, shortcut or URI on this computer.
    Target,
    /// A title-only record.
    Title,
}

impl ManualMode {
    fn name(self) -> &'static str {
        match self {
            Self::Target => "target",
            Self::Title => "title",
        }
    }
}

/// The dialog. `on_added` gets the game's id (the linked game's, when the
/// target joined one) after it closes.
#[component]
pub fn ManualGameDialog(
    mode: ManualMode,
    on_close: EventHandler<()>,
    on_added: EventHandler<String>,
) -> Element {
    let library = use_library();
    let picker = use_path_picker();
    let mut input = use_signal(String::new);
    let mut target = use_signal(|| None::<ManualTarget>);
    let mut title = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut checking = use_signal(|| false);
    let mut adding = use_signal(|| false);
    let mut duplicates = use_signal(|| None::<Vec<DuplicateGame>>);

    let resolve = {
        let library = library.clone();
        move |value: String| {
            if value.trim().is_empty() {
                return;
            }
            input.set(value.clone());
            error.set(None);
            checking.set(true);
            let library = library.clone();
            spawn(async move {
                let resolved = change(async move { library.manual_target(value).await }).await;
                match resolved {
                    Ok(resolved) => {
                        if title.peek().is_empty() {
                            title.set(resolved.title.clone());
                        }
                        target.set(Some(resolved));
                        duplicates.set(None);
                    }
                    Err(report) => {
                        target.set(None);
                        error.set(Some(report.summary));
                    }
                }
                checking.set(false);
            });
        }
    };
    let choose = {
        let resolve = resolve.clone();
        move |_| {
            let mut resolve = resolve.clone();
            spawn(async move {
                let request =
                    PathRequest::new(PickKind::File, t!("The game's program or shortcut"));
                let picked = picker.pick(request).await;
                if let Some(path) = picked {
                    resolve(path);
                }
            });
        }
    };
    let finish = {
        let library = library.clone();
        move |link_to: Option<String>, keep_apart: bool| {
            let game = ManualGame {
                title: title.peek().trim().to_owned(),
                target: match mode {
                    ManualMode::Target => target.peek().clone(),
                    ManualMode::Title => None,
                },
                link_to,
                keep_apart,
            };
            adding.set(true);
            error.set(None);
            let library = library.clone();
            spawn(async move {
                match change(async move { library.add_manual_game(game).await }).await {
                    Ok(id) => {
                        on_close.call(());
                        on_added.call(id);
                    }
                    Err(report) => {
                        error.set(Some(report.summary));
                        adding.set(false);
                    }
                }
            });
        }
    };
    let submit = {
        let library = library.clone();
        let mut finish = finish.clone();
        move || {
            let name = title.peek().trim().to_owned();
            if name.is_empty() || (mode == ManualMode::Target && target.peek().is_none()) {
                return;
            }
            if let Some(found) = duplicates.peek().as_ref() {
                return finish(None, !found.is_empty());
            }
            checking.set(true);
            let wanted = match mode {
                ManualMode::Target => target.peek().clone(),
                ManualMode::Title => None,
            };
            let library = library.clone();
            let mut finish = finish.clone();
            spawn(async move {
                let found =
                    change(async move { library.manual_duplicates(name, wanted).await }).await;
                checking.set(false);
                match found {
                    Ok(found) if found.is_empty() => finish(None, false),
                    Ok(found) => duplicates.set(Some(found)),
                    Err(report) => error.set(Some(report.summary)),
                }
            });
        }
    };

    let busy = checking() || adding();
    let ready = !title().trim().is_empty() && (mode == ManualMode::Title || target().is_some());
    let found = duplicates().filter(|found| !found.is_empty());
    let heading = match mode {
        ManualMode::Target => t!("Add a game on this computer"),
        ManualMode::Title => t!("Add a game by title"),
    };
    let add_label = if found.is_some() {
        t!("Keep separate")
    } else if busy {
        t!("Adding…")
    } else {
        t!("Add game")
    };
    let close = move |_| {
        if !busy {
            on_close.call(());
        }
    };
    let mut submit_title = submit.clone();
    let mut submit_add = submit.clone();
    let resolve_on_leave = resolve.clone();
    let resolve_on_enter = resolve.clone();
    rsx! {
        Dialog {
            title: heading,
            on_close: (!busy).then_some(EventHandler::new(close)),
            class: "max-w-xl",
            "data-cs": "manual-game",
            "data-mode": mode.name(),
            p { class: "text-sm text-fg-muted",
                if mode == ManualMode::Target {
                    {t!("Caskade starts it where it is: nothing is copied, moved or installed. To install a game instead, use Install from file or drop it on the library.")}
                } else {
                    {t!("A record for a game Caskade can't start, like a cartridge or a disc: it keeps your notes, artwork and collections. Add something to start later in Edit details.")}
                }
            }
            div { class: "flex flex-col gap-4",
                if mode == ManualMode::Target {
                    section {
                        "data-part": "target",
                        "aria-label": t!("What it starts"),
                        class: "flex flex-col gap-3 rounded-panel border border-dashed border-border-strong p-4",
                        // As the webview's field resolves on blur: when focus
                        // leaves the field (to Browse or the title).
                        onfocusout: move |_| {
                            let value = input.peek().clone();
                            if !value.trim().is_empty() && target.peek().is_none() && !*checking.peek() {
                                let mut resolve = resolve_on_leave.clone();
                                resolve(value);
                            }
                        },
                        TextField {
                            id: "manual-target-input",
                            label: t!("Program, shortcut or URI"),
                            description: t!("A path, a .desktop, .lnk or .url shortcut, or a URI like steam://rungameid/620. Or drop a file here."),
                            mono: true,
                            value: input(),
                            on_input: move |value: String| {
                                input.set(value);
                                target.set(None);
                                duplicates.set(None);
                            },
                            on_submit: move |value: String| {
                                let mut resolve = resolve_on_enter.clone();
                                resolve(value);
                            },
                            action: rsx! {
                                Button { id: "manual-browse", icon: FILE_SEARCH, disabled: busy, on_press: choose,
                                    {t!("Browse…")}
                                }
                            },
                        }
                        if let Some(target) = target() {
                            TargetSummary { target }
                        }
                    }
                }
                TextField {
                    id: "manual-title-input",
                    label: t!("Title"),
                    value: title(),
                    required: true,
                    on_input: move |value: String| {
                        title.set(value);
                        duplicates.set(None);
                    },
                    on_submit: move |_| submit_title(),
                }
                if let Some(found) = found.clone() {
                    section {
                        "data-part": "duplicates",
                        "aria-label": t!("Maybe already in your library"),
                        class: "flex flex-col gap-2 rounded-panel border border-warning/50 p-4",
                        h3 { class: "text-sm font-semibold", {t!("This may already be in your library")} }
                        ul { class: "flex flex-col gap-2",
                            for duplicate in found {
                                li {
                                    key: "{duplicate.game_id}",
                                    "data-part": "duplicate",
                                    class: "flex flex-wrap items-center justify-between gap-2",
                                    span { class: "text-sm",
                                        "{duplicate.title} "
                                        span { class: "text-xs text-fg-muted",
                                            match duplicate.reason {
                                                DuplicateReason::SameTarget => t!("starts the same program"),
                                                DuplicateReason::SameTitle => t!("has the same title"),
                                            }
                                        }
                                    }
                                    if mode == ManualMode::Target {
                                        Button {
                                            "data-part": "link",
                                            id: "manual-link-{duplicate.game_id}",
                                            size: ButtonSize::Sm,
                                            disabled: busy,
                                            on_press: {
                                                let mut finish = finish.clone();
                                                let id = duplicate.game_id.clone();
                                                move |_| finish(Some(id.clone()), false)
                                            },
                                            {t!("Add to this game")}
                                        }
                                    }
                                }
                            }
                        }
                        p { class: "text-xs text-fg-muted",
                            {t!("Or keep it as a game of its own: Caskade won't merge them later.")}
                        }
                    }
                }
                if let Some(failure) = error() {
                    p { role: "alert", "data-part": "error", class: "text-sm text-danger", "{failure}" }
                }
                div { class: "flex justify-end gap-2",
                    Button { id: "manual-cancel", disabled: busy, on_press: close, {t!("Cancel")} }
                    Button {
                        id: "manual-add",
                        "data-part": "add",
                        variant: ButtonVariant::Primary,
                        disabled: !ready || busy,
                        on_press: move |_| submit_add(),
                        "{add_label}"
                    }
                }
            }
        }
    }
}

/// What the target starts, as the user should check it.
#[component]
fn TargetSummary(target: ManualTarget) -> Element {
    rsx! {
        dl { "data-part": "summary", class: "grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs",
            match &target.launch {
                LaunchSpec::Executable { path, args, cwd, .. } => rsx! {
                    dt { class: "text-fg-muted", {t!("Program")} }
                    dd { class: "font-mono break-all", {format_path(&path.to_string_lossy())} }
                    if !args.is_empty() {
                        dt { class: "text-fg-muted", {t!("Arguments")} }
                        dd { class: "font-mono break-all", {args.join(" ")} }
                    }
                    if let Some(cwd) = cwd {
                        dt { class: "text-fg-muted", {t!("Starts in")} }
                        dd { class: "font-mono break-all", {cwd.to_string_lossy().into_owned()} }
                    }
                    if target.windows && !cfg!(windows) {
                        dt { class: "text-fg-muted", {t!("Runs with")} }
                        dd { {t!("a Windows compatibility tool")} }
                    }
                },
                LaunchSpec::Uri { uri } => rsx! {
                    dt { class: "text-fg-muted", {t!("Opens")} }
                    dd { class: "font-mono break-all", "{uri}" }
                },
                _ => rsx! {},
            }
        }
    }
}
