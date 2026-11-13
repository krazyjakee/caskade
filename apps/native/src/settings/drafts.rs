//! Fields that keep what's being edited (the webview's `DraftField.tsx` and
//! `DraftSlider.tsx`): saving every keystroke and showing the stored value
//! lost characters typed faster than a save round trip, and a slider that
//! saved every step rescaled the page under the pointer (the interface
//! scale, #55).

use std::time::Duration;

use dioxus_native::prelude::*;

use crate::ui::{Slider, TextField};

/// How long typing pauses before a draft saves.
pub const PAUSE: Duration = Duration::from_millis(600);

/// A text field that keeps what the user types. It saves after a short
/// pause, on Enter or when focus leaves, and takes the stored value only
/// while it isn't focused.
#[component]
pub fn DraftField(
    value: String,
    commit: EventHandler<String>,
    /// Saves after [`PAUSE`] without typing; `false` saves only when focus
    /// leaves and on Enter, for a value that does work as it changes (a
    /// folder that's checked on disk).
    #[props(default = true)]
    pause: bool,
    /// Says what's wrong with a draft, or `None`; a draft with a problem
    /// isn't saved.
    #[props(default)]
    validate: Option<Callback<String, Option<String>>>,
    #[props(default)] label: Option<String>,
    #[props(default)] class: String,
) -> Element {
    let mut draft = use_signal(|| value.clone());
    let mut focused = use_signal(|| false);
    let mut typed = use_signal(|| 0u64);
    // The stored value, while the field isn't being edited.
    let stored = value.clone();
    use_effect(use_reactive!(|stored| {
        if !*focused.peek() {
            draft.set(stored);
        }
    }));
    let problem = validate.and_then(|validate| validate(draft()));
    let save = move |next: String| {
        let problem = validate.and_then(|validate| validate(next.clone()));
        if problem.is_none() && next != value {
            commit.call(next);
        }
    };
    let input_save = save.clone();
    let blur_save = save.clone();
    rsx! {
        div {
            class: "contents",
            onfocusin: move |_| focused.set(true),
            onfocusout: move |_| {
                focused.set(false);
                blur_save(draft.peek().clone());
            },
            TextField {
                value: draft(),
                label,
                error: problem,
                class,
                on_input: move |next: String| {
                    draft.set(next.clone());
                    typed += 1;
                    if pause {
                        let mine = *typed.peek();
                        let save = input_save.clone();
                        spawn(async move {
                            crate::data::sleep(PAUSE).await;
                            // Only if nothing was typed since.
                            if *typed.peek() == mine {
                                save(next);
                            }
                        });
                    }
                },
                on_submit: move |next: String| save(next),
            }
        }
    }
}

/// A slider that saves once it's let go (#55): dragging moves only the
/// slider, so a setting that changes the page doesn't rescale it under the
/// pointer, and a drag is one save, not one per step. Keys and a pad's
/// presses end each step, so each saves.
#[component]
pub fn DraftSlider(
    value: f64,
    commit: EventHandler<f64>,
    #[props(default = 0.0)] min: f64,
    #[props(default = 100.0)] max: f64,
    #[props(default = 1.0)] step: f64,
    #[props(default)] format: Option<Callback<f64, String>>,
    #[props(default)] label: Option<String>,
    #[props(default)] class: String,
) -> Element {
    let mut draft = use_signal(|| value);
    let mut moving = use_signal(|| false);
    use_effect(use_reactive!(|value| {
        if !*moving.peek() {
            draft.set(value);
        }
    }));
    rsx! {
        Slider {
            value: draft(),
            min,
            max,
            step,
            format,
            label,
            class,
            on_change: move |next| {
                moving.set(true);
                draft.set(next);
            },
            on_change_end: move |next| {
                moving.set(false);
                if next != value {
                    commit.call(next);
                }
            },
        }
    }
}
