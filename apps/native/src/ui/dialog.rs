//! Modal dialogs, as react-aria's `ModalOverlay`/`Modal`/`Dialog` and the
//! webview's `ConfirmDialog`: the page behind is covered, focus goes in (to
//! `data-focus-default`, else the first control) and back to where it was
//! when the dialog closes (the input layer's overlay scope), and Escape or a
//! pad's Back closes it unless it's busy. A dialog is named by its heading.

use blitz_dom::NodeId;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::button::{Button, ButtonVariant};
use super::ids::use_id;
use super::interact::flag;
use super::popover::{start_in, use_overlay_focus};
use crate::input::use_input;
use crate::input::{Bind, use_input_action};
use caskade_ui_i18n::t;
use caskade_ui_input::Action;

/// A modal dialog titled `title`. `on_close` (Escape, Back, a click on the
/// backdrop) is left out while it mustn't be dismissed.
#[component]
pub fn Dialog(
    title: String,
    #[props(default)] on_close: Option<EventHandler<()>>,
    /// An alert: it asks before something that matters (`role="alertdialog"`).
    #[props(default)]
    alert: bool,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let mut me = use_signal(|| None::<NodeId>);
    let heading = use_id("dialog-title");
    let focus = use_overlay_focus();
    let input = use_input();
    let close = move || {
        if let Some(on_close) = on_close {
            on_close.call(());
        }
    };
    use_input_action(
        Action::Back,
        Bind {
            owner: me(),
            ..Bind::default()
        },
        move || {
            close();
            true
        },
    );
    rsx! {
        div {
            class: "fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm",
            onclick: move |_| close(),
            div {
                role: if alert { "alertdialog" } else { "dialog" },
                "aria-modal": "true",
                "aria-labelledby": "{heading}",
                class: "w-full max-w-md space-y-4 rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg outline-none {class}",
                onmounted: move |event| {
                    focus.mounted(&event);
                    if let Some(handle) = event.data().downcast::<NodeHandle>() {
                        me.set(Some(handle.node_id()));
                        let start = start_in(&handle.doc(), handle.node_id());
                        if let Some(start) = start {
                            input.focus(start);
                        }
                    }
                },
                // A click inside isn't a click on the backdrop.
                onclick: move |event| event.stop_propagation(),
                onkeydown: move |event| {
                    if event.key() == Key::Named(NamedKey::Escape) {
                        event.prevent_default();
                        event.stop_propagation();
                        close();
                    }
                },
                ..attributes,
                h2 { id: "{heading}", class: "font-display text-xl font-semibold", "{title}" }
                {children}
            }
        }
    }
}

/// Asks before something that can't be undone, as the webview's
/// `ConfirmDialog`: Cancel is the first stop, for a pad and Enter alike; it
/// stays open while the action runs (`pending`), and the caller closes it.
#[component]
pub fn ConfirmDialog(
    title: String,
    confirm_label: String,
    /// Shown on the confirm button while the action runs ("Removing…").
    #[props(default)]
    pending_label: Option<String>,
    #[props(default)] pending: bool,
    on_confirm: EventHandler<()>,
    on_close: EventHandler<()>,
    /// What goes, and what stays.
    children: Element,
) -> Element {
    let close = move |_| {
        if !pending {
            on_close.call(());
        }
    };
    let confirm = if pending {
        pending_label.clone().unwrap_or(confirm_label.clone())
    } else {
        confirm_label.clone()
    };
    rsx! {
        Dialog {
            title,
            alert: true,
            on_close: close,
            "aria-busy": flag(pending),
            div { class: "space-y-2 text-sm text-fg-muted", {children} }
            div { class: "flex justify-end gap-2",
                Button { disabled: pending, on_press: close, "data-focus-default": "true", {t!("Cancel")} }
                Button {
                    variant: ButtonVariant::Danger,
                    disabled: pending,
                    on_press: move |_| on_confirm.call(()),
                    "{confirm}"
                }
            }
        }
    }
}
