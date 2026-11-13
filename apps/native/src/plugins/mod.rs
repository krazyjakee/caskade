//! Plugins' UI in the native app (#109). From plugin API 0.2 a plugin's
//! backend draws its slots and views as Caskade's components
//! (`caskade_plugin_ui`), and [`tree`] draws them here. This module puts
//! them where the webview's `plugins/` did: views and their sidebar entries
//! ([`PluginViewPage`], [`PluginNav`]), Home's sections ([`HomeSections`]),
//! the game page's actions, tabs and panels ([`GameDetails`]), a game's
//! menu ([`game_actions`], [`run_action`]), library cards' badges and
//! overlays ([`CardExtras`], [`RowBadges`]), the top bar ([`TopbarItems`])
//! and plugins' own settings pages ([`SettingsPages`]). A contribution that
//! fails to draw shows its plugin's placeholder. 0.1 plugins' JavaScript UI
//! halves don't run here.

mod anatomy;
#[cfg(test)]
mod tests;
mod tree;

pub use anatomy::ANATOMY;

use caskade_ui_data::ErrorReport;
use caskade_ui_data::plugin_ui::ui::{Context, Mode, Node};
use caskade_ui_data::plugin_ui::{Contribution, PluginUi};
use caskade_ui_i18n::t;
use caskade_ui_input::Action;
use dioxus_native::prelude::dioxus_core::spawn_forever;
use dioxus_native::prelude::*;
use serde_json::Value;

use crate::data::{change, use_plugin_contributions, use_plugin_ui};
use crate::router::{Route, RouteLink};
use crate::ui::{Button, Svg, Tab, Tabs, icons, use_toasts};
use tree::{Act, PluginTree};

/// The mode plugins draw for: the window's (#107).
pub fn mode() -> Mode {
    if crate::console::active() {
        Mode::Console
    } else {
        Mode::Desktop
    }
}

/// What enabled plugins draw, loaded once for the whole frame.
#[derive(Clone, Copy)]
struct Contributions(crate::data::Query<Vec<Contribution>>);

/// Loads what plugins draw for `children`: every slot reads it from here,
/// so a grid of cards doesn't ask once per card.
#[component]
pub fn PluginRoot(children: Element) -> Element {
    let query = use_plugin_contributions();
    use_context_provider(|| Contributions(query));
    children
}

/// Every contribution enabled plugins draw; none outside [`PluginRoot`]
/// (the component gallery) or while they load.
pub fn use_contributions() -> Vec<Contribution> {
    use_loaded().unwrap_or_default()
}

/// [`use_contributions`], `None` while they first load.
fn use_loaded() -> Option<Vec<Contribution>> {
    let Contributions(query) = try_use_context::<Contributions>()?;
    match &*query.read() {
        Some(Ok(all)) => Some(all.clone()),
        Some(Err(_)) => Some(Vec::new()),
        None => None,
    }
}

/// The contributions to `slot` that show in this mode.
pub fn use_slot(slot: &'static str) -> Vec<Contribution> {
    use_contributions()
        .into_iter()
        .filter(|item| item.slot == slot && item.shown_in(mode()))
        .collect()
}

fn title(contribution: &Contribution) -> String {
    contribution
        .title
        .clone()
        .unwrap_or_else(|| contribution.plugin_name.clone())
}

/// Runs a `game.actions` or `commands` entry: `act` with `run`. Its notice
/// shows as a toast; a failure says which action failed and why.
pub fn run_action(
    handle: PluginUi,
    contribution: Contribution,
    game_id: Option<String>,
    toasts: crate::ui::Toasts,
) {
    let name = title(&contribution);
    // Beyond the caller: a menu closes as its action starts.
    spawn_forever(async move {
        let context = Context {
            mode: mode(),
            game_id,
            game_title: None,
            state: Value::Null,
        };
        let done: Result<_, ErrorReport> = change(async move {
            handle
                .act(
                    &contribution.plugin,
                    &contribution.slot,
                    &contribution.id,
                    "run",
                    Value::Null,
                    context,
                )
                .await
                .map(|outcome| (contribution, outcome))
        })
        .await;
        match done {
            Ok((contribution, outcome)) => {
                if let Some(notice) = outcome.notice {
                    toasts.show(notice_text(&contribution, &notice), false, None);
                }
            }
            Err(error) => {
                toasts.show(
                    t!(
                        "{action} failed: {error}",
                        action = name,
                        error = error.summary
                    ),
                    true,
                    None,
                );
            }
        }
    });
}

