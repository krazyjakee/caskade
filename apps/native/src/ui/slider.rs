//! A value on a track, as `@caskade/ui`'s `Slider`: the thumb is the
//! control (`role="slider"`); arrows step, Page Up/Down step by ten,
//! Home/End go to the ends, a pad's left and right step while it has
//! focus, and a click or drag on the track sets the value. Screen readers'
//! increment and decrement arrive as arrow keys (Blitz).

use blitz_dom::NodeId;
use caskade_ui_input::Action;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::ids::use_id;
use super::interact::{flag, use_forced};
use super::settings::use_row_label;
use crate::input::{Bind, use_input_action};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("label", "The visible label, when given."),
        ("output", "The value readout."),
        ("track", "The rail; carries `data-hovered`."),
        ("fill", "The filled part of the rail."),
        (
            "thumb",
            "The handle; carries `data-dragging` and `data-focus-visible`.",
        ),
    ],
    interaction: &["data-disabled", "data-orientation"],
    ..Entry::new(
        "slider",
        "Slider (apps/native/src/ui/slider.rs)",
        Source::Ui,
        "A value on a track.",
    )
};

#[component]
pub fn Slider(
    value: f64,
    on_change: EventHandler<f64>,
    #[props(default = 0.0)] min: f64,
    #[props(default = 100.0)] max: f64,
    #[props(default = 1.0)] step: f64,
    #[props(default)] label: Option<String>,
    /// The readout, e.g. `40 %`; the value itself by default.
    #[props(default)]
    format: Option<Callback<f64, String>>,
    #[props(default)] disabled: bool,
    /// A change ended: each key or pad step, or the pointer let go after
    /// a drag or a click on the track. For saving once (`DraftSlider`).
    #[props(default)]
    on_change_end: Option<EventHandler<f64>>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let label_id = use_id("slider");
    // Without a label of its own, a setting row's names it.
    let row = use_row_label().filter(|_| label.is_none());
    let mut thumb = use_signal(|| None::<NodeId>);
    let mut track = use_signal(|| None::<NodeHandle>);
    let forced = use_forced();
    let mut hovered = use_signal(|| forced.hovered);
    let mut dragging = use_signal(|| false);
    let clamp = move |value: f64| {
        let stepped = ((value - min) / step).round() * step + min;
        stepped.clamp(min, max)
    };
    // The value the last change set, for its end; what's shown otherwise.
    let mut latest = use_hook(|| CopyValue::new(value));
    if !*dragging.peek() {
        latest.set(value);
    }
    let mut set = move |next: f64| {
        let next = clamp(next);
        if !disabled && next != value {
            latest.set(next);
            on_change.call(next);
        }
    };
    let end = move || {
        if let Some(on_change_end) = on_change_end
            && !disabled
        {
            on_change_end.call(latest());
        }
    };
    let text = match format {
        Some(format) => format(value),
        None => format!("{value}"),
    };
    let percent = if max > min {
        ((value - min) / (max - min) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    for (action, sign) in [(Action::NavLeft, -1.0), (Action::NavRight, 1.0)] {
        use_input_action(
            action,
            Bind {
                owner: thumb(),
                ..Bind::default()
            },
            move || {
                set(value + sign * step);
                end();
                true
            },
        );
    }
    let mut from_pointer = move |x: f64| {
        let rect = track
            .peek()
            .as_ref()
            .and_then(|handle| handle.doc().get_client_bounding_rect(handle.node_id()));
        if let Some(rect) = rect.filter(|rect| rect.width > 0.0) {
            set(min + (x - rect.x) / rect.width * (max - min));
        }
    };
    let output = rsx! {
        output { "data-part": "output", class: "min-w-10 text-right font-mono text-xs text-fg-muted", "{text}" }
    };
    rsx! {
        div {
            "data-cs": "slider",
            "data-orientation": "horizontal",
            "data-disabled": flag(disabled),
            class: "group flex min-w-48 flex-col gap-1 data-disabled:opacity-40 {class}",
            if let Some(label) = &label {
                div { class: "flex items-center justify-between gap-3",
                    span { id: "{label_id}", "data-part": "label", class: "text-sm font-medium", "{label}" }
                    {output.clone()}
                }
            }
            div { class: "flex items-center gap-3",
                div {
                    "data-part": "track",
                    "data-hovered": flag(hovered()),
                    class: "relative h-5 flex-1",
                    onmounted: move |event| track.set(event.data().downcast::<NodeHandle>().cloned()),
                    onmouseenter: move |_| hovered.set(true),
                    onmouseleave: move |_| {
                        hovered.set(false);
                        if *dragging.peek() {
                            dragging.set(false);
                            end();
                        }
                    },
                    onmousedown: move |event| {
                        dragging.set(true);
                        from_pointer(event.client_coordinates().x);
                    },
                    onmousemove: move |event| {
                        if *dragging.peek() {
                            from_pointer(event.client_coordinates().x);
                        }
                    },
                    onmouseup: move |_| {
                        if *dragging.peek() {
                            dragging.set(false);
                            end();
                        }
                    },
                    div { class: "absolute top-1/2 h-1.5 w-full -translate-y-1/2 rounded-full bg-surface-hover" }
                    div {
                        "data-part": "fill",
                        class: "absolute top-1/2 h-1.5 -translate-y-1/2 rounded-full bg-linear-to-r from-accent to-accent-alt",
                        style: "width: {percent}%;",
                    }
                    div {
                        "data-part": "thumb",
                        role: "slider",
                        tabindex: if disabled { "-1" } else { "0" },
                        "aria-valuenow": "{value}",
                        "aria-valuemin": "{min}",
                        "aria-valuemax": "{max}",
                        "aria-valuetext": "{text}",
                        "aria-orientation": "horizontal",
                        "aria-disabled": flag(disabled),
                        "aria-labelledby": label.as_ref().map(|_| label_id.clone()),
                        "aria-label": row.as_ref().map(|row| row.label.clone()),
                        "aria-describedby": row.as_ref().and_then(|row| row.description_id.clone()),
                        "data-dragging": flag(dragging()),
                        "data-focus-visible": flag(forced.focus_visible),
                        class: "absolute top-1/2 size-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-accent-alt bg-white shadow-[0_0_10px] shadow-accent-alt/60 transition-[scale] duration-(--cs-motion-duration-fast) data-dragging:scale-115 data-focus-visible:focus-ring-outside",
                        style: "left: {percent}%;",
                        onmounted: move |event| thumb.set(event.data().downcast::<NodeHandle>().map(NodeHandle::node_id)),
                        onkeydown: move |event| {
                            let next = match event.key() {
                                Key::Named(NamedKey::ArrowRight | NamedKey::ArrowUp) => value + step,
                                Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowDown) => value - step,
                                Key::Named(NamedKey::PageUp) => value + step * 10.0,
                                Key::Named(NamedKey::PageDown) => value - step * 10.0,
                                Key::Named(NamedKey::Home) => min,
                                Key::Named(NamedKey::End) => max,
                                _ => return,
                            };
                            event.prevent_default();
                            event.stop_propagation();
                            set(next);
                            end();
                        },
                        ..attributes,
                    }
                }
                // Without a label the readout sits beside the track.
                if label.is_none() {
                    {output}
                }
            }
        }
    }
}
