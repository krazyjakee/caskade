//! Settings (#103; the webview's `SettingsView.tsx` and `views/settings`):
//! the sections in groups down the side, the settings search above them,
//! and the open section's panel. A section opens in place (Back leaves
//! Settings), LB/RB move between sections, and a section the signed-in
//! account may not change isn't listed.
//!
//! The framework the panels build on: [`SchemaSettings`] renders a section
//! from the core's JSON Schema; [`DraftField`] and [`DraftSlider`] save
//! once the user is done; [`ResetDefaults`] says what a reset changes
//! first; [`SettingsStatus`] shows a section's load and writes; and
//! [`UnsavedChangesGuard`] asks before edits a Save button holds are lost.
//! The panels themselves come with #104 and #105; until then most show
//! what they will hold.

mod about;
mod accounts;
pub(crate) mod addons;
mod appearance;
mod backup;
mod devices;
mod discovery;
mod drafts;
mod general;
mod launchers;
mod panels;
mod pin;
mod plugins;
mod reset;
mod schema;
mod scripts;
mod search;
mod state;
mod statistics;
mod storage;
mod stores;
mod streaming;
mod sync;
#[cfg(test)]
mod tests;
mod torrent;
mod unsaved;

#[allow(unused_imports)]
pub use drafts::{DraftField, DraftSlider};
#[allow(unused_imports)]
pub use reset::{ResetDefaults, ResetField, ResetPart};
#[allow(unused_imports)]
pub use schema::SchemaSettings;
#[allow(unused_imports)]
pub use state::{
    SectionState, SettingsFeedback, SettingsLoading, SettingsStatus, use_settings_section,
};
#[allow(unused_imports)]
pub use unsaved::{
    Leaving, UnsavedChangesGuard, UnsavedForm, after_unsaved_changes, holds_close,
    use_unsaved_changes,
};

use caskade_ui_data::{Editable, Key};
use caskade_ui_i18n::{t, tp};
use caskade_ui_input::Action;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key as KeyName, NamedKey};
use dioxus_native::prelude::*;

use crate::data::use_settings_query;
use crate::input::{Bind, use_input, use_input_action};
use crate::router::{Route, use_router};
use crate::ui::icons::{
    ACTIVITY, CAST, DATABASE_BACKUP, GAMEPAD, HARD_DRIVE, INFO, MONITOR_SMARTPHONE, PACKAGE_OPEN,
    PALETTE, PUZZLE, ROCKET, SCAN_SEARCH, SETTINGS_2, SQUARE_TERMINAL, STORE, USER_ROUND, USERS,
};
use crate::ui::interact::flag;
use crate::ui::{Found, Icon, Svg};
use search::{SearchResult, SettingsSearch};

pub const ANATOMY: Entry = Entry {
    parts: &[
        (
            "group",
            "A group's heading, before its sections (App, Library, …); not a stop for focus.",
        ),
        (
            "tab",
            "A section; the open one carries `data-selected`, and each `data-hovered`, `data-pressed` and `data-focus-visible`.",
        ),
        ("icon", "A section's icon."),
        ("label", "A section's name; long names wrap."),
    ],
    interaction: &["data-orientation"],
    ..Entry::new(
        "settings-nav",
        "SettingsView (apps/native/src/settings/mod.rs)",
        Source::App,
        "Settings' sections: a vertical tab list beside the open section's panel, in desktop and console modes, grouped under headings, with the settings search above it. It stays in view while the panel scrolls, and scrolls on its own when the sections don't fit.",
    )
};

/// A group of sections under a heading in the list (#46).
pub struct Group {
    pub id: &'static str,
    pub label: fn() -> String,
}

/// A section: its id (the route's `tab`, in saved selections and links),
/// name, icon and group.
pub struct Section {
    pub id: &'static str,
    pub label: fn() -> String,
    pub icon: Icon,
    pub group: &'static str,
}

/// The groups, in order.
pub const GROUPS: [Group; 6] = [
    Group {
        id: "app",
        label: || tp!("settings group", "App"),
    },
    Group {
        id: "library",
        label: || tp!("settings group", "Library"),
    },
    Group {
        id: "play",
        label: || tp!("settings group", "Games & play"),
    },
    Group {
        id: "devices",
        label: || tp!("settings group", "Devices & streaming"),
    },
    Group {
        id: "people",
        label: || tp!("settings group", "People"),
    },
    Group {
        id: "system",
        label: || tp!("settings group", "System"),
    },
];