fn notice_text(contribution: &Contribution, notice: &str) -> String {
    t!(
        note = "A plugin's message: the plugin's name, then what it said",
        "{plugin}: {message}",
        plugin = contribution.plugin_name.clone(),
        message = notice.to_owned()
    )
}

/// A game's plugin actions, for its menu: `plugin:<plugin>/<id>` items.
pub fn game_actions(contributions: &[Contribution]) -> Vec<crate::ui::MenuItem> {
    contributions
        .iter()
        .map(|item| {
            crate::ui::MenuItem::new(format!("plugin:{}/{}", item.plugin, item.id), title(item))
        })
        .collect()
}

/// One contribution, drawn by its plugin: the tree `render` returned, its
/// state kept between draws, and what the user does with it sent to `act`.
/// Another contribution or game starts afresh, with no state.
#[component]
pub fn Drawn(contribution: Contribution, #[props(default)] game_id: Option<String>) -> Element {
    let key = format!(
        "{}/{}/{}/{}",
        contribution.plugin,
        contribution.slot,
        contribution.id,
        game_id.as_deref().unwrap_or("")
    );
    rsx! {
        for key in std::iter::once(key) {
            DrawnOne { key: "{key}", contribution: contribution.clone(), game_id: game_id.clone() }
        }
    }
}

#[component]
fn DrawnOne(contribution: Contribution, game_id: Option<String>) -> Element {
    let handle = use_plugin_ui();
    let toasts = use_toasts();
    let mut state = use_signal(|| Value::Null);
    let mut busy = use_signal(|| false);
    let tree = {
        let handle = handle.clone();
        let contribution = contribution.clone();
        let game_id = game_id.clone();
        use_resource(move || {
            // A language change asks for the drawing again (#141).
            let _ = crate::i18n::LANGUAGE.read();
            let handle = handle.clone();
            let contribution = contribution.clone();
            let context = Context {
                mode: mode(),
                game_id: game_id.clone(),
                game_title: None,
                state: state(),
            };
            change(async move {
                handle
                    .render(
                        &contribution.plugin,
                        &contribution.slot,
                        &contribution.id,
                        context,
                    )
                    .await
            })
        })
    };
    let act = {
        let contribution = contribution.clone();
        let game_id = game_id.clone();
        let mut tree = tree;
        use_callback(move |(action, value): Act| {
            let handle = handle.clone();
            let contribution = contribution.clone();
            let context = Context {
                mode: mode(),
                game_id: game_id.clone(),
                game_title: None,
                state: state.peek().clone(),
            };
            busy.set(true);
            spawn(async move {
                let label = contribution.clone();
                let outcome: Result<_, ErrorReport> = change(async move {
                    handle
                        .act(
                            &contribution.plugin,
                            &contribution.slot,
                            &contribution.id,
                            &action,
                            value,
                            context,
                        )
                        .await
                })
                .await;
                busy.set(false);
                match outcome {
                    Ok(outcome) => {
                        if let Some(notice) = outcome.notice {
                            toasts.show(notice_text(&label, &notice), false, None);
                        }
                        match outcome.state {
                            Some(next) => state.set(next),
                            None => tree.restart(),
                        }
                    }
                    Err(error) => {
                        toasts.show(notice_text(&label, &error.summary), true, None);
                        tree.restart();
                    }
                }
            });
        })
    };
    // In a view, a pad's LB and RB move its first segmented control (#152).
    let segmented = match &*tree.read() {
        Some(Ok(nodes)) if contribution.slot == "views" => tree::first_segmented(nodes),
        _ => None,
    };
    let step = move |segmented: &Option<(String, String, Vec<String>)>, by: usize| {
        let Some((action, value, options)) = segmented else {
            return false;
        };
        if options.len() < 2 {
            return false;
        }
        let at = options
            .iter()
            .position(|option| option == value)
            .unwrap_or(0);
        let next = options[(at + by) % options.len()].clone();
        act((action.clone(), Value::String(next)));
        true
    };
    crate::input::use_input_action(Action::TabPrev, crate::input::Bind::default(), {
        let segmented = segmented.clone();
        move || {
            step(
                &segmented,
                segmented.as_ref().map_or(0, |(_, _, all)| all.len() - 1),
            )
        }
    });
    crate::input::use_input_action(Action::TabNext, crate::input::Bind::default(), {
        let segmented = segmented.clone();
        move || step(&segmented, 1)
    });
    let read: Option<Result<Vec<Node>, ErrorReport>> = tree.read().clone();
    match read {
        None => rsx! {
            p {
                role: "status",
                "data-part": "plugin-loading",
                class: "text-sm text-fg-muted",
                {t!("Loading…")}
            }
        },
        Some(Ok(nodes)) => rsx! {
            div { "data-plugin": "{contribution.plugin}", class: "contents",
                PluginTree { nodes, busy: busy(), on_act: act }
            }
        },
        Some(Err(error)) => {
            tracing::warn!(
                plugin = %contribution.plugin,
                slot = %contribution.slot,
                id = %contribution.id,
                error = %error.summary,
                "a plugin couldn't draw a contribution"
            );
            rsx! {
                Placeholder { contribution }
            }
        }
    }
}

