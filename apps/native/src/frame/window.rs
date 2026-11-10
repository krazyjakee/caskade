//! The window's own title bar (`shell/WindowControls.tsx`,
//! `useWindowDrag.ts`, moved here from #87): on Windows and Linux the
//! window has no native frame, so the top bar, the sidebar's brand and the
//! status bar move it (a press on them, not on a control, then 4 px of
//! travel), a double click there maximizes it, the edges resize it, and the
//! top bar holds minimize, maximize and close. macOS keeps its traffic
//! lights, over the top of the sidebar.

use crate::ui::{Svg, icons};
use blitz_dom::{BaseDocument, LocalName, NodeId};
use caskade_ui_i18n::tp;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::dioxus_html::input_data::MouseButton;
use dioxus_native::prelude::*;
use dioxus_native::winit::window::{ResizeDirection, Window};
use std::time::{Duration, Instant};

/// Whether Caskade draws the window's frame: Windows and Linux. macOS keeps
/// its own title bar's buttons over the window.
pub const CUSTOM_FRAME: bool = cfg!(any(target_os = "linux", target_os = "windows"));

/// Pointer travel before a press on the title bar becomes a window move.
const DRAG_THRESHOLD: f64 = 4.0;
/// Two presses this close make a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(500);

fn window() -> Option<&'static std::sync::Arc<dyn Window>> {
    crate::shell::main_window()
}

/// Whether the window is maximized, kept up to date as it resizes.
fn use_maximized() -> bool {
    let mut maximized = use_signal(|| window().is_some_and(|window| window.is_maximized()));
    // Without a window (headless tests) there are no window events; that
    // stays so for the document's life, so the hook is always or never
    // called.
    if try_consume_context::<std::sync::Arc<dyn Window>>().is_none() {
        return maximized();
    }
    dioxus_native::use_window_event(move |event, _| {
        if matches!(
            event,
            dioxus_native::winit::event::WindowEvent::SurfaceResized(_)
        ) {
            let now = window().is_some_and(|window| window.is_maximized());
            if *maximized.peek() != now {
                maximized.set(now);
            }
        }
    });
    maximized()
}

fn toggle_maximized() {
    if let Some(window) = window() {
        window.set_maximized(!window.is_maximized());
    }
}

const CONTROL: &str = "grid size-8 place-items-center rounded-control text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:text-fg data-focus-visible:focus-ring";

/// Minimize, maximize or restore, and close, at the end of the top bar.
/// Like a native title bar's, they aren't tab stops, so neither Tab nor a
/// pad's moves land on Close by accident.
#[component]
pub fn WindowControls() -> Element {
    let maximized = use_maximized();
    if !CUSTOM_FRAME {
        return rsx! {};
    }
    let neutral = format!("{CONTROL} hover:bg-surface-hover active:bg-surface-hover");
    rsx! {
        div {
            "data-cs": "window-controls",
            "data-status": if maximized { "maximized" } else { "restored" },
            class: "flex items-center gap-1 border-l border-border pl-3",
            button {
                "data-part": "minimize",
                r#type: "button",
                tabindex: "-1",
                "aria-label": tp!("window", "Minimize"),
                class: "{neutral}",
                onclick: move |_| {
                    if let Some(window) = window() {
                        window.set_minimized(true);
                    }
                },
                Svg { icon: icons::MINUS, class: "size-4", stroke_width: 1.75 }
            }
            button {
                "data-part": "maximize",
                r#type: "button",
                tabindex: "-1",
                "aria-label": if maximized { tp!("window", "Restore") } else { tp!("window", "Maximize") },
                class: "{neutral}",
                onclick: move |_| toggle_maximized(),
                if maximized {
                    Svg { icon: icons::COPY, class: "size-3.5 -scale-x-100", stroke_width: 1.75 }
                } else {
                    Svg { icon: icons::SQUARE, class: "size-3.5", stroke_width: 1.75 }
                }
            }
            button {
                "data-part": "close",
                r#type: "button",
                tabindex: "-1",
                "aria-label": tp!("window", "Close"),
                class: "{CONTROL} hover:bg-danger active:bg-danger",
                // As the OS's close: to the tray or quit, as set.
                onclick: move |_| crate::launch::ask(crate::launch::Request::Close),
                Svg { icon: icons::X, class: "size-4", stroke_width: 1.75 }
            }
        }
    }
}

/// What makes an element a control that keeps its own clicks.
fn clickable(doc: &BaseDocument, id: NodeId) -> bool {
    let Some(node) = doc.get_node(id) else {
        return false;
    };
    let Some(element) = node.element_data() else {
        return false;
    };
    let name = element.name.local.as_ref();
    if matches!(
        name,
        "a" | "button" | "input" | "select" | "textarea" | "label" | "summary"
    ) {
        return true;
    }
    if node
        .attr(LocalName::from("contenteditable"))
        .is_some_and(|value| value != "false")
    {
        return true;
    }
    if node
        .attr(LocalName::from("tabindex"))
        .is_some_and(|value| value != "-1")
    {
        return true;
    }
    node.attr(LocalName::from("role")).is_some_and(|role| {
        matches!(
            role,
            "button" | "link" | "menuitem" | "tab" | "checkbox" | "radio" | "switch" | "option"
        )
    })
}

