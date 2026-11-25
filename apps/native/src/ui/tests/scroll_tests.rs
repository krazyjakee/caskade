//! Scroll chaining and trackpad axis lock (#164): a wheel over a shelf or a
//! dialog moves the page behind it only where it should. A shelf keeps
//! sideways scroll to itself but passes vertical scroll on to the page;
//! nothing in a dialog reaches the page. A trackpad gesture locks to its main
//! axis so a mostly vertical swipe over a shelf leaves the shelf still.

use super::*;
use blitz_traits::events::{
    BlitzWheelDelta, BlitzWheelEvent, BlitzWheelPhase, BlitzWheelSource, MouseEventButtons,
};

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
    /// pixels, outside a gesture.
    fn swipe_over(&mut self, node: NodeId, right: f64, down: f64) {
        self.wheel_over(
            node,
            BlitzWheelDelta::Pixels(-right, -down),
            BlitzWheelPhase::Moved,
            BlitzWheelSource::Unknown,
        );
        self.settle();
    }

    fn wheel_over(
        &mut self,
        node: NodeId,
        delta: BlitzWheelDelta,
        phase: BlitzWheelPhase,
        source: BlitzWheelSource,
    ) {
        let (x, y) = self.middle_of(node);
        let at = pointer(x, y, false);
        let coords = at.coords;
        blitz_dom::Document::handle_ui_event(&mut self.doc, UiEvent::PointerMove(at));
        let wheel = BlitzWheelEvent {
            delta,
            phase,
            source,
            coords,
            buttons: MouseEventButtons::None,
            mods: Modifiers::empty(),
            element: Default::default(),
        };
        blitz_dom::Document::handle_ui_event(&mut self.doc, UiEvent::Wheel(wheel));
    }

    /// A trackpad scroll gesture over the middle of `node` as
    /// `(right, down, phase, source)` steps.
    fn gesture_over(
        &mut self,
        node: NodeId,
        steps: &[(f64, f64, BlitzWheelPhase, BlitzWheelSource)],
    ) {
        for &(right, down, phase, source) in steps {
            self.wheel_over(node, BlitzWheelDelta::Pixels(-right, -down), phase, source);
        }
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

#[test]
fn a_mostly_vertical_trackpad_swipe_and_momentum_leave_a_shelf_still() {
    use BlitzWheelPhase::{Ended, Moved, Started};
    use BlitzWheelSource::{Finger, Momentum};

    let _one = one_window();
    let mut window = Window::open(shelf_page);
    let shelf = window.part("items");
    let page = window.id("page");
    window.gesture_over(
        shelf,
        &[
            (0.0, 0.0, Started, Finger),
            (1.5, 15.0, Moved, Finger),
            (10.0, 15.0, Moved, Finger),
            (2.5, 5.0, Ended, Finger),
            (0.0, 0.0, Started, Momentum),
            (2.5, 10.0, Moved, Momentum),
            (0.0, 0.0, Ended, Momentum),
        ],
    );
    assert_eq!(window.scrolled(shelf), (0.0, 0.0));
    assert_eq!(window.scrolled(page), (0.0, 45.0));
}

#[test]
fn a_mostly_horizontal_trackpad_swipe_leaves_the_page_still() {
    use BlitzWheelPhase::{Ended, Moved, Started};
    use BlitzWheelSource::Finger;

    let _one = one_window();
    let mut window = Window::open(shelf_page);
    let shelf = window.part("items");
    let page = window.id("page");
    window.gesture_over(
        shelf,
        &[
            (30.0, 5.0, Started, Finger),
            (30.0, 10.0, Moved, Finger),
            (0.0, 0.0, Ended, Finger),
        ],
    );
    assert_eq!(window.scrolled(shelf), (60.0, 0.0));
    assert_eq!(window.scrolled(page), (0.0, 0.0));
}

#[test]
fn a_diagonal_trackpad_swipe_moves_both_shelf_and_page() {
    use BlitzWheelPhase::{Ended, Moved, Started};
    use BlitzWheelSource::Finger;

    let _one = one_window();
    let mut window = Window::open(shelf_page);
    let shelf = window.part("items");
    let page = window.id("page");
    window.gesture_over(
        shelf,
        &[
            (0.0, 0.0, Started, Finger),
            (20.0, 20.0, Moved, Finger),
            (10.0, 0.0, Moved, Finger),
            (0.0, 0.0, Ended, Finger),
        ],
    );
    assert_eq!(window.scrolled(shelf), (30.0, 0.0));
    assert_eq!(window.scrolled(page), (0.0, 20.0));
}

#[test]
fn lifting_fingers_ends_the_axis_lock() {
    use BlitzWheelPhase::{Ended, Moved, Started};
    use BlitzWheelSource::{Finger, Unknown};

    let _one = one_window();
    let mut window = Window::open(shelf_page);
    let shelf = window.part("items");
    let page = window.id("page");
    // A vertical gesture ends when the fingers lift, so a new horizontal
    // gesture locks to its own axis and a subsequent non-gesture event moves
    // both axes.
    window.gesture_over(
        shelf,
        &[
            (0.0, 20.0, Started, Finger),
            (0.0, 0.0, Ended, Finger),
            (30.0, 0.0, Started, Finger),
            (10.0, 10.0, Moved, Finger),
            (0.0, 0.0, Ended, Finger),
            (10.0, 10.0, Moved, Unknown),
        ],
    );
    assert_eq!(window.scrolled(shelf), (50.0, 0.0));
    assert_eq!(window.scrolled(page), (0.0, 30.0));
}

#[test]
fn a_mouse_wheel_notch_scrolls_outside_a_gesture() {
    use BlitzWheelPhase::{Moved, Started};
    use BlitzWheelSource::{Finger, Wheel};

    let _one = one_window();
    let mut window = Window::open(shelf_page);
    let shelf = window.part("items");
    let page = window.id("page");
    // Even if a horizontal trackpad gesture was active, a physical mouse
    // wheel notch resets it and glides the page vertically.
    window.gesture_over(shelf, &[(30.0, 0.0, Started, Finger)]);
    window.wheel_over(shelf, BlitzWheelDelta::Lines(0.0, -1.0), Moved, Wheel);
    assert!(window.doc.inner.borrow().is_scroll_animating());
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(10));
        window.doc.inner.borrow_mut().resolve(0.0);
        if !window.doc.inner.borrow().is_scroll_animating() {
            break;
        }
    }
    window.settle();
    assert_eq!(window.scrolled(shelf), (30.0, 0.0));
    assert!(
        window.scrolled(page).1 > 0.0,
        "the mouse wheel scrolled the page"
    );
}