/// What shows in place of a contribution that failed to draw, as the
/// webview's `PluginBoundary`: it holds focus, so a pad can move on.
#[component]
fn Placeholder(contribution: Contribution) -> Element {
    rsx! {
        div {
            role: "alert",
            tabindex: "-1",
            "data-cs": "plugin-error",
            "data-plugin": "{contribution.plugin}",
            "data-spatial": "focusable",
            class: "rounded-control border border-dashed border-border-strong px-3 py-2 text-xs text-fg-muted",
            {t!("{name} couldn't show this part.", name = contribution.plugin_name.clone())}
        }
    }
}

/// `/plugins/<plugin>/<view>`: a plugin's full-page view.
#[component]
pub fn PluginViewPage(plugin: String, view: String) -> Element {
    let Some(all) = use_loaded() else {
        return rsx! {};
    };
    let Some(found) = all
        .iter()
        .find(|item| item.slot == "views" && item.plugin == plugin && item.id == view)
        .cloned()
    else {
        return rsx! {
            p { role: "status", class: "py-12 text-fg-muted",
                {t!("This plugin view isn't available.")}
            }
        };
    };
    let name = title(&found);
    // A link into a view this mode hides (#152).
    if !found.shown_in(mode()) {
        return rsx! {
            p { role: "status", class: "py-12 text-fg-muted",
                {t!("{title} isn't available in desktop mode.", title = name)}
            }
        };
    }
    rsx! {
        section { "data-view": "plugin", "data-plugin": "{plugin}", class: "mt-4",
            h1 { class: "mb-4 font-display text-2xl font-semibold", "{name}" }
            Drawn { key: "{plugin}/{view}", contribution: found }
        }
    }
}

/// The sidebar's links to plugin views, then plugins' own sidebar items,
/// styled like the sidebar's own links.
#[component]
pub fn PluginNav(item_class: &'static str) -> Element {
    let views = use_slot("views");
    let items = use_slot("sidebar.items");
    if views.is_empty() && items.is_empty() {
        return rsx! {};
    }
    rsx! {
        nav {
            "aria-label": t!(note = "The sidebar's list of plugin views", "Plugins"),
            "data-part": "plugin-nav",
            class: "flex flex-col gap-1",
            for view in views {
                RouteLink {
                    key: "{view.plugin}/{view.id}",
                    to: Route::Plugin {
                        plugin: view.plugin.clone(),
                        view: view.id.clone(),
                    }
                    .href(),
                    class: item_class,
                    "data-part": "nav-item",
                    Svg { icon: icons::PUZZLE, part: "icon", class: "size-5 shrink-0" }
                    span { "data-part": "label", class: "flex-1", "{title(&view)}" }
                }
            }
            for item in items {
                Drawn { key: "{item.plugin}/{item.id}", contribution: item }
            }
        }
    }
}

/// Home's sections from plugins: each with its title.
#[component]
pub fn HomeSections() -> Element {
    let sections = use_slot("home.sections");
    rsx! {
        for section in sections {
            section {
                key: "{section.plugin}/{section.id}",
                "data-part": "plugin-section",
                "data-plugin": "{section.plugin}",
                "aria-label": "{title(&section)}",
                class: "mt-8",
                h2 { class: "mb-4 font-display text-xl font-semibold", "{title(&section)}" }
                Drawn { contribution: section.clone() }
            }
        }
    }
}