/// The sections, by group, in the list's order (the router's
/// `SETTINGS_TABS`).
pub const SECTIONS: [Section; 17] = [
    Section {
        id: "general",
        label: || tp!("settings tab", "General"),
        icon: SETTINGS_2,
        group: "app",
    },
    Section {
        id: "appearance",
        label: || tp!("settings tab", "Appearance"),
        icon: PALETTE,
        group: "app",
    },
    Section {
        id: "plugins",
        label: || tp!("settings tab", "Plugins"),
        icon: PUZZLE,
        group: "app",
    },
    Section {
        id: "library-discovery",
        label: || tp!("settings tab", "Library Discovery"),
        icon: SCAN_SEARCH,
        group: "library",
    },
    Section {
        id: "launchers",
        label: || tp!("settings tab", "Launchers"),
        icon: ROCKET,
        group: "library",
    },
    // Admins only (#165): stores' sign-ins.
    Section {
        id: "stores",
        label: || tp!("settings tab", "Store accounts"),
        icon: STORE,
        group: "library",
    },
    Section {
        id: "storage",
        label: || tp!("settings tab", "Storage"),
        icon: HARD_DRIVE,
        group: "library",
    },
    Section {
        id: "installer",
        label: || tp!("settings tab", "Installer"),
        icon: PACKAGE_OPEN,
        group: "library",
    },
    // The id stays `emulators`: it is in saved selections and links.
    Section {
        id: "emulators",
        label: || tp!("settings tab", "Emulation"),
        icon: GAMEPAD,
        group: "play",
    },
    // Admins only (#180): global scripts run with every game.
    Section {
        id: "scripts",
        label: || tp!("settings tab", "Scripts"),
        icon: SQUARE_TERMINAL,
        group: "play",
    },
    // Every account sees its devices; only an admin changes them (#231).
    Section {
        id: "devices",
        label: || tp!("settings tab", "Devices"),
        icon: MONITOR_SMARTPHONE,
        group: "devices",
    },
    // Every account plays from other devices; hosting is an admin's (#265).
    Section {
        id: "streaming",
        label: || tp!("settings tab", "Streaming"),
        icon: CAST,
        group: "devices",
    },
    Section {
        id: "profile",
        label: || tp!("settings tab", "My profile"),
        icon: USER_ROUND,
        group: "people",
    },
    // Admins only (#208).
    Section {
        id: "accounts",
        label: || tp!("settings tab", "Accounts"),
        icon: USERS,
        group: "people",
    },
    // Admins only (#193): a backup holds every account's data.
    Section {
        id: "backup",
        label: || tp!("settings tab", "Backup"),
        icon: DATABASE_BACKUP,
        group: "system",
    },
    Section {
        id: "statistics",
        label: || tp!("settings tab", "Statistics"),
        icon: ACTIVITY,
        group: "system",
    },
    Section {
        id: "about",
        label: || tp!("settings tab", "About"),
        icon: INFO,
        group: "system",
    },
];

/// Sections every account opens whatever it was given: About, Statistics
/// (it only reads the machine, #156), My profile (#208), Devices and
/// Streaming (their panels leave changes to admins).
const EVERYONE: [&str; 5] = ["about", "statistics", "profile", "devices", "streaming"];

/// Whether the signed-in account may open `section` (#205). The core
/// refuses changes it may not make whatever this shows.
pub fn opens(editable: &Editable, section: &str) -> bool {
    EVERYONE.contains(&section) || editable.allows(section)
}

/// The section `id` (a known one).
fn section(id: &str) -> &'static Section {
    SECTIONS
        .iter()
        .find(|section| section.id == id)
        .unwrap_or(&SECTIONS[0])
}

/// Settings at `tab` (and its sub-section).
#[component]
pub fn SettingsView(tab: &'static str, section: Option<String>) -> Element {
    rsx! {
        UnsavedChangesGuard {
            SettingsContent { asked: tab, sub: section }
        }
    }
}

