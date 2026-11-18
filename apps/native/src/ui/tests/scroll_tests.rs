//! Scroll chaining (#164): a wheel over a shelf or a dialog moves the page
//! behind it only where it should. A shelf keeps sideways scroll to itself
//! but passes vertical scroll on to the page; nothing in a dialog reaches
//! the page.

use super::*;
use blitz_traits::events::{BlitzWheelDelta, BlitzWheelEvent, MouseEventButtons};

/// The app's stylesheet: the containment comes from its utilities.
const APP_CSS: &str = include_str!("../../../assets/app.css");

/// A page taller and wider than the window, with a shelf of 20 cards.
fn shelf_page() -> Element {
    rsx! {
        style { {APP_CSS} }
        InputRoot {
            div { id: "page", style: "height: 600px; overflow: auto;",
                crate::home::ShelfRow { id: "test", title: "Shelf", icon: crate::ui::icons::SPARKLES,
                    for i in 0..20 {
                        div { key: "{i}", style: "height: 100px;", "{i}" }
                    }
                }
                div { style: "width: 3000px; height: 3000px;" }
            }
        }
    }
}

/// A page taller than the window, under a dialog with a list of its own.
fn dialog_page() -> Element {
    rsx! {
        style { {APP_CSS} }
        InputRoot {
            div { id: "page", style: "height: 600px; overflow: auto;",
                div { style: "height: 3000px;" }
                Dialog { title: "Long list",
                    div { id: "list", class: "max-h-48 overflow-y-auto",
                        for i in 0..50 {
                            p { key: "{i}", "Line {i}" }
                        }
                    }
                }
            }
        }
    }
}

impl Window {
    /// A touchpad's scroll over the middle of `node`: `right` and `down`
    /// pixels.
    fn swipe_over(&mut self, node: NodeId, right: f64, down: f64) {
        let (x, y) = self.middle_of(node);
        let at = pointer(x, y, false);
        let coords = at.coords;
        blitz_dom::Document::handle_ui_event(&mut self.doc, UiEvent::PointerMove(at));
        let wheel = BlitzWheelEvent {
            delta: BlitzWheelDelta::Pixels(-right, -down),
            coords,
            buttons: MouseEventButtons::None,
            mods: Modifiers::empty(),
            element: Default::default(),
        };
        blitz_dom::Document::handle_ui_event(&mut self.doc, UiEvent::Wheel(wheel));
        self.settle();
    }

    fn scrolled(&self, node: NodeId) -> (f64, f64) {
        let offset = *self
            .doc
            .inner
            .borrow()
            .get_node(node)
            .unwrap()
            .scroll_offset();
        (offset.x, offset.y)
    }

    fn part(&self, name: &str) -> NodeId {
        let doc = self.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.attr(LocalName::from("data-part")) == Some(name) {
                found = Some(id);
            }
        });
        found.unwrap_or_else(|| panic!("no data-part={name}"))
    }
}

#[test]
fn a_shelf_keeps_sideways_scroll_and_passes_vertical_scroll_on() {
    let _one = one_window();
    let mut window = Window::open(shelf_page);
    let shelf = window.part("items");
    let page = window.id("page");
    window.swipe_over(shelf, 20_000.0, 200.0);
    let (shelf_x, _) = window.scrolled(shelf);
    assert!(shelf_x > 0.0, "the shelf scrolled sideways");
    assert_eq!(
        window.scrolled(page),
        (0.0, 200.0),
        "the page took the vertical part only"
    );
}

#[test]
fn nothing_in_a_dialog_scrolls_the_page_behind_it() {
    let _one = one_window();
    let mut window = Window::open(dialog_page);
    let list = window.id("list");
    let page = window.id("page");
    window.swipe_over(list, 0.0, 20_000.0);
    assert!(window.scrolled(list).1 > 0.0, "the list scrolled");
    window.swipe_over(list, 0.0, 200.0);
    assert_eq!(window.scrolled(page), (0.0, 0.0));
}
