//! Draws a plugin's UI tree (#109) with Caskade's components: what a 0.2
//! backend's `render` returned, checked by the host. Every node is one of
//! `caskade_plugin_ui`'s; buttons and segmented controls hand their action
//! to the contribution ([`super::Drawn`]), which calls the backend's `act`.

use caskade_ui_data::plugin_ui::ui::{Direction, Gap, Node, Tone, Variant};
use dioxus_native::prelude::*;
use serde_json::Value;

use crate::ui::{Button, ButtonSize, ButtonVariant, Progress, Segment, SegmentedControl};

/// What a control asked for: its action and value.
pub type Act = (String, Value);

/// The first segmented control in `nodes`, depth first: its action, its
/// value and its options' values.
pub fn first_segmented(nodes: &[Node]) -> Option<(String, String, Vec<String>)> {
    nodes.iter().find_map(|node| match node {
        Node::Segmented {
            action,
            value,
            options,
            ..
        } => Some((
            action.clone(),
            value.clone(),
            options.iter().map(|option| option.value.clone()).collect(),
        )),
        Node::Stack { children, .. } | Node::Section { children, .. } => first_segmented(children),
        Node::List { items, .. } => first_segmented(items),
        _ => None,
    })
}

/// A tree: the nodes `render` returned, in a column.
#[component]
pub fn PluginTree(nodes: Vec<Node>, busy: bool, on_act: EventHandler<Act>) -> Element {
    rsx! {
        div { "data-cs": "plugin-ui", class: "flex flex-col gap-4",
            for (index, node) in nodes.into_iter().enumerate() {
                NodeView { key: "{index}", node, busy, on_act }
            }
        }
    }
}

fn gap(gap: Gap) -> &'static str {
    match gap {
        Gap::None => "gap-0",
        Gap::Small => "gap-2",
        Gap::Medium => "gap-4",
        Gap::Large => "gap-6",
    }
}

fn tone(tone: Tone) -> (&'static str, &'static str) {
    match tone {
        Tone::Neutral => ("neutral", "border-border bg-surface-base text-fg-muted"),
        Tone::Accent => (
            "accent",
            "border-accent-alt/40 bg-accent-alt/10 text-accent-alt",
        ),
        Tone::Success => ("success", "border-success/40 bg-success/10 text-success"),
        Tone::Warning => ("warning", "border-warning/40 bg-warning/10 text-warning"),
        Tone::Danger => ("danger", "border-danger/40 bg-danger/10 text-danger"),
    }
}

