//! One edit on the chosen games (#99, the webview's
//! `components/gameSelection/BulkEditDialog.tsx`): each field changes only
//! when its box is ticked, so values that differ between the games stay as
//! they are; lists add, remove or replace values. A preview names each game
//! and what changes before anything is saved; the result says exactly which
//! games saved, which already had it and which failed. Cancelling before
//! Apply saves nothing, and applying again changes only what is left.

use crate::data::{change, use_library};
use crate::input::{Bind, use_input_action};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Checkbox, Dialog, Segment, SegmentedControl, Slider,
    TextField, icons,
};
use blitz_dom::NodeId;
use caskade_core::model::{
    BulkGameResult, BulkStatus, DateEdit, GameEdit, GameField, ListEdit, NotesEdit, RatingEdit,
    TextEdit,
};
use caskade_ui_i18n::{t, tn};
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use std::collections::HashSet;

/// Games a preview lists one by one; the count covers the rest.
const SHOWN: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ListMode {
    #[default]
    Add,
    Remove,
    Replace,
    Reset,
}

impl ListMode {
    fn id(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Remove => "remove",
            Self::Replace => "replace",
            Self::Reset => "reset",
        }
    }

    fn of(id: &str) -> Self {
        match id {
            "remove" => Self::Remove,
            "replace" => Self::Replace,
            "reset" => Self::Reset,
            _ => Self::Add,
        }
    }
}

#[derive(Clone, PartialEq, Default)]
struct ListState {
    mode: ListMode,
    values: Vec<String>,
}

/// What the form holds; only ticked fields reach the patch.
#[derive(Clone, PartialEq, Default)]
pub(crate) struct Form {
    enabled: HashSet<GameField>,
    tags: ListState,
    genres: ListState,
    platforms: ListState,
    aliases: ListState,
    /// Reset to automatic, else replace with `description`.
    description_reset: bool,
    description: String,
    release_reset: bool,
    release: String,
    rating: Option<i32>,
    notes: String,
}

impl Form {
    fn list(&self, field: GameField) -> &ListState {
        match field {
            GameField::Tags => &self.tags,
            GameField::Genres => &self.genres,
            GameField::Platforms => &self.platforms,
            _ => &self.aliases,
        }
    }

    fn list_mut(&mut self, field: GameField) -> &mut ListState {
        match field {
            GameField::Tags => &mut self.tags,
            GameField::Genres => &mut self.genres,
            GameField::Platforms => &mut self.platforms,
            _ => &mut self.aliases,
        }
    }

    /// The patch the ticked fields make.
    pub(crate) fn patch(&self) -> GameEdit {
        let on = |field| self.enabled.contains(&field);
        let list = |field| {
            on(field).then(|| {
                let ListState { mode, values } = self.list(field).clone();
                match mode {
                    ListMode::Add => ListEdit::Add { values },
                    ListMode::Remove => ListEdit::Remove { values },
                    ListMode::Replace => ListEdit::Set { values },
                    ListMode::Reset => ListEdit::Reset,
                }
            })
        };
        GameEdit {
            tags: list(GameField::Tags),
            genres: list(GameField::Genres),
            platforms: list(GameField::Platforms),
            aliases: list(GameField::Aliases),
            description: on(GameField::Description).then(|| {
                if self.description_reset {
                    TextEdit::Reset
                } else {
                    TextEdit::Set {
                        value: self.description.clone(),
                    }
                }
            }),
            release_date: on(GameField::ReleaseDate).then(|| {
                if self.release_reset {
                    DateEdit::Reset
                } else {
                    let value = self.release.trim();
                    DateEdit::Set {
                        value: (!value.is_empty()).then(|| value.to_owned()),
                    }
                }
            }),
            rating: on(GameField::Rating).then_some(RatingEdit { value: self.rating }),
            notes: on(GameField::Notes).then(|| NotesEdit {
                value: self.notes.clone(),
            }),
            ..GameEdit::default()
        }
    }
}

/// A field's name, as the editor shows it.
pub(crate) fn field_label(field: GameField) -> String {
    match field {
        GameField::Title => t!("Title"),
        GameField::SortTitle => t!("Sort title"),
        GameField::Description => t!("Description"),
        GameField::Aliases => t!("Other names"),
        GameField::Genres => t!("Genres"),
        GameField::Tags => t!("Tags"),
        GameField::ReleaseDate => t!("Release date"),
        GameField::Platforms => t!("Platforms"),
        GameField::Rating => t!("Your rating"),
        GameField::Notes => t!("Notes"),
    }
}

