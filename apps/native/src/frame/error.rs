//! Failures in place of a view or the whole shell (`shell/ErrorFallback.tsx`):
//! a view that fails shows its error where it was, with the sidebar, top
//! bar and status bar still working; the shell failing replaces the window,
//! keeping its controls. (A panic in the UI restarts Caskade, #87.)

use crate::router::use_router;
use crate::ui::{Button, ButtonVariant};
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

/// What failed, for the notice.
fn message(errors: &ErrorContext) -> String {
    errors
        .error()
        .map(|error| error.to_string())
        .unwrap_or_else(|| t!("Unknown error"))
}

/// A view that failed: its error, Try again and Open the library.
pub fn view_error(errors: ErrorContext) -> Element {
    rsx! {
        ViewError { errors }
    }
}

#[component]
fn ViewError(errors: ErrorContext) -> Element {
    let router = use_router();
    let message = message(&errors);
    let retry = errors.clone();
    rsx! {
        div { "data-cs": "view-error", role: "alert", class: "grid flex-1 place-items-center",
            div { class: "max-w-md text-center",
                h1 { "data-part": "title", class: "font-display text-2xl font-semibold",
                    {t!("This view couldn't be shown")}
                }
                p { "data-part": "message", class: "mt-2 text-sm break-words text-fg-muted", "{message}" }
                div { "data-part": "actions", class: "mt-5 flex justify-center gap-2",
                    Button {
                        variant: ButtonVariant::Primary,
                        "data-focus-default": "true",
                        on_press: move |_| retry.clear_errors(),
                        {t!("Try again")}
                    }
                    Button {
                        variant: ButtonVariant::Secondary,
                        on_press: move |_| {
                            errors.clear_errors();
                            router.navigate("/library?filter=all");
                        },
                        {t!("Open the library")}
                    }
                }
            }
        }
    }
}

/// The shell itself failed: the whole window, keeping its controls.
pub fn app_crash(errors: ErrorContext) -> Element {
    rsx! {
        AppCrash { errors }
    }
}

#[component]
fn AppCrash(errors: ErrorContext) -> Element {
    let message = message(&errors);
    rsx! {
        div { "data-cs": "view-error", role: "alert", class: "flex h-screen flex-col",
            if super::window::CUSTOM_FRAME {
                div { class: "flex justify-end p-2",
                    super::window::WindowControls {}
                }
            }
            div { class: "grid flex-1 place-items-center",
                div { class: "max-w-md text-center",
                    h1 { "data-part": "title", class: "font-display text-2xl font-semibold",
                        {t!("Caskade ran into a problem")}
                    }
                    p { "data-part": "message", class: "mt-2 text-sm break-words text-fg-muted", "{message}" }
                    div { "data-part": "actions", class: "mt-5 flex justify-center",
                        Button {
                            variant: ButtonVariant::Primary,
                            "data-focus-default": "true",
                            on_press: move |_| errors.clear_errors(),
                            {t!("Reload")}
                        }
                    }
                }
            }
        }
    }
}
