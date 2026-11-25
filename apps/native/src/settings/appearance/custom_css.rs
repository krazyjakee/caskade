//! Appearance → Custom CSS (#142): user CSS validation, unsaved-changes
//! wiring, and quick links to the theming contract docs.

use caskade_ui_i18n::{format, t, tp};
use caskade_ui_theme::user_css::{self, CssProblem, USER_CSS_MAX};
use dioxus_native::prelude::*;

use super::super::unsaved::{UnsavedForm, UnsavedHint, use_unsaved_changes};
use crate::ui::icons::{BOOK_OPEN, POWER_OFF};
use crate::ui::{Button, ButtonSize, ButtonVariant, Link, SettingRow, Svg, Switch};

/// Example in the empty CSS field: CSS syntax, not translated.
const CSS_EXAMPLE: &str =
    "[data-cs=\"game-card\"] [data-part=\"title\"] {\n  color: var(--cs-color-accent);\n}";

/// The theming contract's documentation (`docs/theming/`).
const THEMING_DOCS: &str = "https://github.com/krazyjakee/caskade/blob/master/docs/theming/";

fn css_problems(problems: &[CssProblem]) -> String {
    user_css::problem_text(problems)
}

/// Appearance → Custom CSS (#142): the user's rules in the `cs.user` layer,
/// saved through the settings and applied without a reload. A switch turns
/// them off without losing them, for a look that broke. The CSS waits for
/// Save CSS (#43): leaving the section asks first.
#[component]
pub(super) fn CustomCssEditor(
    css: String,
    enabled: bool,
    saving: bool,
    on_save: EventHandler<String>,
    on_enabled_change: EventHandler<bool>,
) -> Element {
    let mut draft = use_signal(|| None::<String>);
    let mut problems = use_signal(Vec::<CssProblem>::new);
    let safe_mode = caskade_app::commands::themes::safe_mode(None);
    let saved = css.clone();
    // Once the saved value is what was typed, the draft is done.
    use_effect(use_reactive!(|saved| {
        if draft.peek().as_deref() == Some(saved.as_str()) {
            draft.set(None);
        }
    }));
    let text = draft().unwrap_or_else(|| css.clone());
    let too_long = text.chars().count() > USER_CSS_MAX;
    let dirty = draft().is_some_and(|draft| draft != css);
    let lines = text.split('\n').count();
    let save = use_callback(move |()| {
        let text = draft.peek().clone().unwrap_or_default();
        let found = user_css::check(&text);
        let refused = !found.is_empty() || text.chars().count() > USER_CSS_MAX;
        problems.set(found);
        if !refused {
            on_save.call(text);
        }
        !refused
    });
    let revert = use_callback(move |()| {
        draft.set(None);
        problems.set(Vec::new());
    });
    use_unsaved_changes(
        dirty,
        UnsavedForm {
            label: t!("Custom CSS"),
            save: std::rc::Rc::new(move || Box::pin(async move { save(()) })),
            discard: std::rc::Rc::new(move || revert(())),
        },
    );
    let count = t!(
        "{length} / {max} characters",
        length = format::count(text.chars().count() as f64),
        max = format::count(USER_CSS_MAX as f64)
    );
    let docs = [
        (
            format!("{THEMING_DOCS}anatomy.md"),
            tp!("theming docs", "Selectors"),
        ),
        (
            format!("{THEMING_DOCS}tokens.md"),
            tp!("theming docs", "Tokens"),
        ),
        (format!("{THEMING_DOCS}README.md"), t!("Theming guide")),
    ];
    rsx! {
        div { "data-part": "custom-css", class: "flex flex-col gap-3",
            SettingRow {
                icon: Some(POWER_OFF),
                label: t!("Disable custom CSS"),
                description: t!("Keeps your CSS but stops applying it, to recover from a broken look."),
                Switch {
                    checked: !enabled,
                    on_change: move |off: bool| on_enabled_change.call(!off),
                }
            }
            if safe_mode {
                p {
                    role: "status",
                    class: "rounded-control border border-warning/40 bg-warning/10 px-3 py-2 text-sm",
                    {t!("Safe mode is on: your custom CSS isn't applied until Caskade starts normally.")}
                }
            }
            div { class: "flex overflow-hidden rounded-control border border-border-strong bg-surface-base/60 font-mono text-xs leading-5 focus-within:border-accent-alt/60",
                div {
                    "aria-hidden": "true",
                    class: "shrink-0 overflow-hidden border-r border-border py-2 pr-2 pl-3 text-right text-fg-subtle select-none",
                    for number in 1..=lines {
                        div { key: "{number}", "{number}" }
                    }
                }
                textarea {
                    "aria-label": t!("Custom CSS"),
                    "data-part": "css",
                    "data-spatial": "focusable",
                    spellcheck: "false",
                    wrap: "off",
                    rows: "14",
                    placeholder: CSS_EXAMPLE,
                    value: "{text}",
                    class: "block h-72 w-full flex-1 resize-y bg-transparent py-2 pr-3 pl-2 text-fg outline-none placeholder:text-fg-subtle",
                    oninput: move |event| draft.set(Some(event.value())),
                }
            }
            div { class: "flex flex-wrap items-center gap-2",
                span {
                    class: if too_long { "mr-auto font-mono text-xs text-danger" } else { "mr-auto font-mono text-xs text-fg-subtle" },
                    "{count}"
                }
                UnsavedHint { dirty }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    disabled: draft().is_none(),
                    on_press: move |_| revert(()),
                    {t!("Revert")}
                }
                Button {
                    variant: ButtonVariant::Primary,
                    size: ButtonSize::Sm,
                    disabled: draft().is_none() || saving || too_long,
                    on_press: move |_| {
                        save(());
                    },
                    {t!("Save CSS")}
                }
            }
            if !problems().is_empty() {
                div { role: "alert", class: "text-xs text-danger",
                    p { class: "font-medium", {t!("Custom CSS wasn't saved:")} }
                    p { class: "font-mono whitespace-pre-line", {css_problems(&problems())} }
                }
            }
            p { class: "flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-fg-muted",
                Svg { icon: BOOK_OPEN, class: "size-3.5" }
                {t!("Your rules beat the theme and every component. Style through the contract:")}
                for (href, label) in docs {
                    Link {
                        key: "{href}",
                        href: href.clone(),
                        class: "rounded-sm text-accent-alt underline-offset-2 hover:underline data-focus-visible:focus-ring",
                        on_press: move |_| {
                            if let Ok(url) = url::Url::parse(&href) {
                                caskade_app::navigation::open_in_browser(&url);
                            }
                        },
                        "{label}"
                    }
                }
            }
        }
    }
}
