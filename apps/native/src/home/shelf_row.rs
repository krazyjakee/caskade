//! A row of game cards that pages with LT/RT (the webview's
//! `components/ShelfRow.tsx`): a focus section, so focus comes back to the
//! card it left, scrolling sideways when there are more cards than fit.

use blitz_dom::NodeId;
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use crate::input::{Bind, use_input, use_input_action};
use crate::library::{GRID_GAP, MIN_CARD};
use crate::ui::icons::{Icon, Svg};
use caskade_ui_i18n::t;

/// How many cards fit across `width`, as the library's grid has it.
pub fn columns(width: f64) -> usize {
    (((width + GRID_GAP) / (MIN_CARD + GRID_GAP)).floor() as usize).max(1)
}

/// The card `by` pages from the focused one among `count` cards, `at`
/// being the focused one's place and `visible` how many fit.
pub fn page_target(at: usize, count: usize, visible: usize, by: isize) -> usize {
    let step = visible.max(1) as isize * by;
    (at as isize + step).clamp(0, count.saturating_sub(1) as isize) as usize
}

#[component]
pub fn ShelfRow(
    /// Its focus section's name: `shelf-{id}`.
    id: String,
    title: String,
    icon: Icon,
    /// The "View all" link.
    #[props(default)]
    view_all: Option<Element>,
    /// Loading, error and empty states, above the row.
    #[props(default)]
    notice: Option<Element>,
    children: Element,
) -> Element {
    let input = use_input();
    let mut me = use_signal(|| None::<NodeId>);
    let mut items = use_signal(|| None::<NodeHandle>);
    // Measured once laid out; a render after a scroll or a resized window
    // (once laid out: #146) measures again.
    let mut laid_out = use_signal(|| 0u32);
    let _ = laid_out();
    let _ = input.scrolled();
    let _ = input.resized();
    let width = items
        .read()
        .as_ref()
        .and_then(|handle| handle.doc().get_client_bounding_rect(handle.node_id()))
        .map(|rect| rect.width)
        .unwrap_or(0.0);
    let count = if width > 0.0 { columns(width) } else { 5 };
    // LT/RT move focus a page of cards along, while focus is in the shelf.
    let page = use_callback(move |by: isize| {
        let Some(handle) = items.peek().clone() else {
            return false;
        };
        let doc = handle.doc();
        let Some(row) = doc.get_node(handle.node_id()) else {
            return false;
        };
        let cards: Vec<NodeId> = row.children.to_vec();
        let focused = doc.get_focussed_node_id();
        let inside = |card: NodeId| {
            let mut at = focused;
            while let Some(node) = at {
                if node == card {
                    return true;
                }
                at = doc.get_node(node).and_then(|node| node.parent);
            }
            false
        };
        let Some(at) = cards.iter().position(|&card| inside(card)) else {
            return false;
        };
        let visible = doc
            .get_client_bounding_rect(handle.node_id())
            .map(|rect| columns(rect.width))
            .unwrap_or(1);
        let target = cards[page_target(at, cards.len(), visible, by)];
        // The card's own focus stop.
        let stop = crate::input::find_attr(&doc, target, "data-cs", "game-card");
        drop(doc);
        if let Some(stop) = stop {
            input.focus(stop);
        }
        true
    });
    use_input_action(
        Action::PagePrev,
        Bind {
            label: Some(t!("Previous page")),
            owner: me(),
            ..Bind::default()
        },
        move || page.call(-1),
    );
    use_input_action(
        Action::PageNext,
        Bind {
            label: Some(t!("Next page")),
            owner: me(),
            ..Bind::default()
        },
        move || page.call(1),
    );
    let family = crate::console::use_family();
    let heading = format!("shelf-{id}-heading");
    let gaps = GRID_GAP * (count as f64 - 1.0);
    rsx! {
        section {
            "data-cs": "shelf-row",
            "data-focus-section": "shelf-{id}",
            "aria-labelledby": "{heading}",
            class: "mt-8",
            onmounted: move |event| me.set(event.data().downcast::<NodeHandle>().map(NodeHandle::node_id)),
            header { "data-part": "header", class: "mb-4 flex items-center gap-3",
                Svg { icon, part: "icon", class: "size-5 text-accent-alt" }
                h2 { id: "{heading}", "data-part": "title", class: "font-display text-xl font-semibold", "{title}" }
                span { class: "ml-auto flex items-center gap-4",
                    // Console mode shows the buttons that page it (#107).
                    if crate::console::is_console() {
                        span { "data-part": "pager", "aria-hidden": "true", class: "flex gap-1.5",
                            for action in [caskade_ui_input::Action::PagePrev, caskade_ui_input::Action::PageNext] {
                                kbd {
                                    key: "{action:?}",
                                    class: "rounded-control border border-border-strong px-2 py-0.5 font-mono text-xs text-fg-muted",
                                    {crate::console::glyph(action, family).unwrap_or_default()}
                                }
                            }
                        }
                    }
                    {view_all}
                }
            }
            {notice}
            div {
                "data-part": "items",
                class: "grid grid-flow-col gap-4 overflow-x-auto pb-4",
                style: "grid-auto-columns: calc((100% - {gaps}px) / {count}); grid-auto-rows: {crate::library::GRID_ROW - GRID_GAP}px;",
                onmounted: move |event| {
                    items.set(event.data().downcast::<NodeHandle>().cloned());
                    // Measured a frame later, once laid out.
                    spawn(async move {
                        crate::data::sleep(std::time::Duration::from_millis(16)).await;
                        laid_out += 1;
                    });
                },
                {children}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_moves_by_the_cards_that_fit_and_stops_at_the_ends() {
        assert_eq!(page_target(0, 12, 5, 1), 5);
        assert_eq!(page_target(10, 12, 5, 1), 11);
        assert_eq!(page_target(3, 12, 5, -1), 0);
        assert_eq!(page_target(7, 12, 5, -1), 2);
        assert_eq!(columns(1000.0), 5);
        assert_eq!(columns(100.0), 1);
    }
}
