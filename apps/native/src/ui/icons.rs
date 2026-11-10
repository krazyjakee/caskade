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