/// Whether a press on `id` belongs to the title bar: inside an element with
/// `data-window-drag`, and not on a control within it; `"false"` opts a
/// subtree out (`inDragRegion`).
pub fn in_drag_region(doc: &BaseDocument, id: NodeId) -> bool {
    let mut at = Some(id);
    while let Some(id) = at {
        let Some(node) = doc.get_node(id) else {
            return false;
        };
        match node.attr(LocalName::from("data-window-drag")) {
            Some("false") => return false,
            Some(_) => return true,
            None => {}
        }
        if clickable(doc, id) {
            return false;
        }
        at = node.parent;
    }
    false
}

/// A press on the title bar, waiting to become a move.
#[derive(Clone, Copy)]
struct Pressed {
    x: f64,
    y: f64,
}

/// The title bar's moves: the frame's root listens with these.
#[derive(Clone, Copy)]
pub struct Drag {
    root: Signal<Option<NodeHandle>>,
    pressed: Signal<Option<Pressed>>,
    last_press: Signal<Option<(Instant, f64, f64)>>,
}

pub fn use_drag() -> Drag {
    Drag {
        root: use_signal(|| None),
        pressed: use_signal(|| None),
        last_press: use_signal(|| None),
    }
}

impl Drag {
    pub fn mounted(mut self, event: &Event<MountedData>) {
        self.root
            .set(event.data().downcast::<NodeHandle>().cloned());
    }

    fn over_title_bar(&self) -> bool {
        let root = self.root.peek();
        let Some(root) = root.as_ref() else {
            return false;
        };
        let doc = root.doc();
        doc.get_hover_node_id()
            .is_some_and(|hovered| in_drag_region(&doc, hovered))
    }

    pub fn down(mut self, event: &MouseEvent) {
        self.pressed.set(None);
        if event.trigger_button() != Some(MouseButton::Primary) || !self.over_title_bar() {
            return;
        }
        let point = event.screen_coordinates();
        let now = Instant::now();
        let double = self.last_press.peek().is_some_and(|(at, x, y)| {
            now.duration_since(at) < DOUBLE_CLICK
                && (point.x - x).hypot(point.y - y) < DRAG_THRESHOLD
        });
        if double {
            self.last_press.set(None);
            toggle_maximized();
            return;
        }
        self.last_press.set(Some((now, point.x, point.y)));
        self.pressed.set(Some(Pressed {
            x: point.x,
            y: point.y,
        }));
    }

    pub fn moved(mut self, event: &MouseEvent) {
        let Some(pressed) = *self.pressed.peek() else {
            return;
        };
        if !event.held_buttons().contains(MouseButton::Primary) {
            self.pressed.set(None);
            return;
        }
        let point = event.screen_coordinates();
        if (point.x - pressed.x).hypot(point.y - pressed.y) < DRAG_THRESHOLD {
            return;
        }
        self.pressed.set(None);
        self.last_press.set(None);
        if let Some(window) = window() {
            let _ = window.drag_window();
        }
    }

    pub fn up(mut self) {
        self.pressed.set(None);
    }
}

/// The window's edges and corners, which resize it while it has no native
/// frame and isn't maximized.
#[component]
pub fn ResizeEdges() -> Element {
    let maximized = use_maximized();
    if !CUSTOM_FRAME || maximized {
        return rsx! {};
    }
    let edges: [(&str, &str, ResizeDirection); 8] = [
        (
            "top: 0; left: 6px; right: 6px; height: 6px;",
            "ns-resize",
            ResizeDirection::North,
        ),
        (
            "bottom: 0; left: 6px; right: 6px; height: 6px;",
            "ns-resize",
            ResizeDirection::South,
        ),
        (
            "left: 0; top: 6px; bottom: 6px; width: 6px;",
            "ew-resize",
            ResizeDirection::West,
        ),
        (
            "right: 0; top: 6px; bottom: 6px; width: 6px;",
            "ew-resize",
            ResizeDirection::East,
        ),
        (
            "top: 0; left: 0; width: 6px; height: 6px;",
            "nwse-resize",
            ResizeDirection::NorthWest,
        ),
        (
            "top: 0; right: 0; width: 6px; height: 6px;",
            "nesw-resize",
            ResizeDirection::NorthEast,
        ),
        (
            "bottom: 0; left: 0; width: 6px; height: 6px;",
            "nesw-resize",
            ResizeDirection::SouthWest,
        ),
        (
            "bottom: 0; right: 0; width: 6px; height: 6px;",
            "nwse-resize",
            ResizeDirection::SouthEast,
        ),
    ];
    rsx! {
        for (index, (place, cursor, direction)) in edges.into_iter().enumerate() {
            div {
                key: "{index}",
                "aria-hidden": "true",
                "data-window-resize": "true",
                style: "position: fixed; z-index: 60; {place} cursor: {cursor};",
                onmousedown: move |event: MouseEvent| {
                    event.stop_propagation();
                    if let Some(window) = window() {
                        let _ = window.drag_resize_window(direction);
                    }
                },
            }
        }
    }
}
