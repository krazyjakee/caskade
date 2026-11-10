//! A tooltip: a short hint over its trigger while a pointer rests on it
//! (after a moment) or keyboard focus is on it, gone on Escape, as
//! react-aria's `TooltipTrigger`. The trigger is described by it
//! (`aria-describedby`), so screen readers read it too. Tooltips never hold
//! anything to act on: pads see their trigger's label and hints instead.

use std::time::Duration;

use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::ids::use_id;
use super::popover::{OFFSET, use_trigger};

/// How long a pointer rests before the tooltip shows.
const DELAY: Duration = Duration::from_millis(700);

#[component]
pub fn Tooltip(
    /// The hint.
    text: String,
    #[props(default)] class: String,
    /// The trigger: one focusable control.
    children: Element,
) -> Element {
    let trigger = use_trigger();
    let id = use_id("tooltip");
    let mut shown = use_signal(|| false);
    let mut place = use_signal(|| None::<(f64, f64)>);
    let mut generation = use_signal(|| 0u64);
    let mut show_now = move || {
        if let Some(anchor) = trigger.anchor() {
            place.set(Some((anchor.x + anchor.width / 2.0, anchor.y)));
            shown.set(true);
        }
    };
    let mut hide = move || {
        generation += 1;
        shown.set(false);
    };
    rsx! {
        span {
            class: "inline-flex",
            "aria-describedby": shown().then(|| id.clone()),
            onmounted: move |event| trigger.mounted(event),
            onmouseenter: move |_| {
                generation += 1;
                let mine = *generation.peek();
                spawn(async move {
                    // On the app's runtime: a timer needs its reactor.
                    let _ = caskade_app::runtime::spawn(async { tokio::time::sleep(DELAY).await }).await;
                    if *generation.peek() == mine {
                        show_now();
                    }
                });
            },
            onmouseleave: move |_| hide(),
            onfocusin: move |_| show_now(),
            onfocusout: move |_| hide(),
            onkeydown: move |event| {
                if *shown.peek() && event.key() == Key::Named(NamedKey::Escape) {
                    hide();
                }
            },
            {children}
        }
        if let (true, Some((x, y))) = (shown(), place()) {
            div {
                id: "{id}",
                role: "tooltip",
                style: "position: fixed; left: {x}px; bottom: calc(100vh - {y - OFFSET}px); translate: -50% 0;",
                class: "pointer-events-none z-50 max-w-64 rounded-control border border-border-strong bg-surface-overlay px-2.5 py-1.5 text-xs text-fg shadow-lg {class}",
                "{text}"
            }
        }
    }
}