/// One node, and its children.
#[component]
fn NodeView(node: Node, busy: bool, on_act: EventHandler<Act>) -> Element {
    match node {
        Node::Text { text } => rsx! {
            span { "data-part": "text", "{text}" }
        },
        Node::Heading { text, level } => {
            if level == 3 {
                rsx! {
                    h3 { "data-part": "heading", class: "font-medium", "{text}" }
                }
            } else {
                rsx! {
                    h2 { "data-part": "heading", class: "font-display text-xl font-semibold", "{text}" }
                }
            }
        }
        Node::Paragraph { text } => rsx! {
            p { "data-part": "paragraph", "{text}" }
        },
        Node::Note { text } => rsx! {
            p { "data-part": "note", class: "text-sm text-fg-muted", "{text}" }
        },
        Node::Alert { text } => rsx! {
            p { "data-part": "alert", role: "alert", class: "text-sm text-danger", "{text}" }
        },
        Node::Stack {
            direction,
            gap: space,
            children,
        } => {
            let flow = match direction {
                Direction::Column => "flex-col",
                Direction::Row => "flex-row flex-wrap items-center",
            };
            rsx! {
                div {
                    "data-part": "stack",
                    "data-direction": if direction == Direction::Row { "row" } else { "column" },
                    class: "flex {flow} {gap(space)}",
                    for (index, child) in children.into_iter().enumerate() {
                        NodeView { key: "{index}", node: child, busy, on_act }
                    }
                }
            }
        }
        Node::Section { label, children } => rsx! {
            section {
                "data-part": "section",
                "data-focus-section": "plugin-section",
                "aria-label": "{label}",
                class: "flex flex-col gap-4",
                for (index, child) in children.into_iter().enumerate() {
                    NodeView { key: "{index}", node: child, busy, on_act }
                }
            }
        },
        Node::Stats { label, items } => rsx! {
            dl {
                "data-part": "stats",
                "aria-label": label,
                class: "grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm",
                for (index, item) in items.into_iter().enumerate() {
                    dt { key: "{index}", class: "text-fg-muted", "{item.label}" }
                    dd { class: "font-medium", "{item.value}" }
                }
            }
        },
        Node::Tile { value, label } => rsx! {
            div { "data-part": "tile", class: "flex flex-col gap-1 rounded-panel border border-border bg-surface-raised p-4",
                span { "data-part": "value", class: "font-display text-3xl font-semibold", "{value}" }
                span { "data-part": "label", class: "text-sm text-fg-muted", "{label}" }
            }
        },
        Node::Bars { label, items } => {
            let most = items
                .iter()
                .map(|item| item.value)
                .fold(0.0_f64, f64::max)
                .max(f64::MIN_POSITIVE);
            rsx! {
                ol { "data-part": "bars", "aria-label": "{label}", class: "flex flex-col gap-2",
                    for (index, item) in items.into_iter().enumerate() {
                        li { key: "{index}", class: "grid grid-cols-[minmax(0,12rem)_1fr_auto] items-center gap-3 text-sm",
                            span { class: "truncate", "{item.label}" }
                            span { "aria-hidden": "true", class: "h-2 overflow-hidden rounded-full bg-surface-base",
                                span {
                                    "data-part": "bar",
                                    class: "block h-full rounded-full bg-accent",
                                    style: "width: {(item.value / most * 100.0).clamp(0.0, 100.0)}%",
                                }
                            }
                            span { class: "text-fg-muted tabular-nums", "{item.text}" }
                        }
                    }
                }
            }
        }
        Node::Table {
            caption,
            columns,
            rows,
            row_headers,
        } => rsx! {
            table { "data-part": "table", class: "w-full border-collapse text-left text-sm",
                caption { class: "mb-2 text-left font-medium", "{caption}" }
                thead {
                    tr {
                        for (index, column) in columns.into_iter().enumerate() {
                            th { key: "{index}", scope: "col", class: "border-b border-border px-2 py-1 font-medium text-fg-muted", "{column}" }
                        }
                    }
                }
                tbody {
                    for (index, row) in rows.into_iter().enumerate() {
                        tr { key: "{index}",
                            for (cell_index, cell) in row.into_iter().enumerate() {
                                if row_headers && cell_index == 0 {
                                    th { key: "{cell_index}", scope: "row", class: "border-b border-border px-2 py-1 font-normal", "{cell}" }
                                } else {
                                    td { key: "{cell_index}", class: "border-b border-border px-2 py-1", "{cell}" }
                                }
                            }
                        }
                    }
                }
            }
        },
        Node::List { ordered, items } => {
            let children = rsx! {
                for (index, item) in items.into_iter().enumerate() {
                    li { key: "{index}",
                        NodeView { node: item, busy, on_act }
                    }
                }
            };
            if ordered {
                rsx! {
                    ol { "data-part": "list", class: "list-decimal pl-5", {children} }
                }
            } else {
                rsx! {
                    ul { "data-part": "list", class: "list-disc pl-5", {children} }
                }
            }
        }
        Node::Badge { text, tone: kind } => {
            let (name, classes) = tone(kind);
            rsx! {
                span {
                    "data-part": "badge",
                    "data-tone": name,
                    class: "inline-flex w-fit items-center rounded-control border px-2 py-0.5 text-xs font-medium {classes}",
                    "{text}"
                }
            }
        }
        Node::Progress { label, value, max } => rsx! {
            Progress {
                "data-part": "progress",
                value: Some((value / max).clamp(0.0, 1.0)),
                label: Some(label),
            }
        },
        Node::Button {
            label,
            action,
            value,
            variant,
        } => rsx! {
            Button {
                "data-part": "button",
                "data-spatial": "focusable",
                size: ButtonSize::Sm,
                class: "w-fit",
                variant: if variant == Variant::Primary { ButtonVariant::Primary } else { ButtonVariant::Secondary },
                disabled: busy,
                on_press: move |_| on_act.call((action.clone(), value.clone())),
                "{label}"
            }
        },
        Node::Segmented {
            label,
            action,
            value,
            options,
        } => rsx! {
            SegmentedControl {
                "data-part": "segmented",
                "aria-label": "{label}",
                class: "w-fit",
                options: options
                    .iter()
                    .map(|option| Segment::new(option.value.clone(), option.label.clone()))
                    .collect::<Vec<_>>(),
                value,
                disabled: busy,
                on_change: move |chosen: String| on_act.call((action.clone(), Value::String(chosen))),
            }
        },
    }
}
