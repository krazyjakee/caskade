//! Appearance → Theme (#140): installed themes, their previews, problems,
//! rescanning and development folder loading.

use caskade_core::themes::{ThemeInfo, ThemeProblem, ThemeSource};
use caskade_ui_i18n::{t, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use crate::data::{Query, change, use_themes_query};
use crate::ui::icons::{FOLDER_OPEN, REFRESH_CW};
use crate::ui::{Button, ButtonSize, ButtonVariant, FailureNotice, NoticeVariant};

/// A theme's previews, as the window loads them: `caskade-theme:` URLs.
pub fn preview_url(theme: &str, path: &str) -> String {
    // What a URL path segment can't hold as it is.
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'%')
        .add(b'/')
        .add(b'<')
        .add(b'>')
        .add(b'?')
        .add(b'`')
        .add(b'{')
        .add(b'}');
    let encode = |text: &str| percent_encoding::utf8_percent_encode(text, SEGMENT).to_string();
    let path = path.split('/').map(encode).collect::<Vec<_>>().join("/");
    format!("caskade-theme://localhost/{}/{path}", encode(theme))
}

fn problem_text(problems: &[ThemeProblem]) -> String {
    problems
        .iter()
        .map(|problem| {
            let mut at = vec![problem.file.clone()];
            at.extend(problem.line.map(|line| line.to_string()));
            at.extend(problem.column.map(|column| column.to_string()));
            format!("{}: {}", at.join(":"), problem.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn source_label(source: ThemeSource) -> Option<String> {
    match source {
        ThemeSource::Builtin => None,
        ThemeSource::User => Some(tp!("theme source", "Installed")),
        ThemeSource::Plugin => Some(tp!("theme source", "From a plugin")),
        ThemeSource::Dev => Some(tp!("theme source", "Dev folder")),
    }
}

/// Appearance → Theme (#140): every installed theme with its name, author
/// and previews; one that can't be used shows why and can't be picked.
/// A radio group: one tab stop, arrows move and pick.
#[component]
pub fn ThemePicker(value: String, on_change: EventHandler<String>) -> Element {
    let mut themes: Query<Vec<ThemeInfo>> =
        use_themes_query(|themes| async move { themes.list().await });
    let active = use_themes_query(|themes| async move { themes.active().await });
    let mut rescan_error = use_signal(|| None);
    let mut rescanning = use_signal(|| false);
    let handle = crate::data::try_use_themes();
    let rescan = use_callback(move |()| {
        let Some(handle) = handle.clone() else {
            return;
        };
        rescanning.set(true);
        rescan_error.set(None);
        spawn(async move {
            let outcome = change(async move { handle.reload().await }).await;
            rescanning.set(false);
            match outcome {
                Ok(_) => themes.restart(),
                Err(error) => rescan_error.set(Some(error)),
            }
        });
    });
    let notice = match &*active.read() {
        Some(Ok(active)) if active.safe_mode || active.requested == value => active.notice.clone(),
        _ => None,
    };
    let list = themes.read().clone();
    // The usable themes in order: where arrows go.
    let usable: Vec<String> = match &list {
        Some(Ok(list)) => list
            .iter()
            .filter(|theme| theme.problems.is_empty())
            .map(|theme| theme.id.clone())
            .collect(),
        _ => Vec::new(),
    };
    let stop = Some(value.clone())
        .filter(|value| usable.contains(value))
        .or_else(|| usable.first().cloned());
    rsx! {
        div { "data-part": "theme-picker", class: "flex flex-col gap-3 py-2",
            if let Some(notice) = notice {
                p {
                    role: "status",
                    class: "rounded-control border border-warning/40 bg-warning/10 px-3 py-2 text-sm",
                    "{notice}"
                }
            }
            match list {
                None => rsx! {
                    p { role: "status", class: "text-sm text-fg-muted", {t!("Loading…")} }
                },
                Some(Err(error)) => rsx! {
                    FailureNotice {
                        error,
                        variant: NoticeVariant::Inline,
                        title: t!("Themes couldn't be loaded"),
                        on_retry: move |_| themes.restart(),
                    }
                },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    p { class: "text-sm text-fg-muted", {t!("No themes found. Rescan themes looks for them again.")} }
                },
                Some(Ok(list)) => rsx! {
                    div {
                        role: "radiogroup",
                        "aria-label": t!("Theme"),
                        class: "grid gap-3 sm:grid-cols-2 2xl:grid-cols-3",
                        for (index, theme) in list.into_iter().enumerate() {
                            ThemeOption {
                                key: "{theme.id}:{index}",
                                index,
                                checked: theme.problems.is_empty() && theme.id == value,
                                stop: theme.problems.is_empty() && Some(&theme.id) == stop.as_ref(),
                                previous: neighbour(&usable, &theme.id, false),
                                next: neighbour(&usable, &theme.id, true),
                                theme,
                                on_check: on_change,
                            }
                        }
                    }
                },
            }
            if let Some(error) = rescan_error() {
                FailureNotice {
                    error,
                    variant: NoticeVariant::Inline,
                    on_retry: move |_| rescan(()),
                }
            }
            div { class: "flex flex-wrap items-center justify-end gap-2",
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    icon: Some(REFRESH_CW),
                    disabled: rescanning(),
                    on_press: move |_| rescan(()),
                    {t!("Rescan themes")}
                }
                LoadThemeFolder { on_loaded: on_change, themes }
            }
        }
    }
}

/// The usable theme before or after `id`, wrapping.
fn neighbour(usable: &[String], id: &str, forward: bool) -> Option<String> {
    let at = usable.iter().position(|known| known == id)?;
    let next = if forward {
        (at + 1) % usable.len()
    } else {
        (at + usable.len() - 1) % usable.len()
    };
    Some(usable[next].clone())
}

/// One theme: a radio card.
#[component]
fn ThemeOption(
    theme: ThemeInfo,
    index: usize,
    checked: bool,
    stop: bool,
    previous: Option<String>,
    next: Option<String>,
    on_check: EventHandler<String>,
) -> Element {
    let interaction = crate::ui::interact::use_interaction_where(stop);
    let label_id = format!("theme-option-{index}");
    let broken = !theme.problems.is_empty();
    let id = theme.id.clone();
    let check = use_callback(move |()| {
        if !broken {
            on_check.call(id.clone());
        }
    });
    let mut node = use_signal(|| None::<NodeHandle>);
    // Arrows check the next usable theme and move focus there.
    let go = move |to: Option<String>| {
        if let Some(to) = to {
            on_check.call(to.clone());
            if let Some(handle) = node.peek().clone() {
                let doc = handle.doc();
                let mut found = None;
                doc.visit(|id, node| {
                    if node.attr(blitz_dom::LocalName::from("data-theme")) == Some(to.as_str()) {
                        found = Some(id);
                    }
                });
                drop(doc);
                if let Some(found) = found {
                    handle.doc_mut().set_focus_to(found);
                }
            }
        }
    };
    let source = source_label(theme.source);
    let problems = problem_text(&theme.problems);
    let warnings = problem_text(&theme.warnings);
    let previews: Vec<String> = theme
        .previews
        .iter()
        .map(|preview| preview_url(&theme.id, preview))
        .collect();
    rsx! {
        div {
            role: "radio",
            tabindex: if stop { "0" } else { "-1" },
            "aria-checked": if checked { "true" } else { "false" },
            "aria-disabled": crate::ui::interact::flag(broken),
            "aria-labelledby": "{label_id}",
            "data-part": "theme",
            "data-theme": "{theme.id}",
            "data-selected": crate::ui::interact::flag(checked),
            "data-disabled": crate::ui::interact::flag(broken),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            class: "flex cursor-pointer flex-col gap-2 rounded-card border border-border bg-surface-overlay/60 p-3 transition-colors duration-(--cs-motion-duration-fast) data-disabled:cursor-not-allowed data-focus-visible:focus-ring data-hovered:border-border-strong data-selected:border-accent data-selected:bg-surface-hover",
            onmounted: move |event| node.set(event.data().downcast::<NodeHandle>().cloned()),
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| check(()),
            onkeydown: {
                let previous = previous.clone();
                let next = next.clone();
                move |event: KeyboardEvent| {
                    use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
                    match event.key() {
                        Key::Named(NamedKey::ArrowDown | NamedKey::ArrowRight) => {
                            event.prevent_default();
                            go(next.clone());
                        }
                        Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft) => {
                            event.prevent_default();
                            go(previous.clone());
                        }
                        _ => {
                            interaction.key_down(
                                &event,
                                crate::ui::interact::Keys::EnterAndSpace,
                                || check(()),
                            );
                        }
                    }
                }
            },
            onkeyup: move |event| interaction.key_up(&event, || check(())),
            if previews.is_empty() {
                div {
                    "aria-hidden": "true",
                    class: "aspect-video w-full rounded-control border border-border bg-surface-base",
                }
            } else {
                div { class: "grid auto-cols-fr grid-flow-col gap-1",
                    // Each image fills a sized box, as GameArt's: an <img>
                    // sized by its aspect ratio in a grid track lays out as
                    // NaN in Blitz while it loads.
                    for preview in previews {
                        div {
                            key: "{preview}",
                            class: "relative aspect-video w-full overflow-hidden rounded-control border border-border bg-surface-base",
                            img {
                                src: "{preview}",
                                alt: "",
                                class: "absolute inset-0 size-full object-cover",
                            }
                        }
                    }
                }
            }
            span { class: "flex items-baseline justify-between gap-2",
                span { id: "{label_id}", class: "truncate font-display font-semibold tracking-wide",
                    "{theme.name}"
                }
                if !theme.version.is_empty() {
                    span { class: "shrink-0 font-mono text-xs text-fg-muted", "{theme.version}" }
                }
            }
            if let Some(author) = &theme.author {
                span { class: "text-xs text-fg-muted",
                    {t!(note = "A theme's author", "by {0}", 0 = author.clone())}
                }
            }
            if let Some(description) = &theme.description {
                span { class: "text-xs text-fg-muted", "{description}" }
            }
            if let Some(source) = source {
                span { class: "font-mono text-[0.7rem] tracking-wider text-accent-alt uppercase",
                    "{source}"
                }
            }
            if broken {
                span { class: "text-xs whitespace-pre-line text-danger",
                    {t!("Can't be used:")}
                    "\n{problems}"
                }
            } else if !warnings.is_empty() {
                span { class: "text-xs whitespace-pre-line text-warning", "{warnings}" }
            }
        }
    }
}

/// Development builds: loads a theme folder in place, chosen in the app's
/// folder browser.
#[component]
fn LoadThemeFolder(on_loaded: EventHandler<String>, themes: Query<Vec<ThemeInfo>>) -> Element {
    let handle = crate::data::try_use_themes();
    let picker = crate::flows::paths::use_path_picker();
    let mut error = use_signal(|| None);
    let mut loading = use_signal(|| false);
    let Some(handle) = handle.filter(|handle| handle.loads_unpacked()) else {
        return rsx! {};
    };
    let load = move |_| {
        let handle = handle.clone();
        error.set(None);
        spawn(async move {
            let request = crate::flows::paths::PathRequest::new(
                crate::flows::paths::PickKind::Folder,
                t!("Theme folder"),
            );
            let Some(folder) = picker.pick(request).await else {
                return;
            };
            loading.set(true);
            let outcome = change(async move { handle.load_unpacked(&folder).await }).await;
            loading.set(false);
            match outcome {
                Ok(id) => {
                    let mut themes = themes;
                    themes.restart();
                    on_loaded.call(id);
                }
                Err(failed) => error.set(Some(failed)),
            }
        });
    };
    rsx! {
        Button {
            variant: ButtonVariant::Ghost,
            size: ButtonSize::Sm,
            icon: Some(FOLDER_OPEN),
            disabled: loading(),
            on_press: load,
            {t!("Load theme folder…")}
        }
        if let Some(failed) = error() {
            FailureNotice { error: failed, variant: NoticeVariant::Inline }
        }
    }
}
