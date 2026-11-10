//! Toasts: short messages over the corner of the window that go by
//! themselves, as the webview's notifications: a polite live region (an
//! error's is assertive), so screen readers say each as it comes, each with
//! an optional action and a close button. [`ToastRegion`] holds them;
//! [`use_toasts`] shows one from anywhere inside. No catalogued root: the
//! app shell's `notifications` and `notification` (#96) render with it.

use std::time::Duration;

use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::button::{Button, ButtonSize, ButtonVariant};
use super::icons::X;

/// How long a toast stays, unless it has an action.
const SHOWN: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    /// A failure: assertive, and it stays until closed.
    pub error: bool,
    /// An action's label and id (`on_action` gets the id).
    pub action: Option<(String, String)>,
}

/// The toasts shown now.
#[derive(Clone, Copy, PartialEq)]
pub struct Toasts {
    list: Signal<Vec<Toast>>,
    next: Signal<u64>,
}

impl Toasts {
    /// Shows `message`; an error stays until closed, an action until used.
    pub fn show(
        mut self,
        message: impl Into<String>,
        error: bool,
        action: Option<(String, String)>,
    ) -> u64 {
        let id = *self.next.peek();
        self.next.set(id + 1);
        let stays = error || action.is_some();
        self.list.write().push(Toast {
            id,
            message: message.into(),
            error,
            action,
        });
        if !stays {
            spawn(async move {
                // On the app's runtime: a timer needs its reactor.
                let _ =
                    caskade_app::runtime::spawn(async { tokio::time::sleep(SHOWN).await }).await;
                self.dismiss(id);
            });
        }
        id
    }

    pub fn dismiss(mut self, id: u64) {
        self.list.write().retain(|toast| toast.id != id);
    }
}

/// The toasts' state, inside a [`ToastRegion`].
pub fn use_toasts() -> Toasts {
    use_context::<Toasts>()
}

/// Where toasts show (bottom right), around the part of the UI that shows
/// them. `on_action` gets an action's id.
#[component]
pub fn ToastRegion(
    #[props(default)] on_action: Option<EventHandler<String>>,
    children: Element,
) -> Element {
    let toasts = use_context_provider(|| Toasts {
        list: Signal::new(Vec::new()),
        next: Signal::new(1),
    });
    let list = toasts.list;
    rsx! {
        {children}
        div {
            "data-part": "toasts",
            class: "pointer-events-none fixed right-4 bottom-12 z-50 flex w-80 flex-col gap-2",
            // Polite and assertive regions are there from the start, so a
            // toast that comes in is announced.
            div { role: "status", "aria-live": "polite", class: "flex flex-col gap-2",
                for toast in list().into_iter().filter(|toast| !toast.error) {
                    ToastCard { key: "{toast.id}", toast, toasts, on_action }
                }
            }
            div { role: "alert", "aria-live": "assertive", class: "flex flex-col gap-2",
                for toast in list().into_iter().filter(|toast| toast.error) {
                    ToastCard { key: "{toast.id}", toast, toasts, on_action }
                }
            }
        }
    }
}

#[component]
fn ToastCard(toast: Toast, toasts: Toasts, on_action: Option<EventHandler<String>>) -> Element {
    let id = toast.id;
    rsx! {
        div {
            "data-part": "toast",
            "data-tone": toast.error.then_some("danger"),
            class: "pointer-events-auto flex items-start gap-3 rounded-panel border border-border-strong bg-surface-overlay p-3 text-sm text-fg shadow-lg animate-enter data-[tone=danger]:border-danger/50",
            p { "data-part": "message", class: "min-w-0 flex-1 break-words", "{toast.message}" }
            if let Some((label, action)) = toast.action.clone() {
                Button {
                    size: ButtonSize::Sm,
                    on_press: move |_| {
                        if let Some(on_action) = on_action {
                            on_action.call(action.clone());
                        }
                        toasts.dismiss(id);
                    },
                    "{label}"
                }
            }
            Button {
                size: ButtonSize::Sm,
                variant: ButtonVariant::Ghost,
                icon: X,
                "aria-label": t!(note = "Closes a notification", "Dismiss"),
                on_press: move |_| toasts.dismiss(id),
            }
        }
    }
}
