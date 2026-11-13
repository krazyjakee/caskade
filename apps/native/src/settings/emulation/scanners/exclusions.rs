//! What scans skip: a scanner's exclusions and the global ones, with the
//! pattern syntax.

use super::*;

/// An exclusion pattern, as the placeholder shows it.
pub(super) const EXAMPLE_PATTERN: &str = ">*.sav";

/// The exclusion syntax, with what each example skips.
pub(super) fn exclusion_syntax() -> [(&'static str, String); 4] {
    [
        ("bios/", t!("This folder or file in the scan folder")),
        (
            ">*.txt",
            t!("This name in any folder; * stands for any text"),
        ),
        (
            "?^beta/.*",
            t!("A regular expression, from the start of the path"),
        ),
        (
            ">?\\(Beta\\)",
            t!("A regular expression, anywhere in the path"),
        ),
    ]
}

/// Exclusion patterns with remove buttons and a field to add one.
#[component]
pub(super) fn ExclusionList(
    /// Every scanner's list rather than one scanner's.
    #[props(default)]
    global: bool,
    patterns: Vec<String>,
    on_change: EventHandler<Vec<String>>,
    #[props(default)] disabled: bool,
) -> Element {
    let mut pattern = use_signal(String::new);
    let prefix = if global {
        "global-exclusion"
    } else {
        "exclusion"
    };
    let mut add = {
        let patterns = patterns.clone();
        move || {
            let next = pattern.peek().trim().to_owned();
            if !next.is_empty() && !patterns.contains(&next) {
                let mut all = patterns.clone();
                all.push(next);
                on_change.call(all);
            }
            pattern.set(String::new());
        }
    };
    let mut add_on_enter = add.clone();
    let can_add = !disabled && !pattern().trim().is_empty();
    let (list_label, new_label) = if global {
        (t!("Global exclusions"), t!("New global exclusion pattern"))
    } else {
        (t!("Exclusions"), t!("New exclusion pattern"))
    };
    // Each pattern's place, the pattern and its remove button's label.
    let entries: Vec<(usize, String, String)> = patterns
        .iter()
        .enumerate()
        .map(|(index, exclusion)| {
            let label = if global {
                t!(
                    "Remove global exclusion {pattern}",
                    pattern = exclusion.clone()
                )
            } else {
                t!("Remove exclusion {pattern}", pattern = exclusion.clone())
            };
            (index, exclusion.clone(), label)
        })
        .collect();
    rsx! {
        ul {
            "aria-label": list_label,
            class: "mt-2 flex flex-col gap-1",
            for (index, exclusion, label) in entries {
                li {
                    key: "{exclusion}",
                    "data-part": "exclusion",
                    class: "flex items-center gap-2 rounded-control border border-border bg-surface-base/60 py-1 pr-1 pl-3",
                    code { class: "flex-1 truncate font-mono text-xs", "{exclusion}" }
                    RemoveExclusion {
                        id: format!("{prefix}-remove-{index}"),
                        label,
                        disabled,
                        on_press: {
                            let (patterns, exclusion) = (patterns.clone(), exclusion.clone());
                            move |_| {
                                on_change.call(
                                    patterns.iter().filter(|other| **other != exclusion).cloned().collect(),
                                )
                            }
                        },
                    }
                }
            }
            if patterns.is_empty() {
                li { class: "text-xs text-fg-muted italic", {t!("No exclusions.")} }
            }
        }
        div { class: "mt-2",
            TextField {
                id: "{prefix}-new",
                "aria-label": new_label,
                mono: true,
                placeholder: EXAMPLE_PATTERN.to_owned(),
                value: pattern(),
                on_input: move |next: String| pattern.set(next),
                on_submit: move |_| {
                    if can_add {
                        add_on_enter();
                    }
                },
                action: rsx! {
                    Button {
                        id: "{prefix}-add",
                        icon: PLUS,
                        "aria-label": global.then(|| t!("Add global exclusion")),
                        disabled: !can_add,
                        on_press: move |_| add(),
                        {t!(note = "Add the exclusion pattern", "Add")}
                    }
                },
            }
        }
    }
}

/// An exclusion's remove button.
#[component]
pub(super) fn RemoveExclusion(
    id: String,
    label: String,
    disabled: bool,
    on_press: EventHandler<()>,
) -> Element {
    let interaction = use_interaction();
    let press = move || {
        if !disabled {
            on_press.call(());
        }
    };
    rsx! {
        button {
            r#type: "button",
            id: "{id}",
            "aria-label": "{label}",
            "aria-disabled": flag(disabled),
            "data-disabled": flag(disabled),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            class: "grid size-7 place-items-center rounded-[6px] text-fg-subtle outline-none data-focus-visible:focus-ring data-hovered:bg-surface-hover data-hovered:text-danger",
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onclick: move |_| press(),
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::EnterAndSpace, press);
            },
            onkeyup: move |event| interaction.key_up(&event, press),
            Svg { icon: X, class: "size-4" }
        }
    }
}

