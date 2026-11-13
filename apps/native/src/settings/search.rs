//! Settings search (#46, ported in #103): what each section holds, as a
//! static table of translatable labels and the other words people look for
//! them by (the webview's `searchIndex.ts`, entry for entry, with the same
//! messages and contexts, so they share translations). The search never
//! reads the panels: a label here is written as its panel shows it, which
//! is also how the panel's row is found and highlighted once the section
//! opens (`ui::Found`).
//!
//! Add a row here when a panel gains a preference worth finding. Words are
//! one translatable, comma-separated list per entry.

use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::{SECTIONS, Section};
use crate::ui::interact::{Keys, use_interaction};
use crate::ui::{SearchField, fold};

mod search_index;
pub use search_index::{INDEX, section_words};

/// Whether every word of `query` appears somewhere in `texts`.
pub fn matches(query: &str, texts: &[&str]) -> bool {
    let query = fold(query);
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return false;
    }
    let haystack = texts
        .iter()
        .map(|text| fold(text))
        .collect::<Vec<_>>()
        .join("\n");
    terms.iter().all(|term| haystack.contains(term))
}

/// A search result: a section, or a preference in one.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub key: String,
    pub tab: &'static str,
    pub section: Option<&'static str>,
    /// The preference's label as its panel shows it; none for a section.
    pub label: Option<String>,
    /// What the result shows: the section's or the preference's name.
    pub title: String,
    /// The section a preference is in.
    pub within: Option<String>,
}

/// The most results shown.
const MOST: usize = 30;

/// The results for `query` among `sections` (the ones this account sees):
/// sections first, then preferences, each those whose name matches before
/// those found by another word.
pub fn search_settings(query: &str, sections: &[&Section]) -> Vec<SearchResult> {
    let mut found: Vec<(SearchResult, bool)> = sections
        .iter()
        .filter_map(|section| {
            let title = (section.label)();
            let words = section_words(section.id);
            if !matches(query, &[&title, &words]) {
                return None;
            }
            let named = matches(query, &[&title]);
            Some((
                SearchResult {
                    key: section.id.to_owned(),
                    tab: section.id,
                    section: None,
                    label: None,
                    title,
                    within: None,
                },
                named,
            ))
        })
        .collect();
    let mut preferences: Vec<(SearchResult, bool)> = INDEX
        .iter()
        .filter_map(|entry| {
            let within = sections.iter().find(|section| section.id == entry.tab)?;
            let label = (entry.label)();
            let words = entry.words.map(|words| words()).unwrap_or_default();
            if !matches(query, &[&label, &words]) {
                return None;
            }
            let named = matches(query, &[&label]);
            Some((
                SearchResult {
                    key: format!("{}\n{}\n{label}", entry.tab, entry.section.unwrap_or("")),
                    tab: entry.tab,
                    section: entry.section,
                    label: Some(label.clone()),
                    title: label,
                    within: Some((within.label)()),
                },
                named,
            ))
        })
        .collect();
    // Stable: names first, the table's order within each.
    found.sort_by_key(|(_, named)| !named);
    preferences.sort_by_key(|(_, named)| !named);
    found
        .into_iter()
        .chain(preferences)
        .map(|(result, _)| result)
        .take(MOST)
        .collect()
}

/// The search above the section list: a field, and in place of the list
/// while it has a query, the results. Enter opens the first, Escape clears
/// it; a pad types in it on the on-screen keyboard like any other field.
#[component]
pub fn SettingsSearch(
    query: String,
    on_query: EventHandler<String>,
    sections: Vec<&'static str>,
    on_open: EventHandler<SearchResult>,
) -> Element {
    let shown: Vec<&Section> = SECTIONS
        .iter()
        .filter(|section| sections.contains(&section.id))
        .collect();
    let results = if query.trim().is_empty() {
        Vec::new()
    } else {
        search_settings(&query, &shown)
    };
    let first = results.first().cloned();
    rsx! {
        div { class: "flex min-h-0 flex-col gap-2 px-2 pt-2",
            SearchField {
                value: query.clone(),
                on_input: move |value: String| on_query.call(value),
                label: t!("Search settings"),
                placeholder: t!("Search settings"),
                on_submit: move |_| {
                    if let Some(first) = first.clone() {
                        on_open.call(first);
                    }
                },
            }
            if !query.trim().is_empty() {
                if results.is_empty() {
                    p { role: "status", class: "px-1 text-sm text-fg-muted",
                        {t!("No settings match “{query}”.", query = query.clone())}
                    }
                } else {
                    ul {
                        "aria-label": t!("Settings search results"),
                        class: "-mx-1 flex min-h-0 flex-col gap-1 overflow-y-auto overscroll-contain p-1",
                        for result in results {
                            li { key: "{result.key}",
                                ResultButton { result, on_open }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ResultButton(result: SearchResult, on_open: EventHandler<SearchResult>) -> Element {
    let interaction = use_interaction();
    let name = match &result.within {
        Some(within) => t!(
            note = "A settings search result: a preference and its section",
            "{title} in {section}",
            title = result.title.clone(),
            section = within.clone()
        ),
        None => result.title.clone(),
    };
    let open = {
        let result = result.clone();
        use_callback(move |()| on_open.call(result.clone()))
    };
    rsx! {
        button {
            r#type: "button",
            "aria-label": "{name}",
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            class: "flex w-full cursor-pointer flex-col items-start rounded-control border border-transparent px-3 py-2 text-left text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) outline-none data-focus-visible:focus-ring data-hovered:bg-surface-raised data-hovered:text-fg",
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| open(()),
            onkeydown: move |event: KeyboardEvent| {
                if event.key() == Key::Named(NamedKey::Escape) {
                    return;
                }
                interaction.key_down(&event, Keys::EnterAndSpace, || open(()));
            },
            onkeyup: move |event| interaction.key_up(&event, || open(())),
            span { class: "min-w-0 break-words text-fg", "{result.title}" }
            if let Some(within) = &result.within {
                span { "aria-hidden": "true", class: "text-xs text-fg-muted", "{within}" }
            }
        }
    }
}