#[component]
fn SettingsContent(asked: &'static str, sub: Option<String>) -> Element {
    let router = use_router();
    // Every account may change its own sections; until that's known,
    // every section is listed (as without accounts).
    let editable = use_settings_query(Key::Settings(String::new()), |settings| async move {
        settings.editable().await
    });
    let editable = match &*editable.read() {
        Some(Ok(editable)) => editable.clone(),
        _ => Editable::All,
    };
    let shown: Vec<&'static str> = SECTIONS
        .iter()
        .map(|section| section.id)
        .filter(|id| opens(&editable, id))
        .collect();
    // A section this account doesn't have (another account's remembered
    // one, or a link into it) opens the first it has (#50).
    let tab = if shown.contains(&asked) {
        asked
    } else {
        shown.first().copied().unwrap_or("about")
    };
    let go = move |next: &'static str, sub: Option<String>| {
        router.replace(
            &Route::Settings {
                tab: next,
                section: sub,
            }
            .href(),
        );
    };
    use_effect(use_reactive!(|tab, asked| {
        if tab != asked {
            router.replace(&Route::Settings { tab, section: None }.href());
        }
    }));
    let found = use_context_provider(|| Found(Signal::new(None)));
    let mut query = use_signal(String::new);
    // The search field, for typing and a pad's Search (#56).
    let mut search_box = use_signal(|| None::<NodeHandle>);
    let input = use_input();
    let focus_search = use_callback(move |()| {
        if let Some(handle) = search_box.peek().clone()
            && let Some(field) = crate::ui::popover::start_in(&handle.doc(), handle.node_id())
        {
            input.focus(field);
        }
    });
    // Typing on Settings searches settings, not games.
    let page = try_use_context::<crate::frame::PageSearch>();
    use_hook(move || {
        if let Some(crate::frame::PageSearch(mut slot)) = page {
            slot.set(Some(Callback::new(move |text: String| {
                query.with_mut(|query| query.push_str(&text));
                focus_search(());
            })));
        }
    });
    use_drop(move || {
        if let Some(crate::frame::PageSearch(mut slot)) = page {
            slot.set(None);
        }
    });
    // A pad's Search (Y) types in Settings' search on the on-screen
    // keyboard; `/` and F3 move to it.
    use_input_action(
        Action::Search,
        Bind {
            label: Some(tp!("action", "Search")),
            ..Bind::default()
        },
        move || {
            focus_search(());
            true
        },
    );
    let searching = !query().trim().is_empty();
    // LB/RB move between the sections, in place.
    let order = shown.clone();
    let at = order.iter().position(|id| *id == tab).unwrap_or(0);
    let count = order.len().max(1);
    let step = {
        let order = order.clone();
        use_callback(move |forward: bool| {
            let next = if forward {
                (at + 1) % count
            } else {
                (at + count - 1) % count
            };
            if let Some(next) = order.get(next) {
                go(next, None);
            }
        })
    };
    for (action, forward) in [(Action::TabPrev, false), (Action::TabNext, true)] {
        use_input_action(action, Bind::default(), move || {
            step(forward);
            true
        });
    }
    let open_result = move |result: SearchResult| {
        query.set(String::new());
        let Found(mut asked) = found;
        asked.set(result.label.clone());
        go(result.tab, result.section.map(str::to_owned));
    };
    let panel_label = format!("settings-tab-{tab}");
    rsx! {
        div { "data-view": "settings", class: "flex flex-col",
            h1 { class: "font-display text-3xl font-semibold tracking-wide", {t!("Settings")} }
            p { class: "mt-1 text-fg-muted",
                {t!("How Caskade finds, installs and plays your games, and how it looks and syncs.")}
            }
            div { class: "mt-6 flex items-start gap-4 sm:gap-6",
                // Sticky beside a long panel; the list scrolls on its own
                // once the sections outgrow the window.
                div {
                    "data-focus-section": "settings-nav",
                    class: "sticky top-0 flex max-h-[calc(100vh-12rem)] w-48 shrink-0 flex-col gap-2 lg:w-60",
                    div {
                        class: "contents",
                        onmounted: move |event| search_box.set(event.data().downcast::<NodeHandle>().cloned()),
                        SettingsSearch {
                        query: query(),
                        on_query: move |next: String| query.set(next),
                        sections: shown.clone(),
                        on_open: open_result,
                        }
                    }
                    // Out of the way while search results stand in for it.
                    if !searching {
                        SectionList { shown: shown.clone(), tab, on_open: move |next: &'static str| go(next, None) }
                    }
                }
                div {
                    key: "{tab}",
                    role: "tabpanel",
                    id: "{panel_label}-panel",
                    "aria-labelledby": "{panel_label}",
                    "data-spatial": "skip",
                    class: "min-w-0 flex-1 animate-enter",
                    {panels::panel(tab, sub)}
                }
            }
        }
    }
}