/// The game page's plugin actions, tabs and panels; nothing without any.
#[component]
pub fn GameDetails(game_id: String) -> Element {
    let handle = use_plugin_ui();
    let toasts = use_toasts();
    let actions = use_slot("game.actions");
    let tabs = use_slot("game.details.tabs");
    let panels = use_slot("game.details.panels");
    let mut chosen = use_signal(|| None::<String>);
    if actions.is_empty() && tabs.is_empty() && panels.is_empty() {
        return rsx! {};
    }
    let ids: Vec<String> = tabs
        .iter()
        .map(|tab| format!("{}/{}", tab.plugin, tab.id))
        .collect();
    let selected = chosen()
        .filter(|id| ids.contains(id))
        .or_else(|| ids.first().cloned())
        .unwrap_or_default();
    let shown = tabs
        .iter()
        .find(|tab| format!("{}/{}", tab.plugin, tab.id) == selected)
        .cloned();
    rsx! {
        section {
            "data-part": "plugin-details",
            "aria-label": t!("From plugins"),
            class: "mt-8 flex flex-col gap-4",
            if !actions.is_empty() {
                div { "data-part": "plugin-actions", class: "flex flex-wrap gap-2",
                    for action in actions {
                        Button {
                            key: "{action.plugin}/{action.id}",
                            "data-plugin": "{action.plugin}",
                            "data-spatial": "focusable",
                            on_press: {
                                let handle = handle.clone();
                                let action = action.clone();
                                let game_id = game_id.clone();
                                move |_| run_action(handle.clone(), action.clone(), Some(game_id.clone()), toasts)
                            },
                            "{title(&action)}"
                        }
                    }
                }
            }
            if !tabs.is_empty() {
                div { "data-part": "plugin-tabs",
                    Tabs {
                        label: t!("Plugin tabs"),
                        tabs: tabs
                            .iter()
                            .map(|tab| Tab::new(format!("{}/{}", tab.plugin, tab.id), title(tab)))
                            .collect::<Vec<_>>(),
                        selected: selected.clone(),
                        on_change: move |id: String| chosen.set(Some(id)),
                        if let Some(tab) = shown {
                            div { class: "pt-4",
                                Drawn {
                                    key: "{tab.plugin}/{tab.id}",
                                    contribution: tab.clone(),
                                    game_id: Some(game_id.clone()),
                                }
                            }
                        }
                    }
                }
            }
            for panel in panels {
                Drawn {
                    key: "{panel.plugin}/{panel.id}",
                    contribution: panel.clone(),
                    game_id: Some(game_id.clone()),
                }
            }
        }
    }
}

/// A library card's plugin badges and overlays; nothing without any.
#[component]
pub fn CardExtras(game_id: String) -> Element {
    let badges = use_slot("game.card.badges");
    let overlays = use_slot("game.card.overlay");
    rsx! {
        if !badges.is_empty() {
            div { "data-part": "plugin-badges", class: "absolute bottom-2 left-2 flex gap-1",
                for badge in badges {
                    Drawn {
                        key: "{badge.plugin}/{badge.id}",
                        contribution: badge.clone(),
                        game_id: Some(game_id.clone()),
                    }
                }
            }
        }
        for overlay in overlays {
            Drawn {
                key: "{overlay.plugin}/{overlay.id}",
                contribution: overlay.clone(),
                game_id: Some(game_id.clone()),
            }
        }
    }
}

/// A library row's plugin badges, inline; overlays belong on card art.
#[component]
pub fn RowBadges(game_id: String) -> Element {
    let badges = use_slot("game.card.badges");
    if badges.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { "data-part": "plugin-badges", class: "relative z-10 flex shrink-0 gap-1",
            for badge in badges {
                Drawn {
                    key: "{badge.plugin}/{badge.id}",
                    contribution: badge.clone(),
                    game_id: Some(game_id.clone()),
                }
            }
        }
    }
}

/// Plugins' items in the top bar, before Activity.
#[component]
pub fn TopbarItems() -> Element {
    let items = use_slot("topbar.items");
    rsx! {
        for item in items {
            Drawn { key: "{item.plugin}/{item.id}", contribution: item.clone() }
        }
    }
}

/// A plugin's own `settings.pages`, inside its settings section: titled
/// when it has more than one.
#[component]
pub fn SettingsPages(plugin: String) -> Element {
    let pages: Vec<Contribution> = use_slot("settings.pages")
        .into_iter()
        .filter(|page| page.plugin == plugin)
        .collect();
    let titled = pages.len() > 1;
    rsx! {
        for page in pages {
            div { key: "{page.id}", "data-part": "plugin-page",
                if titled {
                    h3 { class: "mb-2 font-medium", "{title(&page)}" }
                }
                Drawn { contribution: page.clone() }
            }
        }
    }
}
