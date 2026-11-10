//! The icons the components draw, as the webview's (lucide's) paths, in
//! `currentColor`.

use dioxus_native::prelude::*;

/// An icon: lucide's 24×24 path data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icon(pub &'static [&'static str]);

pub const CHECK: Icon = Icon(&["M20 6 9 17l-5-5"]);
pub const MINUS: Icon = Icon(&["M5 12h14"]);
pub const CHEVRON_DOWN: Icon = Icon(&["m6 9 6 6 6-6"]);
pub const X: Icon = Icon(&["M18 6 6 18", "m6 6 12 12"]);
const CIRCLE: &str = "M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z";
pub const CIRCLE_CHECK: Icon = Icon(&[CIRCLE, "m9 12 2 2 4-4"]);
pub const CIRCLE_SLASH: Icon = Icon(&[CIRCLE, "M9 15 15 9"]);
pub const CIRCLE_ALERT: Icon = Icon(&[CIRCLE, "M12 8v4", "M12 16h.01"]);
pub const CIRCLE_X: Icon = Icon(&[CIRCLE, "m15 9-6 6", "m9 9 6 6"]);
pub const SHOPPING_CART: Icon = Icon(&[
    "M8 20a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
    "M19 20a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
    "M2.05 2.05h2l2.66 12.42a2 2 0 0 0 2 1.58h9.78a2 2 0 0 0 1.95-1.57l1.65-7.43H5.12",
]);
pub const HEART: Icon = Icon(&[
    "M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z",
]);
pub const CLOCK: Icon = Icon(&[CIRCLE, "M12 6v6l4 2"]);
pub const HARD_DRIVE: Icon = Icon(&[
    "M22 12H2",
    "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z",
    "M6 16h.01",
    "M10 16h.01",
]);
pub const STORE: Icon = Icon(&[
    "m2 7 4.41-4.41A2 2 0 0 1 7.83 2h8.34a2 2 0 0 1 1.42.59L22 7",
    "M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8",
    "M15 22v-4a2 2 0 0 0-2-2h-2a2 2 0 0 0-2 2v4",
    "M2 7h20",
    "M22 7v3a2 2 0 0 1-2 2a2.7 2.7 0 0 1-1.59-.63.7.7 0 0 0-.82 0A2.7 2.7 0 0 1 16 12a2.7 2.7 0 0 1-1.59-.63.7.7 0 0 0-.82 0A2.7 2.7 0 0 1 12 12a2.7 2.7 0 0 1-1.59-.63.7.7 0 0 0-.82 0A2.7 2.7 0 0 1 8 12a2.7 2.7 0 0 1-1.59-.63.7.7 0 0 0-.82 0A2.7 2.7 0 0 1 4 12a2 2 0 0 1-2-2V7",
]);
pub const GAMEPAD: Icon = Icon(&[
    "M6 11h4",
    "M8 9v4",
    "M15 12h.01",
    "M18 10h.01",
    "M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z",
]);

/// Draws `icon` with `class`, `stroke_width` (lucide's default is 2) and
/// `data-part` when given.
#[component]
pub fn Svg(
    icon: Icon,
    #[props(default)] class: String,
    #[props(default = 2.0)] stroke_width: f32,
    #[props(default)] part: Option<&'static str>,
) -> Element {
    rsx! {
        svg {
            "data-part": part,
            "aria-hidden": "true",
            class: "{class}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "{stroke_width}",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            for d in icon.0 {
                path { d: *d }
            }
        }
    }
}
