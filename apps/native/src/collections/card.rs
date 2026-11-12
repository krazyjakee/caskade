//! A collection's card, and the pieces its page shares with it (the
//! webview's `components/CollectionCard.tsx`): the smart or manual badge,
//! the mosaic of its first covers, and when it last changed.

use super::PIN;
use crate::router::{Route, use_router};
use crate::ui::{Link, Svg, icons};
use caskade_core::model::{CollectionKind, CollectionSummary};
use caskade_ui_i18n::{format, t, tp};
use dioxus_native::prelude::*;

/// Smart or manual, as its badge says.
#[component]
pub fn KindBadge(smart: bool) -> Element {
    let (kind, label, icon, class) = if smart {
        (
            "smart",
            tp!("collection kind", "Smart"),
            icons::SPARKLES,
            "border-accent-alt/40 text-accent-alt",
        )
    } else {
        (
            "manual",
            tp!("collection kind", "Manual"),
            icons::LIST_CHECKS,
            "border-border-strong text-fg",
        )
    };
    rsx! {
        span {
            "data-part": "badge",
            "data-kind": kind,
            class: "inline-flex items-center gap-1 rounded-full border bg-surface-base/70 px-2 py-0.5 text-[11px] font-medium backdrop-blur {class}",
            Svg { icon, class: "size-3" }
            "{label}"
        }
    }
}

/// The covers a collection's mosaic shows, as the art cache serves them.
pub fn cover_urls(collection: &CollectionSummary) -> Vec<String> {
    collection
        .covers
        .iter()
        .filter_map(|art| crate::library::art_url_card(Some(art)))
        .collect()
}

/// 2×2 mosaic of the collection's first covers, repeating when it has
/// fewer than four.
#[component]
pub fn CollectionMosaic(covers: Vec<String>, #[props(default)] class: String) -> Element {
    rsx! {
        div {
            "data-part": "art",
            class: "grid grid-cols-2 grid-rows-2 gap-px overflow-hidden bg-surface-base {class}",
            if covers.is_empty() {
                div { class: "col-span-2 row-span-2 grid place-items-center bg-linear-150 from-surface-overlay to-surface-base",
                    Svg { icon: icons::GAMEPAD_2, class: "size-8 text-fg-subtle" }
                }
            } else {
                for cell in 0..4usize {
                    img {
                        key: "{cell}",
                        src: covers[cell % covers.len()].clone(),
                        alt: "",
                        draggable: "false",
                        class: "size-full min-h-0 object-cover",
                    }
                }
            }
        }
    }
}

/// "Today", "3 days ago", or the day, for a time in ms; `standalone`
/// false keeps the language's case for inside a sentence.
fn relative_day(ms: i64, standalone: bool) -> String {
    use chrono::TimeZone;
    let at = crate::library::local_date_time(ms);
    let days = chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|time| {
            (chrono::Local::now().date_naive() - time.date_naive())
                .num_days()
                .max(0) as u64
        })
        .unwrap_or(u64::MAX);
    format::relative_day(at, days, standalone)
}

/// "Updated today", "Updated 3 days ago"; a smart collection follows the
/// library, so it is "Live". Empty when a manual one has no date.
pub fn collection_updated(collection: &CollectionSummary) -> String {
    if matches!(collection.kind, CollectionKind::Smart { .. }) {
        return tp!(
            note = "A smart collection, which follows the library as it changes",
            "collection",
            "Live"
        );
    }
    if collection.updated_at_ms <= 0 {
        return String::new();
    }
    t!(
        note = "{when} is a day: “today”, “3 days ago”",
        "Updated {when}",
        when = relative_day(collection.updated_at_ms, false)
    )
}

/// A collection's tile: a link to its page.
#[component]
pub fn CollectionCard(collection: CollectionSummary, #[props(default)] index: usize) -> Element {
    let router = use_router();
    let href = Route::Collection {
        id: collection.id.clone(),
    }
    .href();
    let smart = matches!(collection.kind, CollectionKind::Smart { .. });
    let covers = cover_urls(&collection);
    let count = format::count(collection.games as f64);
    let playtime = if collection.playtime_ms >= 1000 {
        format::duration(collection.playtime_ms / 1000)
    } else {
        "—".to_owned()
    };
    let updated = collection_updated(&collection);
    // A space keeps the line's height without a description.
    let subtitle = if collection.description.is_empty() {
        "\u{a0}".to_owned()
    } else {
        collection.description.clone()
    };
    rsx! {
        Link {
            href: href.clone(),
            on_press: move |_| router.navigate(&href),
            "data-cs": "collection-card",
            "data-kind": if smart { "smart" } else { "manual" },
            "data-focus-key": "collection-card:{collection.id}",
            style: "--i: {index};",
            class: "relative flex w-full animate-enter flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none data-hovered:bg-surface-hover data-focus-visible:focus-ring-over",
            div { class: "relative",
                CollectionMosaic { covers, class: "aspect-(--cs-card-aspect) w-full" }
                div {
                    "aria-hidden": "true",
                    class: "absolute inset-0 bg-linear-to-t from-surface-base/80 via-transparent to-transparent",
                }
                div { class: "absolute top-2 left-2",
                    KindBadge { smart }
                }
                if collection.pinned {
                    span {
                        "data-part": "pin",
                        role: "img",
                        "aria-label": tp!("collection", "Pinned"),
                        class: "absolute top-2.5 right-2.5",
                        Svg { icon: PIN, class: "size-4 rotate-45 fill-accent-alt/30 text-accent-alt drop-shadow" }
                    }
                }
                span {
                    "data-part": "count",
                    class: "absolute right-3 bottom-2 font-display text-3xl font-semibold text-white/90 drop-shadow",
                    "{count}"
                }
            }
            div { class: "flex flex-col gap-1 px-3 py-2.5",
                span { "data-part": "title", class: "truncate text-sm font-semibold", "{collection.name}" }
                span { "data-part": "subtitle", class: "truncate text-xs text-fg-subtle", "{subtitle}" }
                div {
                    "data-part": "meta",
                    class: "flex items-center justify-between font-mono text-[11px] text-fg-muted",
                    span { class: "inline-flex items-center gap-1",
                        Svg { icon: icons::CLOCK, class: "size-3.5" }
                        "{playtime}"
                    }
                    span { "{updated}" }
                }
            }
        }
    }
}
