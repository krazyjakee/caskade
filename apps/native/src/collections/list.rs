//! Every collection (the webview's `views/CollectionsView.tsx`): a header
//! with their counts and New Collection, All · Smart · Manual tabs (LB and
//! RB step through them), and a card per collection, pinned ones first,
//! each with its menu at a right click, the menu key or a pad's Start.

use super::actions::{CollectionActions, use_collection_actions};
use super::card::CollectionCard;
use super::{PIN, PlainButton};
use crate::data::{Query, use_query};
use crate::input::{Bind, use_input_action};
use crate::library::collection_dialog::CollectionDialog;
use crate::router::{CollectionFilter, Route, use_router};
use crate::ui::{Anchor, Button, Icon, Link, Menu, Svg, icons};
use caskade_core::model::{CollectionKind, CollectionSummary, GameFilter, GameSort, ListGames};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tp};
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

/// The tabs, in order.
const FILTERS: [CollectionFilter; 3] = [
    CollectionFilter::All,
    CollectionFilter::Smart,
    CollectionFilter::Manual,
];

/// A tab's name.
fn filter_label(filter: CollectionFilter) -> String {
    match filter {
        CollectionFilter::All => t!("All Collections"),
        CollectionFilter::Smart => tp!(
            note = "Collections that save a library filter and keep themselves up to date",
            "collection type",
            "Smart"
        ),
        CollectionFilter::Manual => tp!(
            note = "Collections whose games are picked by hand",
            "collection type",
            "Manual"
        ),
    }
}

/// Whether `collection` shows under `filter`.
fn matches(filter: CollectionFilter, collection: &CollectionSummary) -> bool {
    match filter {
        CollectionFilter::All => true,
        CollectionFilter::Smart => matches!(collection.kind, CollectionKind::Smart { .. }),
        CollectionFilter::Manual => matches!(collection.kind, CollectionKind::Manual),
    }
}

/// The signed-in account's collections, pinned first, then by name (the
/// core's order).
pub fn use_collections() -> Query<Vec<CollectionSummary>> {
    use_query(DataKey::Collections, |library| {
        let _ = super::revision();
        async move { library.collections().await }
    })
}