/// How to write an exclusion pattern.
#[component]
pub(super) fn ExclusionSyntax() -> Element {
    rsx! {
        aside { class: "rounded-control border border-accent-alt/25 bg-accent-alt/5 p-3 text-xs text-fg-muted",
            div { class: "mb-2 font-mono tracking-wider text-accent-alt uppercase",
                {t!(note = "How to write an exclusion pattern", "Patterns")}
            }
            p { class: "mb-2",
                {t!("Type a path to skip that folder or file. Start with > to skip a name wherever it turns up: >*.txt skips every text file.")}
            }
            dl { class: "grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5",
                for (pattern, meaning) in exclusion_syntax() {
                    div { key: "{pattern}", class: "contents",
                        dt {
                            code { class: "font-mono text-fg", "{pattern}" }
                        }
                        dd { "{meaning}" }
                    }
                }
            }
        }
    }
}

/// Exclusions every scanner applies (§6.3); saved as soon as they change.
#[component]
pub(super) fn GlobalExclusions(reload: Signal<u64>) -> Element {
    let emulation = use_emulation();
    let live = use_live();
    let loaded = {
        let emulation = emulation.clone();
        use_resource(move || {
            let _ = (live.changed)();
            let _ = reload();
            let emulation = emulation.clone();
            change(async move { emulation.exclusions().await })
        })
    };
    // What the last save returned, until the list is read again.
    let mut saved = use_signal(|| None::<Vec<String>>);
    use_effect(move || {
        let _ = loaded.read();
        if saved.peek().is_some() {
            saved.set(None);
        }
    });
    // The patterns being saved, shown while they are.
    let mut pending = use_signal(|| None::<Vec<String>>);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let save = move |patterns: Vec<String>| {
        pending.set(Some(patterns.clone()));
        failure.set(None);
        let emulation = emulation.clone();
        spawn(async move {
            let result = change(async move { emulation.set_exclusions(patterns).await }).await;
            match result {
                Ok(next) => saved.set(Some(next)),
                Err(report) => failure.set(Some(report)),
            }
            pending.set(None);
        });
    };
    let state = match &*loaded.read() {
        None => None,
        Some(Ok(patterns)) => Some(Ok(patterns.clone())),
        Some(Err(report)) => Some(Err(report.clone())),
    };
    rsx! {
        SettingsSection {
            id: "global-exclusions",
            icon: FILTER_X,
            title: t!("Global exclusions"),
            description: t!("Files and folders every scanner skips, from its next scan."),
            match state {
                Some(Err(report)) => rsx! {
                    p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
                },
                None => rsx! {
                    p { role: "status", class: "mt-3 text-sm text-fg-muted", {t!("Loading global exclusions…")} }
                },
                Some(Ok(patterns)) => rsx! {
                    div { class: "mt-4 grid gap-4",
                        div {
                            ExclusionList {
                                global: true,
                                patterns: pending().or(saved()).unwrap_or(patterns),
                                on_change: save,
                                disabled: pending().is_some(),
                            }
                        }
                        ExclusionSyntax {}
                    }
                },
            }
            if let Some(report) = failure() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{report.summary}" }
            }
        }
    }
}
