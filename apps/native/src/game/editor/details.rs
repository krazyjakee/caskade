//! The editor's Details tab (the webview's `gameEditor/DetailsTab.tsx`):
//! the game's descriptive fields and the user's own rating and notes, as a
//! draft the editor saves in one patch.

use caskade_core::model::{FieldError, FieldProblem, GameEditState, GameField};
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::draft::{Change, Draft, Value, change, is_manual, shown, value};
use super::{USER_PEN, flag};
use crate::library::{ListInput, TextAreaField, field_label};
use crate::ui::icons::ROTATE_CCW;
use crate::ui::{Button, ButtonSize, ButtonVariant, Checkbox, Slider, Svg, TextField};

/// A field with its label, whether its value is the user's, Reset, and
/// what went wrong (the webview's `FieldFrame`).
#[component]
fn FieldFrame(
    field: GameField,
    manual: bool,
    #[props(default)] on_reset: Option<EventHandler<()>>,
    #[props(default)] error: Option<FieldError>,
    /// For a conflict: the value saved meanwhile.
    #[props(default)]
    theirs: Option<String>,
    #[props(default)] on_take: Option<EventHandler<()>>,
    #[props(default)] on_keep: Option<EventHandler<()>>,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    let label = field_label(field);
    let conflict = error
        .as_ref()
        .is_some_and(|error| error.problem == FieldProblem::Conflict)
        && theirs.is_some();
    rsx! {
        div {
            "data-part": "field",
            "data-field": field.key(),
            "data-manual": flag(manual),
            "data-invalid": flag(error.is_some()),
            class: "flex flex-col gap-1.5 {class}",
            div { class: "flex min-h-8 items-center gap-2",
                span { "data-part": "label", class: "text-sm font-medium", "{label}" }
                if manual && let Some(on_reset) = on_reset {
                    span {
                        "data-part": "manual",
                        title: t!("Set by you: refreshes keep it until you reset it."),
                        class: "inline-flex items-center gap-1 rounded-control border border-accent-alt/40 px-1.5 py-0.5 text-[11px] text-accent-alt",
                        Svg { icon: USER_PEN, class: "size-3" }
                        {t!("Yours")}
                    }
                    Button {
                        "data-part": "reset",
                        "data-spatial": "focusable",
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::Sm,
                        icon: ROTATE_CCW,
                        class: "ml-auto",
                        "aria-label": t!("Reset {label} to automatic", label = label.clone()),
                        on_press: move |_| on_reset.call(()),
                        {t!("Reset")}
                    }
                }
            }
            {children}
            if let Some(error) = error.clone() {
                div { role: "alert", "data-part": "error", class: "flex flex-wrap items-center gap-2",
                    span { class: "text-xs text-danger",
                        if conflict {
                            {t!(
                                "Changed elsewhere since you opened the editor; it is now “{theirs}”.",
                                theirs = theirs.clone().unwrap_or_default()
                            )}
                        } else {
                            "{error.message}"
                        }
                    }
                    if conflict {
                        Button {
                            "data-part": "use-theirs",
                            "data-spatial": "focusable",
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            on_press: move |_| {
                                if let Some(on_take) = on_take {
                                    on_take.call(());
                                }
                            },
                            {t!("Use theirs")}
                        }
                        Button {
                            "data-part": "keep-mine",
                            "data-spatial": "focusable",
                            size: ButtonSize::Sm,
                            variant: ButtonVariant::Ghost,
                            on_press: move |_| {
                                if let Some(on_keep) = on_keep {
                                    on_keep.call(());
                                }
                            },
                            {t!("Keep mine")}
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn DetailsTab(
    state: GameEditState,
    draft: Draft,
    on_draft: EventHandler<Draft>,
    errors: Vec<FieldError>,
    /// Takes the value saved meanwhile into the draft and the base.
    on_take_theirs: EventHandler<GameField>,
    /// Keeps the draft's value: the next save overwrites the one saved
    /// meanwhile.
    on_keep_mine: EventHandler<GameField>,
) -> Element {
    let set = {
        let (state, draft) = (state.clone(), draft.clone());
        move |field: GameField, next: Change| {
            on_draft.call(change(&state, &draft, field, Some(next)))
        }
    };
    // A field's frame: whether it's the user's, its error, and a conflict's
    // value saved meanwhile.
    let frame = |field: GameField| {
        let error = errors.iter().find(|error| error.field == field).cloned();
        let theirs = error
            .as_ref()
            .filter(|error| error.problem == FieldProblem::Conflict)
            .map(|_| value(&state.game, field).text());
        (is_manual(&state, &draft, field), error, theirs)
    };
    let text = |field: GameField| {
        let (manual, error, theirs) = frame(field);
        let current = shown(&state, &draft, field).text();
        let on_input = {
            let (state, draft, set) = (state.clone(), draft.clone(), set.clone());
            move |text: String| {
                if field == GameField::Title {
                    // A sort title that followed the title keeps following it.
                    let following = shown(&state, &draft, GameField::SortTitle)
                        == shown(&state, &draft, GameField::Title);
                    let mut next = change(
                        &state,
                        &draft,
                        GameField::Title,
                        Some(Change::Set(Value::Text(text.clone()))),
                    );
                    if following {
                        next = change(
                            &state,
                            &next,
                            GameField::SortTitle,
                            Some(Change::Set(Value::Text(text))),
                        );
                    }
                    on_draft.call(next);
                } else {
                    set(field, Change::Set(Value::Text(text)));
                }
            }
        };
        let reset = set.clone();
        rsx! {
            FieldFrame {
                field,
                manual,
                error,
                theirs,
                on_reset: move |_| reset(field, Change::Reset),
                on_take: move |_| on_take_theirs.call(field),
                on_keep: move |_| on_keep_mine.call(field),
                TextField { "aria-label": field_label(field), value: current, on_input }
            }
        }
    };
    let list = |field: GameField, split: bool| {
        let (manual, error, theirs) = frame(field);
        let values = shown(&state, &draft, field).list();
        let (reset, set) = (set.clone(), set.clone());
        rsx! {
            FieldFrame {
                field,
                manual,
                error,
                theirs,
                on_reset: move |_| reset(field, Change::Reset),
                on_take: move |_| on_take_theirs.call(field),
                on_keep: move |_| on_keep_mine.call(field),
                ListInput {
                    label: field_label(field),
                    values,
                    split,
                    on_change: move |values: Vec<String>| set(field, Change::Set(Value::List(values))),
                }
            }
        }
    };
    let area = |field: GameField, resettable: bool| {
        let (manual, error, theirs) = frame(field);
        let current = shown(&state, &draft, field).text();
        let (reset, set) = (set.clone(), set.clone());
        rsx! {
            FieldFrame {
                field,
                manual,
                error,
                theirs,
                class: "md:col-span-2",
                on_reset: resettable.then(|| EventHandler::new(move |_| reset(field, Change::Reset))),
                on_take: move |_| on_take_theirs.call(field),
                on_keep: move |_| on_keep_mine.call(field),
                TextAreaField {
                    label: field_label(field),
                    value: current,
                    on_change: move |text: String| set(field, Change::Set(Value::Text(text))),
                }
            }
        }
    };
    let date = {
        let field = GameField::ReleaseDate;
        let (manual, error, theirs) = frame(field);
        let current = shown(&state, &draft, field).text();
        let (reset, set) = (set.clone(), set.clone());
        rsx! {
            FieldFrame {
                field,
                manual,
                error,
                theirs,
                on_reset: move |_| reset(field, Change::Reset),
                on_take: move |_| on_take_theirs.call(field),
                on_keep: move |_| on_keep_mine.call(field),
                TextField {
                    "aria-label": field_label(field),
                    placeholder: t!("YYYY-MM-DD"),
                    mono: true,
                    value: current,
                    on_input: move |text: String| set(field, Change::Set(Value::Text(text))),
                }
            }
        }
    };
    let rating = {
        let field = GameField::Rating;
        let (manual, error, theirs) = frame(field);
        let current = shown(&state, &draft, field).rating();
        let (check, slide) = (set.clone(), set.clone());
        rsx! {
            FieldFrame {
                field,
                manual,
                error,
                theirs,
                on_take: move |_| on_take_theirs.call(field),
                on_keep: move |_| on_keep_mine.call(field),
                div { class: "flex flex-wrap items-center gap-4",
                    Checkbox {
                        "data-spatial": "focusable",
                        checked: current.is_some(),
                        label: t!("Rated"),
                        on_change: move |rated: bool| {
                            check(field, Change::Set(Value::Rating(rated.then_some(current.unwrap_or(50)))))
                        },
                    }
                    Slider {
                        label: field_label(field),
                        class: "flex-1",
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        disabled: current.is_none(),
                        value: f64::from(current.unwrap_or(0)),
                        format: Callback::new(|value: f64| t!("{value} / 100", value = value.round())),
                        on_change: move |value: f64| {
                            slide(field, Change::Set(Value::Rating(Some(value.round().clamp(0.0, 100.0) as u8))))
                        },
                    }
                }
            }
        }
    };
    rsx! {
        div { "data-part": "details", class: "grid gap-x-6 gap-y-4 md:grid-cols-2",
            {text(GameField::Title)}
            {text(GameField::SortTitle)}
            {area(GameField::Description, true)}
            {list(GameField::Genres, true)}
            {list(GameField::Tags, true)}
            {list(GameField::Platforms, false)}
            {list(GameField::Aliases, false)}
            {date}
            {rating}
            {area(GameField::Notes, false)}
        }
    }
}