#[component]
pub fn CollectionsView(filter: CollectionFilter) -> Element {
    let router = use_router();
    let mut collections = use_collections();
    let mut creating = use_signal(|| false);
    let actions = use_collection_actions();
    // LB and RB step through the tabs, in place.
    for (action, by) in [(Action::TabPrev, FILTERS.len() - 1), (Action::TabNext, 1)] {
        use_input_action(
            action,
            Bind {
                label: Some(if by == 1 {
                    t!("Next tab")
                } else {
                    t!("Previous tab")
                }),
                ..Bind::default()
            },
            move || {
                let at = FILTERS.iter().position(|f| *f == filter).unwrap_or(0);
                let next = FILTERS[(at + by) % FILTERS.len()];
                router.replace(&Route::Collections { filter: next }.href());
                true
            },
        );
    }

    let (all, failure, loading) = match &*collections.read() {
        Some(Ok(list)) => (list.clone(), None, false),
        Some(Err(report)) => (Vec::new(), Some(report.summary.clone()), false),
        None => (Vec::new(), None, true),
    };
    let count = |filter: CollectionFilter| all.iter().filter(|c| matches(filter, c)).count();
    let stats: [(&str, Icon, usize, String); 4] = [
        ("all", icons::SQUARE_LIBRARY, all.len(), t!("Collections")),
        (
            "smart",
            icons::SPARKLES,
            count(CollectionFilter::Smart),
            filter_label(CollectionFilter::Smart),
        ),
        (
            "manual",
            icons::LIST_CHECKS,
            count(CollectionFilter::Manual),
            filter_label(CollectionFilter::Manual),
        ),
        (
            "pinned",
            PIN,
            all.iter().filter(|c| c.pinned).count(),
            tp!("collection stats", "Pinned"),
        ),
    ];
    let tabs: Vec<(CollectionFilter, String, usize)> = FILTERS
        .into_iter()
        .map(|f| (f, filter_label(f), count(f)))
        .collect();
    let visible: Vec<CollectionSummary> =
        all.iter().filter(|c| matches(filter, c)).cloned().collect();
    let first = all.is_empty();
    let shown = visible.len();
    // In the Smart tab, a new one starts smart.
    let preset = (filter == CollectionFilter::Smart).then(|| CollectionKind::Smart {
        query: ListGames {
            filter: GameFilter::Installed,
            sort: GameSort::Title,
            ..ListGames::default()
        },
    });
    rsx! {
        div { "data-view": "collections",
            section {
                "data-cs": "panel",
                class: "flex flex-wrap items-end justify-between gap-6 rounded-panel border border-border bg-surface-raised p-8",
                div {
                    div { class: "flex items-center gap-2 font-mono text-xs tracking-[0.2em] text-accent-alt uppercase",
                        Svg { icon: icons::SQUARE_LIBRARY, class: "size-4" }
                        " "
                        {t!("Collections")}
                    }
                    h1 { class: "mt-2 font-display text-4xl font-semibold tracking-wide",
                        {t!("Your Collections")}
                    }
                    p { class: "mt-2 text-fg-muted",
                        {t!("Hand-pick games into lists, or save a filter as a smart collection that keeps itself up to date.")}
                    }
                    div { class: "mt-6 flex flex-wrap gap-3",
                        for (i, (id, icon, value, label)) in stats.into_iter().enumerate() {
                            div {
                                key: "{id}",
                                style: "--i: {i};",
                                class: "chamfer flex animate-enter items-center gap-3 bg-surface-overlay px-4 py-3",
                                Svg { icon, class: "size-6 text-accent-alt" }
                                div {
                                    div { class: "font-mono text-lg font-semibold", "{value}" }
                                    div { class: "text-xs text-fg-muted", "{label}" }
                                }
                            }
                        }
                    }
                }
                div { "data-part": "primary-action", class: "animate-enter [--i:4]",
                    PlainButton {
                        on_press: move |_| creating.set(true),
                        class: "chamfer relative flex items-center gap-2 bg-linear-to-r from-accent to-accent-strong px-6 py-2.5 font-display font-semibold tracking-wider text-accent-fg uppercase data-hovered:brightness-110 data-focus-visible:focus-ring-chamfer",
                        Svg { icon: icons::PLUS, class: "size-4" }
                        {t!("New Collection")}
                    }
                }
            }

            div { role: "tablist", class: "mt-6 mb-4 flex flex-wrap gap-2",
                for (tab, label, count) in tabs {
                    {
                        let selected = tab == filter;
                        let href = Route::Collections { filter: tab }.href();
                        rsx! {
                            Link {
                                key: "{label}",
                                href: href.clone(),
                                role: "tab",
                                "aria-selected": if selected { "true" } else { "false" },
                                "data-selected": selected.then_some("true"),
                                class: "rounded-full border border-border bg-surface-raised px-4 py-1.5 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) data-hovered:text-fg data-focus-visible:focus-ring data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg data-selected:shadow-[0_0_16px_-6px] data-selected:shadow-accent-alt",
                                on_press: move |_| router.replace(&href),
                                "{label} "
                                span { class: "font-mono text-xs text-fg-subtle", "{count}" }
                            }
                        }
                    }
                }
            }

            if let Some(summary) = failure {
                div { role: "alert", class: "rounded-panel border border-border p-8",
                    p { {t!("Could not load your collections.")} }
                    p { class: "mt-2 text-sm text-fg-muted", "{summary}" }
                    Button { class: "mt-4", on_press: move |_| collections.restart(), {t!("Try again")} }
                }
            } else if loading {
                p { role: "status", class: "text-fg-muted", {t!("Loading collections…")} }
            } else {
                div { class: "grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-4",
                    for (index, collection) in visible.into_iter().enumerate() {
                        CollectionTile { key: "{collection.id}", collection, index, actions }
                    }
                    PlainButton {
                        "data-cs": "new-collection",
                        on_press: move |_| creating.set(true),
                        style: "--i: {shown};",
                        class: "flex min-h-48 animate-enter flex-col items-center justify-center gap-2 rounded-card border border-dashed border-border-strong text-fg-subtle transition-colors duration-(--cs-motion-duration-fast) data-hovered:border-accent-alt/50 data-hovered:bg-surface-raised data-hovered:text-accent-alt data-focus-visible:focus-ring",
                        Svg { icon: icons::PLUS, class: "size-6" }
                        span { class: "text-sm font-medium", {t!("New Collection")} }
                        span { class: "text-xs",
                            if first {
                                {t!("Make your first one")}
                            } else {
                                {t!("Manual list or smart filter")}
                            }
                        }
                    }
                }
            }
            if let Some(message) = actions.error() {
                p { role: "alert", class: "mt-4 text-sm text-danger", "{message}" }
            }
            if creating() {
                CollectionDialog {
                    preset,
                    on_close: move |_| creating.set(false),
                    on_saved: move |id: String| {
                        super::changed();
                        router.navigate(&Route::Collection { id }.href());
                    },
                }
            }
            {actions.dialogs(None)}
        }
    }
}

/// A collection's card, with its menu at a right click, the menu key,
/// Shift+F10 or a pad's Start.
#[component]
fn CollectionTile(
    collection: CollectionSummary,
    index: usize,
    actions: CollectionActions,
) -> Element {
    let mut me = use_signal(|| None::<NodeHandle>);
    let menu = use_signal(|| None::<Anchor>);
    let open_at = move |anchor: Option<Anchor>| {
        let mut menu = menu;
        let anchor = anchor.or_else(|| me.peek().as_ref().and_then(Anchor::of));
        if let Some(anchor) = anchor {
            menu.set(Some(anchor));
        }
    };
    use_input_action(
        Action::Menu,
        Bind {
            owner: me.read().as_ref().map(NodeHandle::node_id),
            label: Some(t!(
                note = "Button hint: a pad's Start opens the focused item's menu",
                "Options"
            )),
            ..Bind::default()
        },
        move || {
            open_at(None);
            true
        },
    );
    let items = actions.items(&collection);
    let name = collection.name.clone();
    let target = collection.clone();
    rsx! {
        div {
            onmounted: move |event| me.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                event.stop_propagation();
                open_at(Some(crate::frame::pointer_anchor(me.peek().as_ref(), &event)));
            },
            onkeydown: move |event: KeyboardEvent| {
                let shift_f10 = event.key() == Key::Named(NamedKey::F10) && event.modifiers().shift();
                if event.key() == Key::Named(NamedKey::ContextMenu) || shift_f10 {
                    event.prevent_default();
                    event.stop_propagation();
                    open_at(None);
                }
            },
            CollectionCard { collection, index }
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items,
                    label: name,
                    return_to: me.peek().as_ref().map(NodeHandle::node_id),
                    on_action: move |id: String| actions.run(&id, &target),
                    on_close: move |_| {
                        let mut menu = menu;
                        menu.set(None);
                    },
                }
            }
        }
    }
}
