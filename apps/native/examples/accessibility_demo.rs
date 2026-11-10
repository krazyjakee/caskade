//! The accessibility test's demo elements (#83) in a window, for checks
//! with a screen reader: `scripts/a11y-atspi.py` runs it on a private
//! display and AT-SPI bus.
//!
//! `cargo run -p caskade-native --example accessibility_demo`

#[path = "../tests/accessibility/demo.rs"]
#[allow(dead_code)] // the test reads what the handlers did; the example doesn't
mod demo;

fn main() {
    dioxus_native::launch(demo::Demo);
}
