//! A device's kind, as an icon.

use caskade_core::devices::DeviceKind;
use dioxus_native::prelude::*;

use super::icons::{GAMEPAD_2, LAPTOP, MONITOR, Svg, TV};

/// A device's kind, as an icon.
#[component]
pub fn DeviceIcon(
    kind: DeviceKind,
    #[props(default = "size-5".to_owned())] class: String,
) -> Element {
    let icon = match kind {
        DeviceKind::Desktop => MONITOR,
        DeviceKind::Laptop => LAPTOP,
        DeviceKind::Handheld => GAMEPAD_2,
        DeviceKind::Tv => TV,
    };
    rsx! {
        Svg { icon, class }
    }
}