/// The sections, under their groups' headings: a vertical tab list.
/// Arrows, Home and End move between sections and open them (over the
/// headings); Enter and Space and a click open one.
#[component]
fn SectionList(
    shown: Vec<&'static str>,
    tab: &'static str,
    on_open: EventHandler<&'static str>,
) -> Element {
    let input = use_input();
    let tabs = use_signal(Vec::<(&'static str, blitz_dom::NodeId)>::new);
    let groups: Vec<(&Group, Vec<&'static Section>)> = GROUPS
        .iter()
        .map(|group| {
            (
                group,
                SECTIONS
                    .iter()
                    .filter(|section| section.group == group.id && shown.contains(&section.id))
                    .collect::<Vec<&'static Section>>(),
            )
        })
        .filter(|(_, sections)| !sections.is_empty())
        .collect();
    let move_to = move |id: &'static str| {
        on_open.call(id);
        if let Some((_, node)) = tabs.peek().iter().find(|(known, _)| *known == id) {
            input.focus(*node);
        }
    };
    let keys = {
        let shown = shown.clone();
        move |event: KeyboardEvent| {
            let at = shown.iter().position(|id| *id == tab).unwrap_or(0);
            let last = shown.len().saturating_sub(1);
            let next = match event.key() {
                KeyName::Named(NamedKey::ArrowDown) => (at + 1).min(last),
                KeyName::Named(NamedKey::ArrowUp) => at.saturating_sub(1),
                KeyName::Named(NamedKey::Home) => 0,
                KeyName::Named(NamedKey::End) => last,
                _ => return,
            };
            event.prevent_default();
            event.stop_propagation();
            if let Some(id) = shown.get(next) {
                move_to(id);
            }
        }
    };
    rsx! {
        div {
            role: "tablist",
            "aria-label": t!("Settings sections"),
            "aria-orientation": "vertical",
            "data-cs": "settings-nav",
            "data-orientation": "vertical",
            class: "flex min-h-0 flex-col gap-1 overflow-x-hidden overflow-y-auto overscroll-contain p-2",
            onkeydown: keys,
            for (group, sections) in groups {
                div {
                    key: "group:{group.id}",
                    role: "presentation",
                    "data-part": "group",
                    class: "px-3 pt-3 pb-1 font-mono text-xs tracking-wider text-fg-subtle uppercase first:pt-0",
                    {(group.label)()}
                }
                for section in sections {
                    SectionTab {
                        key: "{section.id}",
                        id: section.id,
                        selected: section.id == tab,
                        tabs,
                        on_open: move |id: &'static str| on_open.call(id),
                    }
                }
            }
        }
    }
}

#[component]
fn SectionTab(
    id: &'static str,
    selected: bool,
    tabs: Signal<Vec<(&'static str, blitz_dom::NodeId)>>,
    on_open: EventHandler<&'static str>,
) -> Element {
    let interaction = crate::ui::interact::use_interaction_where(selected);
    let section = section(id);
    let open = move || on_open.call(id);
    rsx! {
        div {
            id: "settings-tab-{id}",
            role: "tab",
            tabindex: if selected { "0" } else { "-1" },
            "aria-selected": if selected { "true" } else { "false" },
            "aria-controls": "settings-tab-{id}-panel",
            "data-part": "tab",
            "data-selected": flag(selected),
            "data-hovered": interaction.hovered(),
            "data-pressed": interaction.pressed(),
            "data-focus-visible": interaction.ring(),
            // A pad starts on the open section.
            "data-focus-default": flag(selected),
            class: "flex cursor-pointer items-center gap-2 rounded-control border border-transparent px-3 py-2 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) outline-none data-focus-visible:focus-ring data-hovered:bg-surface-raised data-hovered:text-fg data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg data-selected:shadow-[0_0_16px_-6px] data-selected:shadow-accent-alt",
            onmounted: move |event| {
                if let Some(node) = event.data().downcast::<NodeHandle>().map(NodeHandle::node_id) {
                    let mut tabs = tabs;
                    tabs.write().retain(|(known, _)| *known != id);
                    tabs.write().push((id, node));
                }
            },
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onclick: move |_| open(),
            onkeydown: move |event| {
                interaction.key_down(&event, crate::ui::interact::Keys::EnterAndSpace, open);
            },
            onkeyup: move |event| interaction.key_up(&event, open),
            Svg { icon: section.icon, part: "icon", class: "size-4 shrink-0" }
            span { "data-part": "label", class: "min-w-0 break-words", {(section.label)()} }
        }
    }
}