#[derive(Clone, PartialEq)]
enum Step {
    Edit,
    Preview(Vec<BulkGameResult>),
    Done(Vec<BulkGameResult>),
}

impl Step {
    fn name(&self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Preview(_) => "preview",
            Self::Done(_) => "done",
        }
    }
}

#[component]
pub fn BulkEditDialog(game_ids: Vec<String>, on_close: EventHandler<()>) -> Element {
    let library = use_library();
    let mut form = use_signal(Form::default);
    let mut step = use_signal(|| Step::Edit);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut body = use_signal(|| None::<NodeId>);
    let edit = form.read().patch();
    let ready = !edit.fields().is_empty();
    let count = game_ids.len();

    let run = use_callback(move |apply: bool| {
        let library = library.clone();
        let ids = game_ids.clone();
        let edit = form.peek().patch();
        pending.set(true);
        error.set(None);
        spawn(async move {
            let result = change(async move { library.bulk_edit(ids, edit, apply).await }).await;
            match result {
                Ok(results) => step.set(if apply {
                    Step::Done(results)
                } else {
                    Step::Preview(results)
                }),
                Err(report) => error.set(Some(report.summary)),
            }
            pending.set(false);
        });
    });
    let primary = move || {
        if pending() {
            return;
        }
        match &*step.peek() {
            Step::Edit if form.peek().patch().fields().is_empty() => {}
            Step::Edit => run(false),
            Step::Preview(_) => run(true),
            Step::Done(_) => on_close.call(()),
        }
    };
    let hint = match &*step.read() {
        Step::Edit => t!("Preview"),
        Step::Preview(_) => t!("Apply"),
        Step::Done(_) => t!("Done"),
    };
    use_input_action(
        Action::Menu,
        Bind {
            label: Some(hint),
            owner: body(),
            ..Bind::default()
        },
        move || {
            primary();
            true
        },
    );
    let close = move |_| {
        if !pending() {
            on_close.call(());
        }
    };

    let toggle = move |field: GameField| {
        move |on: bool| {
            let mut form = form.write();
            if on {
                form.enabled.insert(field);
            } else {
                form.enabled.remove(&field);
            }
        }
    };
    let enabled = |field: GameField| form.read().enabled.contains(&field);
    let list_field = move |field: GameField| {
        let label = field_label(field);
        let state = form.read().list(field).clone();
        rsx! {
            Field {
                key: "{field.key()}",
                field,
                label: label.clone(),
                on: enabled(field),
                on_toggle: toggle(field),
                SegmentedControl {
                    "aria-label": t!("How to change {0}", 0 = label),
                    value: state.mode.id(),
                    on_change: move |id: String| form.write().list_mut(field).mode = ListMode::of(&id),
                    options: vec![
                        Segment::new("add", t!("Add")),
                        Segment::new("remove", t!("Remove")),
                        Segment::new("replace", t!("Replace")),
                        Segment::new("reset", t!("Reset to automatic")),
                    ],
                }
                if state.mode != ListMode::Reset {
                    ListInput {
                        label: label.clone(),
                        values: state.values.clone(),
                        split: field != GameField::Aliases,
                        on_change: move |values: Vec<String>| form.write().list_mut(field).values = values,
                    }
                }
            }
        }
    };

    let current = step();
    let status = if let Some(error) = error() {
        Some(rsx! {
            span { class: "text-danger",
                {t!("That didn't finish: {0}. Trying again is safe: what saved stays as it is.", 0 = error)}
            }
        })
    } else if pending() {
        Some(rsx! {
            span { class: "text-fg-muted",
                if current == Step::Edit { {t!("Working out the changes…")} } else { {t!("Saving…")} }
            }
        })
    } else {
        None
    };
    let description = form.read().description.clone();
    let release = form.read().release.clone();
    let rating = form.read().rating;
    let notes = form.read().notes.clone();
    let description_reset = form.read().description_reset;
    let release_reset = form.read().release_reset;

    rsx! {
        Dialog {
            title: tn!("Edit # game", "Edit # games", count),
            on_close: close,
            "data-cs": "bulk-editor",
            "data-step": current.name(),
            class: "max-w-3xl! flex h-[88vh] flex-col",
            div {
                class: "flex min-h-0 flex-1 flex-col",
                onmounted: move |event| {
                    if let Some(handle) = event.data().downcast::<NodeHandle>() {
                        body.set(Some(handle.node_id()));
                    }
                },
                div { class: "min-h-0 flex-1 overflow-y-auto py-2",
                    match &current {
                        Step::Edit => rsx! {
                            div { "data-part": "fields", class: "flex flex-col gap-4",
                                p { class: "text-sm text-fg-muted",
                                    {t!("Tick what to change. Everything else stays as each game has it.")}
                                }
                                {list_field(GameField::Tags)}
                                {list_field(GameField::Genres)}
                                {list_field(GameField::Platforms)}
                                {list_field(GameField::Aliases)}
                                Field {
                                    field: GameField::Description,
                                    label: field_label(GameField::Description),
                                    on: enabled(GameField::Description),
                                    on_toggle: toggle(GameField::Description),
                                    SegmentedControl {
                                        "aria-label": t!("How to change {0}", 0 = field_label(GameField::Description)),
                                        value: if description_reset { "reset" } else { "replace" },
                                        on_change: move |id: String| form.write().description_reset = id == "reset",
                                        options: vec![
                                            Segment::new("replace", t!("Replace")),
                                            Segment::new("reset", t!("Reset to automatic")),
                                        ],
                                    }
                                    if !description_reset {
                                        TextAreaField {
                                            label: field_label(GameField::Description),
                                            value: description,
                                            on_change: move |value: String| form.write().description = value,
                                        }
                                    }
                                }
                                Field {
                                    field: GameField::ReleaseDate,
                                    label: field_label(GameField::ReleaseDate),
                                    on: enabled(GameField::ReleaseDate),
                                    on_toggle: toggle(GameField::ReleaseDate),
                                    SegmentedControl {
                                        "aria-label": t!("How to change {0}", 0 = field_label(GameField::ReleaseDate)),
                                        value: if release_reset { "reset" } else { "replace" },
                                        on_change: move |id: String| form.write().release_reset = id == "reset",
                                        options: vec![
                                            Segment::new("replace", t!("Set")),
                                            Segment::new("reset", t!("Reset to automatic")),
                                        ],
                                    }
                                    if !release_reset {
                                        TextField {
                                            "aria-label": field_label(GameField::ReleaseDate),
                                            placeholder: t!("YYYY-MM-DD, or empty for none"),
                                            mono: true,
                                            value: release,
                                            on_input: move |value: String| form.write().release = value,
                                        }
                                    }
                                }
                                Field {
                                    field: GameField::Rating,
                                    label: field_label(GameField::Rating),
                                    on: enabled(GameField::Rating),
                                    on_toggle: toggle(GameField::Rating),
                                    div { class: "flex flex-wrap items-center gap-4",
                                        Checkbox {
                                            checked: rating.is_some(),
                                            label: t!("Rated"),
                                            on_change: move |rated: bool| form.write().rating = rated.then_some(50),
                                        }
                                        Slider {
                                            label: field_label(GameField::Rating),
                                            class: "flex-1",
                                            min: 0.0,
                                            max: 100.0,
                                            disabled: rating.is_none(),
                                            value: f64::from(rating.unwrap_or(0)),
                                            on_change: move |value: f64| form.write().rating = Some(value.round() as i32),
                                            format: move |value: f64| t!("{value} / 100", value = value.round()),
                                        }
                                    }
                                }
                                Field {
                                    field: GameField::Notes,
                                    label: field_label(GameField::Notes),
                                    on: enabled(GameField::Notes),
                                    on_toggle: toggle(GameField::Notes),
                                    TextAreaField {
                                        label: field_label(GameField::Notes),
                                        value: notes,
                                        on_change: move |value: String| form.write().notes = value,
                                    }
                                }
                            }
                        },
                        Step::Preview(results) => rsx! { Results { results: results.clone(), applied: false } },
                        Step::Done(results) => rsx! { Results { results: results.clone(), applied: true } },
                    }
                }
                div {
                    "data-part": "footer",
                    class: "flex flex-wrap items-center gap-3 border-t border-border pt-4",
                    p { role: "status", "data-part": "status", class: "mr-auto text-sm", {status} }
                    if !matches!(current, Step::Done(_)) {
                        Button {
                            "data-part": "cancel",
                            "data-spatial": "focusable",
                            disabled: pending(),
                            on_press: move |_| {
                                if matches!(*step.peek(), Step::Preview(_)) {
                                    error.set(None);
                                    step.set(Step::Edit);
                                } else {
                                    on_close.call(());
                                }
                            },
                            if matches!(current, Step::Preview(_)) { {t!("Back")} } else { {t!("Cancel")} }
                        }
                    }
                    Button {
                        key: "{current.name()}",
                        "data-part": "apply",
                        "data-spatial": "focusable",
                        // Each step's content replaces the last: focus
                        // moves to its main button.
                        "data-focus-default": (current != Step::Edit).then_some("true"),
                        variant: ButtonVariant::Primary,
                        disabled: pending() || (current == Step::Edit && !ready),
                        on_press: move |_| primary(),
                        match &current {
                            Step::Edit => rsx! { {t!("Preview")} },
                            Step::Preview(_) if error().is_some() => rsx! { {t!("Try again")} },
                            Step::Preview(_) => rsx! { {t!("Apply")} },
                            Step::Done(_) => rsx! { {t!("Done")} },
                        }
                    }
                }
            }
        }
    }
}

/// A field the edit changes only when its box is ticked.
#[component]
fn Field(
    field: GameField,
    label: String,
    on: bool,
    on_toggle: EventHandler<bool>,
    children: Element,
) -> Element {
    rsx! {
        div {
            "data-part": "field",
            "data-field": field.key(),
            "data-enabled": on.then_some("true"),
            class: "flex flex-col gap-2",
            Checkbox {
                "data-spatial": "focusable",
                checked: on,
                label: t!("Change {0}", 0 = label),
                on_change: move |on: bool| on_toggle.call(on),
            }
            if on {
                div { class: "flex flex-col gap-2 pl-7", {children} }
            }
        }
    }
}

/// How many games each outcome covers, and the games themselves.
#[component]
fn Results(results: Vec<BulkGameResult>, applied: bool) -> Element {
    let by = |status: BulkStatus| -> Vec<&BulkGameResult> {
        results
            .iter()
            .filter(|result| result.status == status)
            .collect()
    };
    let changed = by(BulkStatus::Changed);
    let unchanged = by(BulkStatus::Unchanged).len();
    let rejected = by(BulkStatus::Rejected);
    let missing = by(BulkStatus::Missing).len();
    let more = changed.len().saturating_sub(SHOWN);
    rsx! {
        div { "data-part": "results", class: "flex flex-col gap-4",
            ul { "data-part": "summary", class: "flex flex-wrap gap-x-6 gap-y-1 text-sm",
                li {
                    if applied {
                        {tn!("# game saved", "# games saved", changed.len())}
                    } else {
                        {tn!("# game changes", "# games change", changed.len())}
                    }
                }
                li { class: "text-fg-muted", {tn!("# already has it", "# already have it", unchanged)} }
                if !rejected.is_empty() {
                    li { class: "text-danger", {tn!("# can't take it", "# can't take it", rejected.len())} }
                }
                if missing > 0 {
                    li { class: "text-fg-muted",
                        {tn!("# is no longer in the library", "# are no longer in the library", missing)}
                    }
                }
            }
            if !rejected.is_empty() {
                section { "aria-label": t!("Not saved"), class: "flex flex-col gap-1",
                    h3 { class: "text-sm font-semibold text-danger", {t!("Not saved")} }
                    ul { class: "flex flex-col gap-1 text-sm",
                        for result in rejected {
                            li { key: "{result.game_id}", "data-part": "rejected",
                                span { class: "font-medium", "{result.title}" }
                                ": "
                                {
                                    result
                                        .errors
                                        .iter()
                                        .map(|error| format!("{}: {}", field_label(error.field), error.message))
                                        .collect::<Vec<_>>()
                                        .join("; ")
                                }
                            }
                        }
                    }
                }
            }
            if !changed.is_empty() {
                section {
                    "aria-label": if applied { t!("Saved") } else { t!("What changes") },
                    class: "flex flex-col gap-1",
                    h3 { class: "text-sm font-semibold",
                        if applied { {t!("Saved")} } else { {t!("What changes")} }
                    }
                    ul { class: "flex flex-col divide-y divide-border text-sm",
                        for result in changed.iter().take(SHOWN) {
                            li { key: "{result.game_id}", "data-part": "change", class: "py-2",
                                p { class: "font-medium", "{result.title}" }
                                for change in result.changes.iter() {
                                    p { key: "{change.field.key()}", class: "text-xs text-fg-muted",
                                        "{field_label(change.field)}: "
                                        del { {if change.before.is_empty() { t!("none") } else { change.before.clone() }} }
                                        " → "
                                        ins { class: "no-underline text-fg",
                                            {if change.after.is_empty() { t!("none") } else { change.after.clone() }}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if more > 0 {
                        p { class: "text-xs text-fg-subtle", {t!("And {0} more.", 0 = more)} }
                    }
                }
            }
        }
    }
}

/// A list of values as chips, each with its own remove button, and a field
/// that adds more: Enter or Add, and with `split` a comma between several
/// (the webview's `gameEditor/fields.tsx`).
#[component]
pub fn ListInput(
    label: String,
    values: Vec<String>,
    on_change: EventHandler<Vec<String>>,
    #[props(default)] split: bool,
) -> Element {
    let mut text = use_signal(String::new);
    let add = {
        let values = values.clone();
        move || {
            let typed = text.peek().clone();
            let pieces: Vec<&str> = if split {
                typed.split(',').collect()
            } else {
                vec![typed.as_str()]
            };
            let mut next = values.clone();
            for piece in pieces {
                let piece = piece.trim();
                if !piece.is_empty() && !next.iter().any(|value| value == piece) {
                    next.push(piece.to_owned());
                }
            }
            if next.len() > values.len() {
                on_change.call(next);
            }
            text.set(String::new());
        }
    };
    let mut add_on_enter = add.clone();
    let mut add_on_press = add;
    let empty = text.read().trim().is_empty();
    rsx! {
        div { "data-part": "list", class: "flex flex-col gap-2",
            if values.is_empty() {
                p { class: "text-xs text-fg-subtle", {t!("None")} }
            } else {
                ul { "aria-label": "{label}", class: "flex flex-wrap gap-1.5",
                    for value in values.iter().cloned() {
                        li {
                            key: "{value}",
                            "data-part": "chip",
                            class: "inline-flex items-center gap-1 rounded-control border border-border-strong bg-surface-overlay/70 py-0.5 pr-0.5 pl-2 text-xs",
                            span { "{value}" }
                            Button {
                                "data-part": "remove",
                                "data-spatial": "focusable",
                                variant: ButtonVariant::Ghost,
                                size: ButtonSize::Sm,
                                icon: icons::X,
                                class: "h-6 px-1.5",
                                "aria-label": t!("Remove {value}", value = value.clone()),
                                on_press: {
                                    let values = values.clone();
                                    move |_| on_change.call(values.iter().filter(|other| **other != value).cloned().collect())
                                },
                            }
                        }
                    }
                }
            }
            TextField {
                "aria-label": t!("Add to {label}", label = label.clone()),
                placeholder: if split { t!("Add, separated by commas") } else { t!("Add") },
                value: text(),
                on_input: move |value: String| text.set(value),
                on_submit: move |_| add_on_enter(),
                action: rsx! {
                    Button {
                        "data-spatial": "focusable",
                        icon: icons::PLUS,
                        disabled: empty,
                        "aria-label": t!("Add to {label}", label = label.clone()),
                        on_press: move |_| add_on_press(),
                    }
                },
            }
        }
    }
}

/// A multi-line text field (a description, notes).
#[component]
pub fn TextAreaField(label: String, value: String, on_change: EventHandler<String>) -> Element {
    rsx! {
        div { "data-cs": "text-field", class: "group flex flex-col",
            div {
                "data-part": "control",
                class: "flex rounded-control border border-border-strong bg-surface-base/60 transition-[border-color,box-shadow] duration-(--cs-motion-duration-fast) focus-within:border-accent-alt/60 focus-within:shadow-[0_0_16px_-6px] focus-within:shadow-accent-alt",
                textarea {
                    "aria-label": "{label}",
                    "data-part": "input",
                    "data-spatial": "focusable",
                    rows: "4",
                    value: "{value}",
                    class: "block w-full resize-y bg-transparent px-3 py-2 text-sm text-fg outline-none placeholder:text-fg-subtle",
                    oninput: move |event| on_change.call(event.value()),
                }
            }
        }
    }
}
